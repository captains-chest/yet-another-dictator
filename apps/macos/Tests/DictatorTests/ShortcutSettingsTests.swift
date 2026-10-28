import DictationCore
import Foundation
import Testing
@testable import Dictator

@MainActor
final class FakeHotkey: HotkeyRegistering {
    struct Refused: Error, CustomStringConvertible {
        var description: String { "refused" }
    }

    var registrations: [(Shortcut, Shortcut?)] = []
    var unregisterCount = 0
    /// Makes the next `register` call throw.
    var failNext = false
    var modeWhenRegistered: EventTapHotkey.Mode = .withholdsKeys
    private(set) var isRegistered = false
    private(set) var mode: EventTapHotkey.Mode?

    func register(_ shortcut: Shortcut, undo: Shortcut?) throws {
        registrations.append((shortcut, undo))
        if failNext {
            failNext = false
            throw Refused()
        }
        isRegistered = true
        mode = modeWhenRegistered
    }

    func unregister() {
        unregisterCount += 1
        isRegistered = false
    }
}

@MainActor
struct ShortcutSettingsTests {
    let defaults: UserDefaults
    let hotkey = FakeHotkey()

    init() {
        defaults = UserDefaults(suiteName: "ShortcutSettingsTests-\(UUID().uuidString)")!
    }

    static func key(_ code: UInt16, _ label: String) -> Shortcut.Key { .init(code: code, label: label) }
    let f5 = Shortcut(keys: [key(96, "F5")])
    let f6 = Shortcut(keys: [key(97, "F6")])
    let f7 = Shortcut(keys: [key(98, "F7")])

    func make() -> ShortcutSettings { ShortcutSettings(hotkey: hotkey, defaults: defaults) }

    func stored(_ key: String) -> Shortcut? { ShortcutSettings.loadShortcut(from: defaults, key: key) }

    @Test func conflictingShortcutThatFailsToRegisterKeepsUndo() {
        let settings = make()
        settings.setShortcut(f5)
        settings.setUndoShortcut(f6)
        hotkey.failNext = true

        settings.setShortcut(f6)

        #expect(settings.shortcut == f5)
        #expect(settings.undoShortcut == f6)
        #expect(settings.shortcutError == "refused")
        #expect(stored(ShortcutSettings.shortcutKey) == f5)
        #expect(stored(ShortcutSettings.undoShortcutKey) == f6)
        // The previous pair is registered again.
        let last = hotkey.registrations.last
        #expect(last?.0 == f5)
        #expect(last?.1 == f6)
        #expect(hotkey.isRegistered)
    }

    @Test func conflictingShortcutThatRegistersClearsUndo() {
        let settings = make()
        settings.setShortcut(f5)
        settings.setUndoShortcut(f6)

        settings.setShortcut(f6)

        let last = hotkey.registrations.last
        #expect(last?.0 == f6)
        #expect(last?.1 == nil)
        #expect(settings.shortcut == f6)
        #expect(settings.undoShortcut == nil)
        #expect(settings.shortcutError == nil)
        #expect(stored(ShortcutSettings.shortcutKey) == f6)
        #expect(defaults.data(forKey: ShortcutSettings.undoShortcutKey) == nil)
    }

    @Test func emptyShortcutIsRejected() {
        let settings = make()
        settings.setShortcut(f5)
        settings.setShortcut(Shortcut(keys: []))
        #expect(settings.shortcutError == "Press at least one key.")
        #expect(settings.shortcut == f5)
        #expect(stored(ShortcutSettings.shortcutKey) == f5)
    }

    @Test func undoEqualToDictationIsRejected() {
        let settings = make()
        settings.setShortcut(f5)
        settings.setUndoShortcut(f5)
        #expect(settings.undoShortcutError == "That's the dictation shortcut.")
        #expect(settings.undoShortcut == nil)
    }

    @Test func suspendUnregisters() {
        let settings = make()
        settings.start()
        settings.suspendHotkey()
        #expect(hotkey.unregisterCount == 1)
        #expect(!hotkey.isRegistered)
    }

    @Test func retryOnlyWhenStartedUnregisteredAndNotSuspended() {
        let settings = make()
        settings.retryHotkeyIfNeeded()
        #expect(hotkey.registrations.isEmpty) // not started

        hotkey.failNext = true
        settings.start()
        #expect(!hotkey.isRegistered)
        settings.retryHotkeyIfNeeded()
        #expect(hotkey.isRegistered)
        #expect(hotkey.registrations.count == 2)

        settings.retryHotkeyIfNeeded()
        #expect(hotkey.registrations.count == 2) // already registered

        settings.suspendHotkey()
        settings.retryHotkeyIfNeeded()
        #expect(hotkey.registrations.count == 2) // suspended
    }

    @Test func shortcutsRoundTripAndCorruptDataFallsBack() {
        let settings = make()
        settings.setShortcut(f7)
        settings.setUndoShortcut(f6)

        let reloaded = make()
        #expect(reloaded.shortcut == f7)
        #expect(reloaded.undoShortcut == f6)

        defaults.set(Data("not json".utf8), forKey: ShortcutSettings.shortcutKey)
        defaults.set(Data("{}".utf8), forKey: ShortcutSettings.undoShortcutKey)
        let corrupt = make()
        #expect(corrupt.shortcut == .default)
        #expect(corrupt.undoShortcut == nil)
    }

    @Test func hotkeyModeFollowsTheHotkey() {
        let settings = make()
        #expect(settings.hotkeyMode == nil)
        hotkey.modeWhenRegistered = .listenOnly
        settings.start()
        #expect(settings.hotkeyMode == .listenOnly)
        hotkey.modeWhenRegistered = .withholdsKeys
        settings.setShortcut(f5)
        #expect(settings.hotkeyMode == .withholdsKeys)
    }
}
