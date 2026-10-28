import CoreGraphics
import DictationCore
import Foundation

/// Types text into the focused field by synthesising Unicode key events.
///
/// Deliberately avoids the clipboard: pasting would expose every dictation to clipboard managers
/// and other apps reading the pasteboard, and would clobber the user's clipboard.
///
/// Each key event carries at most 20 UTF-16 units, so a dictation is typed as many small chunks.
/// Posting them in one burst let the system deliver a chunk late, after the ones behind it, so a
/// 20-character piece of a long dictation could end up at the end of the text (issue #43). Chunks
/// are therefore paced: each is posted only after a short pause following the previous one.
@MainActor
final class KeystrokeTextInserter: TextInserting {
    enum Failure: LocalizedError {
        case notPermitted, eventCreationFailed
        var errorDescription: String? {
            switch self {
            case .notPermitted: "Dictator needs Accessibility permission to type. Enable it in System Settings → Privacy & Security → Accessibility, then relaunch Dictator."
            case .eventCreationFailed: "Could not create keyboard events."
            }
        }
    }

    /// What gets posted: a Unicode chunk, or one Backspace.
    enum Keystroke: Equatable {
        case text(String)
        case backspace
    }

    /// Gap between two posted chunks. About 4 ms per 20 characters: a 1,000-character dictation
    /// takes a fifth of a second longer to type, and every chunk arrives in order.
    static let defaultInterval: Duration = .milliseconds(4)

    private let canPost: @MainActor () -> Bool
    private let requestPost: @MainActor () -> Void
    private let post: @MainActor (Keystroke) throws -> Void
    private let sleep: @MainActor (Duration) async throws -> Void
    private let interval: Duration

    init(
        canPost: @escaping @MainActor () -> Bool = { Permissions.canPostEvents },
        requestPost: @escaping @MainActor () -> Void = { Permissions.requestPostEvents() },
        post: @escaping @MainActor (Keystroke) throws -> Void = KeystrokeTextInserter.postEvents,
        sleep: @escaping @MainActor (Duration) async throws -> Void = { try await Task.sleep(for: $0) },
        interval: Duration = KeystrokeTextInserter.defaultInterval
    ) {
        self.canPost = canPost
        self.requestPost = requestPost
        self.post = post
        self.sleep = sleep
        self.interval = interval
    }

    func preflight() throws {
        guard canPost() else {
            requestPost()
            throw Failure.notPermitted
        }
    }

    func deleteBackward(_ count: Int) async throws {
        try preflight()
        try await type(Array(repeating: .backspace, count: count))
    }

    func insert(_ text: String) async throws {
        try preflight()
        try await type(TextChunker.chunks(text).map(Keystroke.text))
    }

    /// Posts the keystrokes strictly one after another, pausing between them. The pause also ends
    /// typing early when the task is cancelled (Cancel Dictation) and keeps the main thread free.
    private func type(_ keystrokes: [Keystroke]) async throws {
        for (index, keystroke) in keystrokes.enumerated() {
            if index > 0 { try await sleep(interval) }
            try Task.checkCancellation()
            try post(keystroke)
        }
    }

    /// The real poster: a key-down/key-up pair from a private event source, which keeps physically
    /// held modifiers (e.g. from the hotkey) out of our events.
    static func postEvents(_ keystroke: Keystroke) throws {
        let source = CGEventSource(stateID: .privateState)
        for keyDown in [true, false] {
            let event: CGEvent?
            switch keystroke {
            case .backspace:
                event = CGEvent(keyboardEventSource: source, virtualKey: 0x33, keyDown: keyDown) // kVK_Delete
            case .text(let chunk):
                event = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: keyDown)
                let utf16 = Array(chunk.utf16)
                event?.keyboardSetUnicodeString(stringLength: utf16.count, unicodeString: utf16)
            }
            guard let event else { throw Failure.eventCreationFailed }
            event.flags = []
            event.post(tap: .cghidEventTap)
        }
    }
}
