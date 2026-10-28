import DictationCore
import Testing
@testable import Dictator

/// Issue #43: a long dictation came out with one 20-character chunk moved to the end. Chunks must
/// be posted strictly in order with a pause between each, and nothing may be lost.
@MainActor
struct KeystrokeTextInserterTests {
    @MainActor final class Recorder {
        var events: [String] = []
    }

    func makeInserter(_ recorder: Recorder) -> KeystrokeTextInserter {
        KeystrokeTextInserter(
            canPost: { true },
            post: { keystroke in
                switch keystroke {
                case .text(let chunk): recorder.events.append("post:\(chunk)")
                case .backspace: recorder.events.append("backspace")
                }
            },
            sleep: { _ in recorder.events.append("pause") }
        )
    }

    /// The dictation from the issue.
    let text = "We should also note that we want the M1, 2, 3 and 4 logos to be on the card so we can showcase what lines you actually are available to take."

    @Test func typesEveryChunkInOrderWithAPauseBetweenEach() async throws {
        let recorder = Recorder()
        try await makeInserter(recorder).insert(text)

        let chunks = TextChunker.chunks(text)
        #expect(chunks.count > 4)
        var expected: [String] = []
        for (index, chunk) in chunks.enumerated() {
            if index > 0 { expected.append("pause") }
            expected.append("post:\(chunk)")
        }
        #expect(recorder.events == expected)
        let typed = recorder.events.compactMap { $0.hasPrefix("post:") ? String($0.dropFirst(5)) : nil }.joined()
        #expect(typed == text)
    }

    @Test func pacesBackspacesToo() async throws {
        let recorder = Recorder()
        try await makeInserter(recorder).deleteBackward(3)
        #expect(recorder.events == ["backspace", "pause", "backspace", "pause", "backspace"])
    }

    @Test func cancellingStopsTypingBetweenChunks() async throws {
        let recorder = Recorder()
        let task = Task { @MainActor in
            try await KeystrokeTextInserter(
                canPost: { true },
                post: { keystroke in
                    if case .text(let chunk) = keystroke { recorder.events.append(chunk) }
                    if recorder.events.count == 2 { withUnsafeCurrentTask { $0?.cancel() } }
                },
                sleep: { _ in }
            ).insert(text)
        }
        await #expect(throws: CancellationError.self) { try await task.value }
        #expect(recorder.events.count == 2)
    }

    @Test func insertWithoutAccessibilityThrowsAndAsksOnce() async {
        let recorder = Recorder()
        let inserter = KeystrokeTextInserter(
            canPost: { false },
            requestPost: { recorder.events.append("request") },
            post: { _ in recorder.events.append("post") },
            sleep: { _ in }
        )
        await #expect(throws: KeystrokeTextInserter.Failure.notPermitted) { try await inserter.insert("hello") }
        #expect(recorder.events == ["request"])
    }
}
