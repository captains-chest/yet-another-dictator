import DictationCore
import Testing
@testable import Dictator

struct ShortcutRouterTests {
    static let control: UInt64 = 0x1, option: UInt64 = 0x20, rightOption: UInt64 = 0x40
    static let command: UInt64 = 0x8, rightCommand: UInt64 = 0x10

    func shortcut(_ codes: UInt16...) -> Shortcut {
        Shortcut(keys: codes.map { Shortcut.Key(code: $0, label: "") })
    }

    func defaultRouter(undo: Shortcut? = nil) -> ShortcutRouter {
        ShortcutRouter(
            shortcut: shortcut(KeyCode.leftControl, KeyCode.leftOption, 0x02), undo: undo
        )
    }

    @Test func defaultChordPressAndReleaseAreWithheldWithoutOtherInput() {
        var router = defaultRouter()
        #expect(router.handle(.key(.flagsChanged(KeyCode.leftControl, flags: Self.control))) == ([], false))
        #expect(router.handle(.key(.flagsChanged(KeyCode.leftOption, flags: Self.control | Self.option))) == ([], false))
        #expect(router.handle(.key(.down(0x02))) == ([.dictation(.pressed)], true))
        #expect(router.handle(.key(.up(0x02))) == ([.dictation(.released)], true))
    }

    @Test func plainLetterIsOtherInputAndNotWithheld() {
        var router = defaultRouter()
        #expect(router.handle(.key(.down(0x00))) == ([.otherInput], false))
        #expect(router.handle(.key(.up(0x00))) == ([], false))
    }

    @Test func typingWhileModifierDictationHeldInterruptsAndCountsAsOtherInput() {
        var router = ShortcutRouter(shortcut: shortcut(KeyCode.rightOption), undo: nil)
        #expect(router.handle(.key(.flagsChanged(KeyCode.rightOption, flags: Self.rightOption))) == ([.dictation(.pressed)], false))
        // Right ⌥ held, then E: the dictation ends and E was typed where the cursor is.
        #expect(router.handle(.key(.down(0x0E))) == ([.dictation(.interrupted), .otherInput], false))
    }

    @Test func modifierOnlyUndoFiresOnCleanRelease() {
        var router = defaultRouter(undo: shortcut(KeyCode.rightCommand))
        #expect(router.handle(.key(.flagsChanged(KeyCode.rightCommand, flags: Self.rightCommand))) == ([], false))
        #expect(router.handle(.key(.flagsChanged(KeyCode.rightCommand, flags: 0))) == ([.undo], false))
    }

    @Test func typingWithModifierUndoHeldNeverFiresUndo() {
        var router = defaultRouter(undo: shortcut(KeyCode.rightCommand))
        #expect(router.handle(.key(.flagsChanged(KeyCode.rightCommand, flags: Self.rightCommand))) == ([], false))
        #expect(router.handle(.key(.down(0x08))) == ([.otherInput], false)) // Right ⌘C
        #expect(router.handle(.key(.flagsChanged(KeyCode.rightCommand, flags: 0))) == ([], false))
    }

    @Test func clickWhileModifierUndoHeldDropsIt() {
        var router = defaultRouter(undo: shortcut(KeyCode.rightCommand))
        _ = router.handle(.key(.flagsChanged(KeyCode.rightCommand, flags: Self.rightCommand)))
        #expect(router.handle(.click(ignorable: false)) == ([.otherInput], false))
        #expect(router.handle(.key(.flagsChanged(KeyCode.rightCommand, flags: 0))) == ([], false))
    }

    @Test func dictationChordDoesNotTriggerRightOptionUndo() {
        var router = defaultRouter(undo: shortcut(KeyCode.rightOption))
        var outputs: [ShortcutRouter.Output] = []
        outputs += router.handle(.key(.flagsChanged(KeyCode.leftOption, flags: Self.option))).outputs
        outputs += router.handle(.key(.flagsChanged(KeyCode.leftControl, flags: Self.option | Self.control))).outputs
        let d = router.handle(.key(.down(0x02)))
        outputs += d.outputs
        #expect(d == ([.dictation(.pressed)], true))
        #expect(!outputs.contains(.undo))
        #expect(!outputs.contains(.otherInput))
    }

    @Test func supersetUndoChordInterruptsDictationAndFiresUndoOnPress() {
        var router = ShortcutRouter(
            shortcut: shortcut(KeyCode.rightOption),
            undo: shortcut(KeyCode.rightOption, 0x06) // Right ⌥Z
        )
        #expect(router.handle(.key(.flagsChanged(KeyCode.rightOption, flags: Self.rightOption))) == ([.dictation(.pressed)], false))
        let z = router.handle(.key(.down(0x06)))
        #expect(z == ([.dictation(.interrupted), .undo], true))
        #expect(!z.outputs.contains(.otherInput))
    }

    @Test func ownMenuOpenSuppressesOtherInput() {
        var router = defaultRouter()
        router.ownMenuIsOpen = true
        #expect(router.handle(.key(.down(0x00))) == ([], false))
        router.ownMenuIsOpen = false
        #expect(router.handle(.key(.down(0x01))) == ([.otherInput], false))
    }

    @Test func clicksByIgnorability() {
        var router = defaultRouter()
        #expect(router.handle(.click(ignorable: true)) == ([], false))
        #expect(router.handle(.click(ignorable: false)) == ([.otherInput], false))
    }

    @Test func reenabledEndsAHeldDictation() {
        var router = ShortcutRouter(shortcut: shortcut(KeyCode.rightOption), undo: nil)
        _ = router.handle(.key(.flagsChanged(KeyCode.rightOption, flags: Self.rightOption)))
        #expect(router.handle(.reenabled) == ([.dictation(.released)], false))
        #expect(router.handle(.key(.flagsChanged(KeyCode.rightOption, flags: 0))) == ([], false))
    }

    @Test func reenabledWhileIdleDoesNothing() {
        var router = defaultRouter()
        #expect(router.handle(.reenabled) == ([], false))
    }
}
