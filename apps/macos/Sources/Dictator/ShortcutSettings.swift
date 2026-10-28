import DictationCore
import Foundation
import Observation
import OSLog

/// The dictation and undo shortcuts: what is stored, what is registered, and the errors shown in
/// Settings. A change is registered first and only persisted once registration succeeded, so a
/// failed change never loses the existing shortcuts.
@MainActor
@Observable
final class ShortcutSettings {
    private(set) var shortcut: Shortcut
    /// Optional global shortcut for "Undo Last Dictation".
    private(set) var undoShortcut: Shortcut?
    private(set) var undoShortcutError: String?
    private(set) var shortcutError: String?
    /// How the shortcut is being watched; nil until the hotkey was registered once.
    private(set) var hotkeyMode: EventTapHotkey.Mode?

    @ObservationIgnored private let hotkey: any HotkeyRegistering
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private var isSuspended = false
    /// False until the app has started (this copy is the only one running).
    @ObservationIgnored private(set) var isStarted = false

    static let shortcutKey = "shortcut"
    static let undoShortcutKey = "undoShortcut"
    static let emptyMessage = "Press at least one key."
    static let undoIsDictationMessage = "That's the dictation shortcut."
    private static let log = Logger(subsystem: "com.captainduckay.dictator", category: "state")

    init(hotkey: any HotkeyRegistering, defaults: UserDefaults) {
        self.hotkey = hotkey
        self.defaults = defaults
        shortcut = Self.loadShortcut(from: defaults, key: Self.shortcutKey) ?? .default
        undoShortcut = Self.loadShortcut(from: defaults, key: Self.undoShortcutKey)
    }

    /// Called once the app has started; registers the stored shortcuts.
    func start() {
        isStarted = true
        resumeHotkey()
    }

    func setShortcut(_ new: Shortcut) {
        isSuspended = false
        do {
            try new.validate()
        } catch {
            shortcutError = Self.emptyMessage
            resumeHotkey()
            shortcutError = Self.emptyMessage
            return
        }
        // A dictation shortcut that takes over the undo shortcut's keys removes the undo one, but
        // only once the new pair is actually registered.
        let dropsUndo = new.codes == undoShortcut?.codes
        let undo = dropsUndo ? nil : undoShortcut
        do {
            try hotkey.register(new, undo: undo)
        } catch {
            // Nothing changes: state and storage keep the previous pair, which is registered again.
            let message = String(describing: error)
            try? hotkey.register(shortcut, undo: undoShortcut)
            noteHotkeyMode()
            shortcutError = message
            return
        }
        noteHotkeyMode()
        if dropsUndo {
            undoShortcut = nil
            undoShortcutError = nil
            defaults.removeObject(forKey: Self.undoShortcutKey)
        }
        shortcut = new
        shortcutError = nil
        if let data = try? JSONEncoder().encode(new) { defaults.set(data, forKey: Self.shortcutKey) }
    }

    func setUndoShortcut(_ new: Shortcut) {
        isSuspended = false
        if new.keys.isEmpty {
            undoShortcutError = Self.emptyMessage
        } else if new.codes == shortcut.codes {
            undoShortcutError = Self.undoIsDictationMessage
        } else {
            undoShortcut = new
            undoShortcutError = nil
            if let data = try? JSONEncoder().encode(new) { defaults.set(data, forKey: Self.undoShortcutKey) }
        }
        resumeHotkey()
    }

    func clearUndoShortcut() {
        undoShortcut = nil
        undoShortcutError = nil
        defaults.removeObject(forKey: Self.undoShortcutKey)
        if hotkey.isRegistered { resumeHotkey() }
    }

    /// Unregisters the global shortcut while the user records a new one.
    func suspendHotkey() {
        isSuspended = true
        hotkey.unregister()
    }

    func resumeHotkey() {
        isSuspended = false
        do {
            try hotkey.register(shortcut, undo: undoShortcut)
            noteHotkeyMode()
            shortcutError = nil
        } catch {
            shortcutError = String(describing: error)
        }
    }

    /// Retries registration, e.g. after the user granted Input Monitoring.
    func retryHotkeyIfNeeded() {
        if isStarted, !hotkey.isRegistered, !isSuspended { resumeHotkey() }
    }

    private func noteHotkeyMode() {
        guard hotkeyMode != hotkey.mode else { return }
        hotkeyMode = hotkey.mode
        let mode = hotkey.mode.map { "\($0)" } ?? "none"
        Self.log.notice("hotkey tap: \(mode, privacy: .public)")
    }

    static func loadShortcut(from defaults: UserDefaults, key: String) -> Shortcut? {
        guard
            let data = defaults.data(forKey: key),
            let stored = try? JSONDecoder().decode(Shortcut.self, from: data),
            (try? stored.validate()) != nil
        else { return nil }
        return stored
    }
}
