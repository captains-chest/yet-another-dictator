import Foundation
@testable import S1MiniCleanup
import Testing
import TranscriptCleanup

struct S1MiniCleanerTests {
    /// `S1_MINI_MODEL_FOLDER`, or `Model/`, where `scripts/fetch-model.sh s1-mini` puts the files
    /// (that script keeps only one model in `Model/`). The end-to-end tests are skipped when the
    /// model isn't there, like the WhisperKit ones.
    static let modelFolder: URL = {
        if let path = ProcessInfo.processInfo.environment["S1_MINI_MODEL_FOLDER"] {
            return URL(filePath: path, directoryHint: .isDirectory)
        }
        return URL(filePath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appending(path: "Model", directoryHint: .isDirectory)
    }()

    static var modelAvailable: Bool {
        FileManager.default.fileExists(atPath: modelFolder.appending(path: S1MiniCleaner.modelFileName).path(percentEncoded: false))
    }

    @Test func missingModelFileFailsWithoutLoading() async throws {
        let empty = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let cleaner = S1MiniCleaner(modelFolder: empty)

        await #expect(throws: S1MiniCleaner.Failure.self) { try await cleaner.prepare() }
        await #expect(throws: S1MiniCleaner.Failure.self) { _ = try await cleaner.clean("hello", style: .default) }
    }

    /// The examples from the model card, under the default semi-formal / prose / general.
    @Test(.enabled(if: modelAvailable), .timeLimit(.minutes(5)), arguments: [
        ("so um i need to like send the the report by uh friday no wait make that thursday", "I need to send the report by Thursday."),
        ("i think the answer is forty two no sorry forty three", "I think the answer is 43."),
        ("let's meet at half past two tomorrow uh actually make it three fifteen p m", "Let's meet at 3:15pm tomorrow."),
        ("send it to support at superwhisper dot com", "Send it to support@superwhisper.com."),
        ("um", ""),
    ])
    func cleansLikeTheModelCard(_ raw: String, _ expected: String) async throws {
        let cleaner = S1MiniCleaner(modelFolder: Self.modelFolder)

        let cleaned = try await cleaner.clean(raw, style: .default)

        #expect(cleaned == expected, "raw: \(raw)")
    }

    @Test(.enabled(if: modelAvailable), .timeLimit(.minutes(5)))
    func longTranscriptIsCleanedInChunks() async throws {
        let cleaner = S1MiniCleaner(modelFolder: Self.modelFolder)
        let sentence = "so um the meeting is on tuesday at ten and we will uh review the budget."
        let raw = Array(repeating: sentence, count: 120).joined(separator: " ")

        let cleaned = try await cleaner.clean(raw, style: .default)

        #expect(!cleaned.isEmpty)
        #expect(!cleaned.lowercased().contains(" um "))
    }

    @Test(.enabled(if: modelAvailable), .timeLimit(.minutes(5)))
    func cancelledCleanupStops() async throws {
        let cleaner = S1MiniCleaner(modelFolder: Self.modelFolder)
        try await cleaner.prepare()
        let raw = Array(repeating: "and then uh we went to the shop and bought some bread", count: 60).joined(separator: " ")

        let task = Task { try await cleaner.clean(raw, style: .default) }
        try await Task.sleep(for: .milliseconds(50))
        task.cancel()

        await #expect(throws: CancellationError.self) { _ = try await task.value }
    }
}
