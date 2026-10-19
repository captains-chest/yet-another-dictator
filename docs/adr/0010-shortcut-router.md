# 10. One ShortcutRouter owns what each tap event means

- Status: Accepted
- Date: 2026-10-19

## Context

The CGEvent tap in `EventTapHotkey` decided everything inline: it ran two `ChordMatcher`s,
fired undo the moment its chord matched, and reported "other input" only for a key-down that
produced no actions at all. Three bugs followed:

1. Right ⌥ dictation, then typing ⌥E (or any letter): the matcher reported `.interrupted`, so
   the key-down produced an action and was not counted as other input. The controller therefore
   never learnt the cursor may have moved, and a later Undo could delete text it did not dictate.
2. A modifier-only undo shortcut (e.g. Right ⌘) fired on *press*. Holding Right ⌘ and pressing C
   to copy both copied and undid the last dictation.
3. When macOS disabled the tap (timeout), the matchers were rebuilt inline in the adapter, so a
   hold-to-talk dictation could stay "recording" with its release never seen.

The decision logic was untested: it sat inside a CoreGraphics callback on a private thread type.

## Decision

Design A: one pure `ShortcutRouter` (`Sources/Dictator/System/ShortcutRouter.swift`,
`Sendable`, DictationCore only) owns what each event means.

- Inputs are `.key(KeyEvent)`, `.click(ignorable:)` and `.reenabled`; outputs are
  `.dictation(ChordMatcher.Action)`, `.undo` and `.otherInput`, plus a `withhold` flag.
  `EventTapHotkey`'s `TapContext` keeps only the adapter work: CG-to-input mapping, the tap
  lifecycle and thread, the lock, the own-PID filter, and menu-bar geometry. It holds the router
  and the tap under its lock; `AppModel`'s menu observers now call
  `EventTapHotkey.setOwnMenuOpen(_:)`, which forwards into the router under the same lock.
- `otherInput` is reported for a key-down that is not withheld, presses no chord and fires no
  undo, while our own menu is closed — including a key that interrupts or releases a chord, so
  bug 1 cannot recur.
- A modifier-only undo chord fires only on a clean `.released`: an interrupt or a non-ignorable
  click while held drops the pending undo. A non-modifier undo chord still fires on press.
- `.reenabled` rebuilds both matchers and returns `.dictation(.released)` iff the dictation
  chord was active, ending a hold-to-talk cleanly after a tap timeout (bug 3).

### Rejected: B. Keep the decisions in TapContext, add callbacks

Adding `onOtherInput`-style callbacks for the new cases would leave the policy in the CG
adapter, still untestable without a tap, and each new rule would need another callback.
Splitting into per-chord modules (`DictationChord`, `UndoChord`) was also rejected: the rules
that matter are exactly the interactions between the chords (superset chords, interrupts,
shared other-input), which a split would scatter.

### Deletion test

Delete `ShortcutRouter` and the complexity reappears inside `TapContext.handle`: two matchers,
pending-undo state, active-chord tracking and the menu gate, all back in the CoreGraphics
callback. The router earns its keep.

## Consequences

- All routing rules are unit-tested offline with raw `KeyEvent`s; no behaviour depends on a
  live tap in tests.
- Modifier-only undo now fires on release, a beat later than before; a hold-to-talk dictation
  now ends when the tap times out instead of sticking.
- Undo after typing with a modifier held no longer deletes text the user typed themselves.
