import Foundation
import Testing
import WhisperTranscription
@testable import Dictator

/// Phonon-2 in the picker and the bundled manifest. Offline: reads only the manifest file.
struct Phonon2CatalogTests {
    static let revision = "a812a0dfef205660787ef6234317ab79acf4d5d6"

    static func manifest() throws -> ModelManifest {
        let url = URL(filePath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appending(path: "scripts/model-manifest.txt")
        return try ModelManifest(text: String(contentsOf: url, encoding: .utf8))
    }

    @Test func phonon2IsEnglishOnlyAndCredited() {
        #expect(SpeechModel.phonon2.rawValue == "phonon-2")
        #expect(SpeechModel.phonon2.isEnglishOnly)
        #expect(SpeechModel.parakeetV2.isEnglishOnly)
        #expect(!SpeechModel.whisperLargeV3Turbo.isEnglishOnly)
        #expect(SpeechModel.phonon2.source.contains("CC-BY-4.0"))
    }

    @Test func everyPhonon2FileIsPinnedToOneImmutableRevision() throws {
        let files = try Self.manifest().files(for: SpeechModel.phonon2.rawValue)
        #expect(files.count == 18)
        for file in files {
            #expect(
                file.remotePath == "FluidInference/phonon-2-coreml/resolve/\(Self.revision)/\(file.localPath)",
                "\(file.remotePath) is not pinned to \(Self.revision)"
            )
        }
    }

    @Test func phonon2ManifestHoldsWhatFluidAudioLoads() throws {
        let local = try Self.manifest().files(for: SpeechModel.phonon2.rawValue).map(\.localPath)
        for component in ["Preprocessor", "Encoder", "Decoder", "JointDecisionv3"] {
            for part in ["coremldata.bin", "model.mil", "weights/weight.bin"] {
                #expect(local.contains("\(component).mlmodelc/\(part)"), "missing \(component).mlmodelc/\(part)")
            }
        }
        #expect(local.contains("parakeet_vocab.json"))
        // Parakeet v2's joint is a different contract; it must not be mixed in.
        #expect(!local.contains { $0.hasPrefix("JointDecision.mlmodelc/") })
    }

    @Test func phonon2IsSmallerThanParakeet() throws {
        let manifest = try Self.manifest()
        let phonon = manifest.totalBytes(for: SpeechModel.phonon2.rawValue)
        #expect(phonon == 357_573_278)
        #expect(phonon < manifest.totalBytes(for: SpeechModel.parakeetV2.rawValue))
        // The default sparse-g8 encoder, not a smaller, slower variant.
        let encoder = manifest.files(for: SpeechModel.phonon2.rawValue).first { $0.localPath == "Encoder.mlmodelc/weights/weight.bin" }
        #expect(encoder?.bytes == 319_611_456)
        #expect(encoder?.sha256 == "d76b3604417342bf06dc39c01cdb8a6f5d129c835c4442cf8ff92c08d51b8889")
    }
}
