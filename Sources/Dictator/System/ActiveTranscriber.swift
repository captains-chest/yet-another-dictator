import DictationCore
import Foundation
import Synchronization

/// The transcriber the controller talks to. It forwards to the engine for the chosen model, so
/// switching models in Settings swaps the engine without rebuilding the controller (ADR 0009).
/// The swap is synchronous, so a loaded model serves at once and a recording in progress is
/// never disturbed by a switch.
final class ActiveTranscriber: Transcribing, Sendable {
    struct NoModel: Error, CustomStringConvertible {
        var description: String { "Choose a speech model (Settings → Model)." }
    }

    /// A model that failed to load; the reason is what the user is told.
    struct LoadFailed: Error, CustomStringConvertible {
        let reason: String
        var description: String { reason }
    }

    private enum Slot {
        case empty(reason: String?)
        case serving(any Transcribing)
    }

    private let slot = Mutex<Slot>(.empty(reason: nil))

    /// Serves `engine` at once, clearing any load-failure reason.
    func use(_ engine: any Transcribing) {
        slot.withLock { $0 = .serving(engine) }
    }

    /// Records why no model can serve. Only meaningful while empty: a serving model keeps serving.
    func unavailable(_ reason: String) {
        slot.withLock {
            if case .empty = $0 { $0 = .empty(reason: reason) }
        }
    }

    func prepare() async throws {
        switch slot.withLock({ $0 }) {
        case .serving(let engine): try await engine.prepare()
        case .empty(let reason):
            if let reason { throw LoadFailed(reason: reason) }
            throw NoModel()
        }
    }

    func transcribe(_ samples: [Float], language: String?) async throws -> String {
        switch slot.withLock({ $0 }) {
        case .serving(let engine): return try await engine.transcribe(samples, language: language)
        case .empty(let reason):
            if let reason { throw LoadFailed(reason: reason) }
            throw NoModel()
        }
    }
}
