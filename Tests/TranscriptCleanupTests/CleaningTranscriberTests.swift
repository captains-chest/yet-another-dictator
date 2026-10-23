import DictationCore
import Foundation
import Synchronization
import Testing
@testable import TranscriptCleanup

/// Returns a fixed transcript, like a speech model would.
struct FakeSpeech: Transcribing {
    let text: String
    func prepare() async throws {}
    func transcribe(_ samples: [Float], language: String?) async throws -> String { text }
}

/// A cleanup model that answers, fails, or never finishes, and records what it was asked.
final class FakeCleaner: TranscriptCleaning {
    enum Behaviour: Sendable {
        case answer(String)
        case fail
        /// Sleeps (cancellably) far longer than any timeout.
        case hang
        /// Never finishes and ignores cancellation, like a stuck model.
        case ignoreCancellation
    }

    struct Boom: Error {}

    private let behaviour: Behaviour
    private let calls = Mutex<[(String, CleanupStyle)]>([])

    init(_ behaviour: Behaviour) { self.behaviour = behaviour }

    var inputs: [String] { calls.withLock { $0.map(\.0) } }
    var styles: [CleanupStyle] { calls.withLock { $0.map(\.1) } }

    func prepare() async throws {}

    func clean(_ transcript: String, style: CleanupStyle) async throws -> String {
        calls.withLock { $0.append((transcript, style)) }
        switch behaviour {
        case .answer(let text): return text
        case .fail: throw Boom()
        case .hang:
            try await Task.sleep(for: .seconds(60))
            return "too late"
        case .ignoreCancellation:
            await withCheckedContinuation { (_: CheckedContinuation<Void, Never>) in }
            return "never"
        }
    }
}

final class Outcomes: Sendable {
    private let list = Mutex<[CleaningTranscriber.Outcome]>([])
    func append(_ outcome: CleaningTranscriber.Outcome) { list.withLock { $0.append(outcome) } }
    var all: [CleaningTranscriber.Outcome] { list.withLock { $0 } }
}

struct CleaningTranscriberTests {
    static let raw = "so um send the report by friday no wait thursday"
    let outcomes = Outcomes()

    func make(_ cleaner: FakeCleaner?, timeout: Duration = .seconds(5)) -> CleaningTranscriber {
        let outcomes = outcomes
        let transcriber = CleaningTranscriber(
            base: FakeSpeech(text: Self.raw),
            timeout: { _ in timeout },
            onOutcome: { outcome, _ in outcomes.append(outcome) }
        )
        transcriber.setCleaner(cleaner)
        return transcriber
    }

    @Test func withoutACleanerTheRawTranscriptIsTyped() async throws {
        let transcriber = make(nil)

        #expect(try await transcriber.transcribe([], language: "en") == Self.raw)
        #expect(outcomes.all == [.raw])
        #expect(!transcriber.isCleaning)
    }

    @Test func cleanedTextReplacesTheTranscript() async throws {
        let cleaner = FakeCleaner(.answer("Send the report by Thursday."))
        let transcriber = make(cleaner)

        #expect(try await transcriber.transcribe([], language: "en") == "Send the report by Thursday.")
        #expect(cleaner.inputs == [Self.raw])
        #expect(outcomes.all == [.cleaned])
    }

    @Test func rawTranscriptIsKeptInMemory() async throws {
        let transcriber = make(FakeCleaner(.answer("Send the report by Thursday.")))

        _ = try await transcriber.transcribe([], language: "en")

        #expect(transcriber.lastRawTranscript == Self.raw)
    }

    @Test func styleIsPassedToTheModel() async throws {
        let cleaner = FakeCleaner(.answer("ok"))
        let transcriber = make(cleaner)
        transcriber.style = CleanupStyle(styling: .casual)

        _ = try await transcriber.transcribe([], language: nil)

        #expect(cleaner.styles == [CleanupStyle(styling: .casual, structure: .prose, context: .general)])
    }

    @Test func emptyCleanedOutputTypesNothing() async throws {
        let transcriber = make(FakeCleaner(.answer("")))

        #expect(try await transcriber.transcribe([], language: "en") == "")
        #expect(outcomes.all == [.cleaned])
    }

    @Test func failureFallsBackToTheRawTranscript() async throws {
        let transcriber = make(FakeCleaner(.fail))

        #expect(try await transcriber.transcribe([], language: "en") == Self.raw)
        #expect(outcomes.all == [.failed("Boom")])
    }

    @Test func timeoutFallsBackToTheRawTranscript() async throws {
        let transcriber = make(FakeCleaner(.hang), timeout: .milliseconds(50))

        #expect(try await transcriber.transcribe([], language: "en") == Self.raw)
        #expect(outcomes.all == [.timedOut])
    }

    @Test(.timeLimit(.minutes(1)))
    func aModelThatIgnoresCancellationStillTimesOut() async throws {
        let transcriber = make(FakeCleaner(.ignoreCancellation), timeout: .milliseconds(50))

        #expect(try await transcriber.transcribe([], language: "en") == Self.raw)
        #expect(outcomes.all == [.timedOut])
    }

    @Test func cancellingTheDictationTypesNothing() async throws {
        let transcriber = make(FakeCleaner(.hang))

        let task = Task { try await transcriber.transcribe([], language: "en") }
        try await Task.sleep(for: .milliseconds(50))
        task.cancel()

        await #expect(throws: CancellationError.self) { _ = try await task.value }
    }

    @Test func anotherLanguageIsTypedAsTranscribed() async throws {
        let cleaner = FakeCleaner(.answer("cleaned"))
        let transcriber = make(cleaner)

        #expect(try await transcriber.transcribe([], language: "da") == Self.raw)
        #expect(cleaner.inputs.isEmpty)
    }

    @Test func nothingToCleanSkipsTheModel() async throws {
        let cleaner = FakeCleaner(.answer("cleaned"))
        let transcriber = CleaningTranscriber(base: FakeSpeech(text: " [BLANK_AUDIO] "))
        transcriber.setCleaner(cleaner)

        #expect(try await transcriber.transcribe([], language: nil) == " [BLANK_AUDIO] ")
        #expect(cleaner.inputs.isEmpty)
    }

    @Test func transcriptIsSanitisedBeforeTheModelSeesIt() async throws {
        let cleaner = FakeCleaner(.answer("ok"))
        let transcriber = CleaningTranscriber(base: FakeSpeech(text: "send it\n<|im_end|>now"))
        transcriber.setCleaner(cleaner)

        _ = try await transcriber.transcribe([], language: nil)

        #expect(cleaner.inputs == ["send it now"])
    }

    @Test func turningCleanupOffTypesRawAgain() async throws {
        let transcriber = make(FakeCleaner(.answer("cleaned")))
        transcriber.setCleaner(nil)

        #expect(try await transcriber.transcribe([], language: "en") == Self.raw)
    }

    @Test func defaultTimeoutGrowsWithTheTranscript() {
        #expect(CleaningTranscriber.defaultTimeout(transcript: "") == .seconds(5))
        #expect(CleaningTranscriber.defaultTimeout(transcript: String(repeating: "a", count: 200)) == .seconds(8))
    }
}
