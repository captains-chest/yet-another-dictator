import DictationCore
import Foundation

/// The transcriber the controller talks to. It forwards to the engine for the chosen model, so
/// switching models in Settings swaps the engine without rebuilding the controller.
actor ActiveTranscriber: Transcribing {
    struct NoModel: Error, CustomStringConvertible {
        var description: String { "Choose a speech model (Settings → Model)." }
    }

    private var engine: (any Transcribing)?

    func use(_ engine: any Transcribing) {
        self.engine = engine
    }

    func prepare() async throws {
        guard let engine else { throw NoModel() }
        try await engine.prepare()
    }

    func transcribe(_ samples: [Float], language: String?) async throws -> String {
        guard let engine else { throw NoModel() }
        return try await engine.transcribe(samples, language: language)
    }
}
