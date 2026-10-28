import Foundation
import Observation

/// The speech models Dictator can download, best first. Nothing is downloaded until the user picks
/// one (ADR 0006). Each case's raw value is its model-id in `model-manifest.txt`.
enum SpeechModel: String, CaseIterable, Identifiable, Sendable {
    // Declaration order is the ranking shown in the picker: best first.
    // Parakeet v2 stays first: its accuracy is measured on the official Open ASR Leaderboard.
    // Phonon-2 is smaller and newer, but its better average comes from its maker's own run, so it
    // ranks second until it beats Parakeet on our own dictations (ADR 0006, 2026-10-23 note).
    case parakeetV2 = "parakeet-tdt-0.6b-v2"
    case phonon2 = "phonon-2"
    case whisperLargeV3Turbo = "openai_whisper-large-v3-v20240930_turbo"

    var id: String { rawValue }

    /// Best first.
    static var ranked: [SpeechModel] { allCases }

    var displayName: String {
        switch self {
        case .parakeetV2: "NVIDIA Parakeet TDT 0.6B v2"
        case .phonon2: "Phonon-2"
        case .whisperLargeV3Turbo: "Whisper Large v3 Turbo"
        }
    }

    var accuracyNote: String {
        switch self {
        case .parakeetV2: "Most accurate for English (6.1% average word error rate on the Open ASR Leaderboard)."
        case .phonon2: "Smaller Parakeet for English. Its maker reports 5.2% average word error rate (not an official leaderboard result)."
        case .whisperLargeV3Turbo: "Accurate (7.8% average word error rate in English)."
        }
    }

    var speedNote: String {
        switch self {
        case .parakeetV2: "Fastest: well under a second per minute of speech on Apple silicon."
        case .phonon2: "About as fast as Parakeet. The first load takes about a minute while macOS prepares it."
        case .whisperLargeV3Turbo: "Fast, but several times slower than Parakeet."
        }
    }

    var isEnglishOnly: Bool { self != .whisperLargeV3Turbo }

    var languageNote: String { isEnglishOnly ? "English only" : "Automatic or English" }

    /// Whose model it is, for the picker.
    var source: String {
        switch self {
        case .parakeetV2: "NVIDIA, CC-BY-4.0"
        case .phonon2: "Fermion Research, from NVIDIA Parakeet, CC-BY-4.0"
        case .whisperLargeV3Turbo: "OpenAI, MIT"
        }
    }
}
