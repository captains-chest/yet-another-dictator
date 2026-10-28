import DictationCore

/// The global shortcut as `ShortcutSettings` sees it, so its policy can be tested with a fake.
@MainActor
protocol HotkeyRegistering: AnyObject {
    /// Watches for `shortcut` and the optional `undo` shortcut, replacing any earlier pair.
    func register(_ shortcut: Shortcut, undo: Shortcut?) throws
    func unregister()
    var isRegistered: Bool { get }
    /// How the shortcut is watched; nil until registered once.
    var mode: EventTapHotkey.Mode? { get }
}

extension EventTapHotkey: HotkeyRegistering {}
