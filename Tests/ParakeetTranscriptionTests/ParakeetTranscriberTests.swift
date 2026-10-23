import Foundation
import ModelSession
import ParakeetTranscription
import Testing

struct ParakeetModelTests {
    func missingFilesError(_ make: (URL) -> ModelSession) async -> String? {
        let folder = FileManager.default.temporaryDirectory.appending(path: "parakeet-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        do {
            try await make(folder).prepare()
            return nil
        } catch {
            return "\(error)"
        }
    }

    @Test func incompleteFolderIsRefusedBeforeLoading() async {
        let error = await missingFilesError(ModelSession.parakeet(modelFolder:))
        #expect(error?.contains("parakeet_vocab.json") == true, "factory files should name parakeet_vocab.json, got: \(String(describing: error))")
        #expect(error?.contains("JointDecision.mlmodelc") == true, "got: \(String(describing: error))")
        #expect(error?.contains("JointDecisionv3") == false, "got: \(String(describing: error))")
    }

    @Test func phonon2FolderNeedsTheV3Joint() async {
        let error = await missingFilesError(ModelSession.phonon2(modelFolder:))
        #expect(error?.contains("JointDecisionv3.mlmodelc") == true, "got: \(String(describing: error))")
        #expect(error?.contains("parakeet_vocab.json") == true, "got: \(String(describing: error))")
    }
}
