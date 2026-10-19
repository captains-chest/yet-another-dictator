import DictationCore

/// What each event from the tap means (ADR 0010). Pure: the adapter in `EventTapHotkey` maps
/// CGEvents to inputs, delivers the outputs, and keeps the tap itself. All chord policy — when
/// undo fires, when a key counts as "the cursor may have moved" — lives here, tested offline.
struct ShortcutRouter: Sendable {
    enum Input: Equatable, Sendable {
        case key(KeyEvent)
        /// A mouse click. `ignorable` clicks (our own PID, the menu bar) don't move the text
        /// cursor; the adapter works that out from the event.
        case click(ignorable: Bool)
        /// The tap was disabled (timeout or by the user) and re-enabled; key state is unknown.
        case reenabled
    }

    enum Output: Equatable, Sendable {
        case dictation(ChordMatcher.Action)
        case undo
        case otherInput
    }

    /// Set while our own menu is tracking, so its key equivalents don't count as typing.
    var ownMenuIsOpen = false

    private let shortcut: Shortcut
    private let undoShortcut: Shortcut?
    private var dictation: ChordMatcher
    private var undoMatcher: ChordMatcher?
    private var dictationActive = false
    /// A modifier-only undo chord is down; it fires only on a clean release.
    private var undoPending = false

    init(shortcut: Shortcut, undo: Shortcut?) {
        self.shortcut = shortcut
        undoShortcut = undo
        dictation = ChordMatcher(shortcut)
        undoMatcher = undo.map(ChordMatcher.init)
    }

    private var undoIsModifierOnly: Bool {
        undoShortcut.map { $0.keys.allSatisfy { KeyCode.isModifier($0.code) } } ?? false
    }

    /// Returns the outputs for one input, and whether the event is withheld from other apps.
    mutating func handle(_ input: Input) -> (outputs: [Output], withhold: Bool) {
        switch input {
        case .click(let ignorable):
            if ignorable { return ([], false) }
            // A real click while a modifier-only undo is held means it wasn't an undo gesture.
            undoPending = false
            return ([.otherInput], false)

        case .reenabled:
            let wasActive = dictationActive
            dictation = ChordMatcher(shortcut)
            undoMatcher = undoShortcut.map(ChordMatcher.init)
            dictationActive = false
            undoPending = false
            return (wasActive ? [.dictation(.released)] : [], false)

        case .key(let event):
            let (dictationActions, dictationSwallow) = dictation.handle(event)
            var outputs: [Output] = []
            var pressedSomething = false
            for action in dictationActions {
                outputs.append(.dictation(action))
                switch action {
                case .pressed: dictationActive = true; pressedSomething = true
                case .released, .interrupted: dictationActive = false
                }
            }

            var undoSwallow = false
            if var matcher = undoMatcher {
                let (actions, swallow) = matcher.handle(event)
                undoMatcher = matcher
                undoSwallow = swallow
                for action in actions {
                    if undoIsModifierOnly {
                        switch action {
                        case .pressed: undoPending = true
                        case .released:
                            if undoPending { outputs.append(.undo) }
                            undoPending = false
                        case .interrupted: undoPending = false
                        }
                    } else if action == .pressed {
                        outputs.append(.undo)
                        pressedSomething = true
                    }
                }
            }

            let withhold = dictationSwallow || undoSwallow
            // A key-down that presses no chord, fires no undo and isn't withheld is typing: the
            // cursor may have moved. That includes a key that interrupts or releases a chord
            // (Right ⌥ held, then E: the dictation is interrupted *and* E was typed). Modifier
            // changes arrive as flagsChanged, never as key-down, so they can't reach this.
            if case .down = event, !withhold, !pressedSomething, !outputs.contains(.undo), !ownMenuIsOpen {
                outputs.append(.otherInput)
            }
            return (outputs, withhold)
        }
    }
}
