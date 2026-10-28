import DictationCore
@preconcurrency import FluidAudio
import Foundation
import ModelSession

/// The only code that uses FluidAudio: Parakeet-family Core ML models, loaded from a folder the app
/// downloaded and verified against its pinned manifest. Two models use it: NVIDIA Parakeet TDT
/// 0.6B v2 and Fermion Research's Phonon-2 (an English re-training of Parakeet TDT 0.6B v3).
///
/// FluidAudio never downloads anything here (`AsrModels.loadLocal`). Parakeet reads long audio in
/// its own overlapping windows, so the Whisper 30-second splitter is not used.
extension ModelSession {
    static let parakeetRequiredFiles = [
        "Preprocessor.mlmodelc",
        "Encoder.mlmodelc",
        "Decoder.mlmodelc",
        "JointDecision.mlmodelc",
        "parakeet_vocab.json",
    ]

    /// Phonon-2 uses the Parakeet v3 contract: the same files, but the v3 joint.
    static let phonon2RequiredFiles = [
        "Preprocessor.mlmodelc",
        "Encoder.mlmodelc",
        "Decoder.mlmodelc",
        "JointDecisionv3.mlmodelc",
        "parakeet_vocab.json",
    ]

    /// The Parakeet model as a session. `ModelSession` (ADR 0008) owns loading and ordering.
    public static func parakeet(modelFolder: URL) -> ModelSession {
        fluidAudio(modelFolder: modelFolder, requiredFiles: parakeetRequiredFiles, family: .parakeetV2)
    }

    /// The Phonon-2 model as a session. Needs macOS 15 (its encoder uses newer Core ML ops).
    public static func phonon2(modelFolder: URL) -> ModelSession {
        fluidAudio(modelFolder: modelFolder, requiredFiles: phonon2RequiredFiles, family: .phonon2)
    }

    /// Which FluidAudio model contract a folder holds. Our own type, so nothing from FluidAudio
    /// crosses into the session's load closure.
    enum Family: Sendable {
        case parakeetV2, phonon2

        var asrVersion: AsrModelVersion {
            switch self {
            case .parakeetV2: .v2
            case .phonon2: .phonon2
            }
        }
    }

    private static func fluidAudio(modelFolder: URL, requiredFiles: [String], family: Family) -> ModelSession {
        ModelSession(modelFolder: modelFolder, requiredFiles: requiredFiles) { folder in
            let models = try AsrModels.loadLocal(from: folder, version: family.asrVersion)
            let manager = AsrManager(config: .default, models: models)
            // Pay Core ML's one-off setup during "Loading model…", not on the first dictation.
            var state = TdtDecoderState.make()
            _ = try? await manager.transcribe([Float](repeating: 0, count: 16_000), decoderState: &state)
            return LoadedParakeet(manager)
        }
    }
}

/// A loaded Parakeet-family model.
final class LoadedParakeet: LoadedSpeechModel, @unchecked Sendable {
    private let manager: AsrManager

    init(_ manager: AsrManager) { self.manager = manager }

    /// `language` is ignored: these models are English only.
    func transcribe(_ samples: [Float], language: String?, stop: StopSignal) async throws -> String {
        var state = TdtDecoderState.make()
        let result = try await manager.transcribe(samples, decoderState: &state)
        if stop.isSet { throw CancellationError() }
        try Task.checkCancellation()
        return result.text.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
