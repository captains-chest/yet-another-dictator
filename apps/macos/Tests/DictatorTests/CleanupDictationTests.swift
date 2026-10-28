import DictationCore
import Foundation
import Testing
import TranscriptCleanup
@testable import Dictator

/// Hands back one second of audio.
@MainActor
final class CleanupFakeRecorder: AudioRecording {
    func start(onLevel: @escaping @Sendable (Float) -> Void) throws {}
    func stop() -> [Float] { Array(repeating: 0.1, count: 16_000) }
}

/// Records what was typed and deleted.
@MainActor
final class CleanupFakeInserter: TextInserting {
    var inserted: [String] = []
    var deleted: [Int] = []
    func preflight() throws {}
    func insert(_ text: String) async throws { inserted.append(text) }
    func deleteBackward(_ count: Int) async throws { deleted.append(count) }
}

@MainActor
final class CleanupNoPrecedingText: PrecedingTextReading {
    func textBeforeCursor(maxLength: Int) -> String? { nil }
}

/// Fails every cleanup.
struct CleanupBrokenCleaner: TranscriptCleaning {
    struct Broken: Error {}
    func prepare() async throws {}
    func clean(_ transcript: String, style: CleanupStyle) async throws -> String { throw Broken() }
}

/// The whole dictation path with cleanup in it: what is typed, and that undo removes exactly that.
@MainActor
struct CleanupDictationTests {
    static let raw = "um send the report by friday no wait thursday"

    let recorder = CleanupFakeRecorder()
    let inserter = CleanupFakeInserter()
    let reader = CleanupNoPrecedingText()

    func dictate(with cleaner: (any TranscriptCleaning)?) async -> DictationController {
        let transcriber = CleaningTranscriber(base: CleanupFixedSpeech(text: Self.raw))
        transcriber.setCleaner(cleaner)
        let controller = DictationController(
            recorder: recorder, transcriber: transcriber, inserter: inserter, precedingText: reader,
            stopTail: .zero, containsSpeech: { _ in true }
        )
        await controller.loadModel()
        controller.hotkeyPressed()
        controller.hotkeyPressed()
        for _ in 0..<500 where controller.state != .ready {
            try? await Task.sleep(for: .milliseconds(10))
        }
        return controller
    }

    @Test func theCleanedTextIsTypedAndUndoRemovesExactlyThat() async {
        let controller = await dictate(with: FakeTextCleaner(answer: "I need to send the report by Thursday."))
        #expect(inserter.inserted == ["I need to send the report by Thursday."])
        #expect(controller.canUndo)
        await controller.undoLastDictation()
        #expect(inserter.deleted == ["I need to send the report by Thursday.".count])
    }

    @Test func aFailingCleanupTypesTheRawTranscriptAndUndoRemovesThat() async {
        let controller = await dictate(with: CleanupBrokenCleaner())
        #expect(inserter.inserted.count == 1)
        #expect(inserter.inserted.first?.lowercased() == Self.raw)
        await controller.undoLastDictation()
        #expect(inserter.deleted == [inserter.inserted.first?.count ?? -1])
    }

    @Test func anEmptyCleanupTypesNothing() async {
        let controller = await dictate(with: FakeTextCleaner(answer: ""))
        #expect(inserter.inserted.isEmpty)
        #expect(!controller.canUndo)
    }

    @Test func withoutCleanupTheRawTranscriptIsTyped() async {
        _ = await dictate(with: nil)
        #expect(inserter.inserted.first?.lowercased() == Self.raw)
    }
}
