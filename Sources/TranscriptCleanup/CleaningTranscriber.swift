import DictationCore
import Foundation
import Synchronization

/// Transcribes with the speech model, then, if cleanup is on and its model is ready, rewrites the
/// transcript with a `TranscriptCleaning` model (ADR 0011).
///
/// Cleanup never costs the user a dictation: if it is off, still downloading, fails or takes too
/// long, the raw transcript is returned instead. Only cancellation of the dictation itself is
/// passed on. The controller types whatever this returns and remembers exactly that for Undo, so
/// Undo removes the cleaned text when the cleaned text was typed.
public final class CleaningTranscriber: Transcribing {
    /// What happened to the last transcript. Contains no text, so it may be logged.
    public enum Outcome: Sendable, Equatable {
        /// Cleanup is off, its model isn't ready yet, or there was nothing to clean.
        case raw
        case cleaned
        /// Cleanup failed; the raw transcript was used.
        case failed(String)
        /// Cleanup took longer than its timeout; the raw transcript was used.
        case timedOut
    }

    private struct State {
        var cleaner: (any TranscriptCleaning)?
        var style = CleanupStyle.default
        var lastRawTranscript: String?
    }

    private let base: any Transcribing
    private let timeout: @Sendable (_ transcript: String) -> Duration
    private let state = Mutex(State())

    /// Called after each transcript with what cleanup did and how long it took. Never with text.
    private let onOutcome: @Sendable (Outcome, Duration) -> Void

    public init(
        base: any Transcribing,
        timeout: @escaping @Sendable (_ transcript: String) -> Duration = CleaningTranscriber.defaultTimeout,
        onOutcome: @escaping @Sendable (Outcome, Duration) -> Void = { _, _ in }
    ) {
        self.base = base
        self.timeout = timeout
        self.onOutcome = onOutcome
    }

    /// 5 s plus 15 ms per character. Typical dictations clean in well under a second; this only
    /// bounds a stuck or overloaded model, and stays far inside the controller's own watchdog.
    public static func defaultTimeout(transcript: String) -> Duration {
        .seconds(5) + .milliseconds(15 * transcript.count)
    }

    /// The model to clean with, or nil to type raw transcripts (cleanup off or not ready yet).
    public func setCleaner(_ cleaner: (any TranscriptCleaning)?) {
        state.withLock { $0.cleaner = cleaner }
    }

    public var isCleaning: Bool { state.withLock { $0.cleaner != nil } }

    public var style: CleanupStyle {
        get { state.withLock { $0.style } }
        set { state.withLock { $0.style = newValue } }
    }

    /// The last speech-model transcript before cleanup. In memory only, never written or logged.
    public var lastRawTranscript: String? { state.withLock { $0.lastRawTranscript } }

    public func prepare() async throws {
        try await base.prepare()
    }

    public func transcribe(_ samples: [Float], language: String?) async throws -> String {
        let raw = try await base.transcribe(samples, language: language)
        let (cleaner, style) = state.withLock { state in
            state.lastRawTranscript = raw
            return (state.cleaner, state.style)
        }
        // S1-mini is English only; an explicitly chosen other language is typed as transcribed.
        let isEnglish = language == nil || language == "en"
        let input = TranscriptCleaner.clean(raw)
        guard let cleaner, isEnglish, !input.isEmpty else {
            onOutcome(.raw, .zero)
            return raw
        }
        let clock = ContinuousClock()
        let start = clock.now
        let outcome: Outcome
        let text: String
        switch await Self.clean(input, style: style, with: cleaner, timeout: timeout(input)) {
        case .success(let cleaned):
            outcome = .cleaned
            text = cleaned
        case .failure(let error) where error is TimedOut:
            outcome = .timedOut
            text = raw
        case .failure(let error) where error is CancellationError && Task.isCancelled:
            // The dictation was cancelled, not just the cleanup: type nothing.
            throw CancellationError()
        case .failure(let error):
            outcome = .failed(String(describing: type(of: error)))
            text = raw
        }
        try Task.checkCancellation()
        onOutcome(outcome, clock.now - start)
        return text
    }

    struct TimedOut: Error {}

    /// Races the cleanup against `timeout` and returns whichever finishes first, without waiting
    /// for the loser: a model that ignores cancellation still can't hold up the dictation.
    private static func clean(
        _ input: String, style: CleanupStyle, with cleaner: any TranscriptCleaning, timeout: Duration
    ) async -> Result<String, any Error> {
        let first = FirstResult()
        let work = Task { try await cleaner.clean(input, style: style) }
        let timer = Task {
            if (try? await Task.sleep(for: timeout)) != nil { first.resolve(.failure(TimedOut())) }
        }
        Task { first.resolve(await work.result) }
        let result = await withTaskCancellationHandler {
            await first.value()
        } onCancel: {
            first.resolve(.failure(CancellationError()))
        }
        work.cancel()
        timer.cancel()
        return result
    }
}

/// The first of several racing results; later ones are ignored.
private final class FirstResult: Sendable {
    private struct State {
        var result: Result<String, any Error>?
        var waiter: CheckedContinuation<Result<String, any Error>, Never>?
    }

    private let state = Mutex(State())

    func resolve(_ result: Result<String, any Error>) {
        let waiter = state.withLock { state -> CheckedContinuation<Result<String, any Error>, Never>? in
            guard state.result == nil else { return nil }
            state.result = result
            defer { state.waiter = nil }
            return state.waiter
        }
        waiter?.resume(returning: result)
    }

    func value() async -> Result<String, any Error> {
        await withCheckedContinuation { continuation in
            let ready = state.withLock { state -> Result<String, any Error>? in
                if let result = state.result { return result }
                state.waiter = continuation
                return nil
            }
            if let ready { continuation.resume(returning: ready) }
        }
    }
}
