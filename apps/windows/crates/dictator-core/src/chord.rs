//! Port of `Chord.swift` (key tracking, chord matching and recording).
//!
//! Changed for Windows: key identifiers are Windows virtual-key codes in a [`KeyCode`] newtype
//! instead of macOS key codes, so the macOS tables (flag bits, fn key) are gone. Modifiers must be
//! reported with their sided codes (`VK_LCONTROL`, not `VK_CONTROL`), which is what a low-level
//! keyboard hook delivers. Modifiers may arrive either as plain `Down`/`Up` events or as
//! [`KeyEvent::FlagsChanged`] carrying the full set of held modifiers (e.g. from
//! `GetAsyncKeyState`), which keeps the macOS recovery from missed releases. Caps Lock is a
//! normal key on Windows (it reports its release), so the macOS "momentary tap" special case is
//! dropped.

use std::collections::BTreeSet;

use crate::shortcut::Shortcut;

/// A Windows virtual-key code (layout independent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyCode(pub u32);

impl KeyCode {
    pub const LEFT_SHIFT: Self = Self(0xA0);
    pub const RIGHT_SHIFT: Self = Self(0xA1);
    pub const LEFT_CONTROL: Self = Self(0xA2);
    pub const RIGHT_CONTROL: Self = Self(0xA3);
    /// Alt is `VK_LMENU`/`VK_RMENU`. Right Alt is AltGr on many layouts.
    pub const LEFT_ALT: Self = Self(0xA4);
    pub const RIGHT_ALT: Self = Self(0xA5);
    pub const LEFT_WIN: Self = Self(0x5B);
    pub const RIGHT_WIN: Self = Self(0x5C);
    pub const CAPS_LOCK: Self = Self(0x14);
    pub const F1: Self = Self(0x70);
    pub const F24: Self = Self(0x87);

    const MODIFIERS: [Self; 8] = [
        Self::LEFT_CONTROL,
        Self::RIGHT_CONTROL,
        Self::LEFT_ALT,
        Self::RIGHT_ALT,
        Self::LEFT_SHIFT,
        Self::RIGHT_SHIFT,
        Self::LEFT_WIN,
        Self::RIGHT_WIN,
    ];

    /// Modifier keys (everything except Caps Lock).
    pub fn is_modifier(self) -> bool {
        Self::MODIFIERS.contains(&self)
    }

    /// F1–F24. Bare function keys rarely do anything in the focused app.
    pub fn is_function_key(self) -> bool {
        (Self::F1.0..=Self::F24.0).contains(&self.0)
    }

    /// Maps right-hand modifiers onto their left-hand twin; other keys are unchanged.
    pub fn left_side(self) -> Self {
        match self {
            Self::RIGHT_CONTROL => Self::LEFT_CONTROL,
            Self::RIGHT_ALT => Self::LEFT_ALT,
            Self::RIGHT_SHIFT => Self::LEFT_SHIFT,
            Self::RIGHT_WIN => Self::LEFT_WIN,
            other => other,
        }
    }
}

pub type KeySet = BTreeSet<KeyCode>;

/// A raw keyboard event, independent of the Win32 API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyEvent {
    Down(KeyCode),
    Up(KeyCode),
    /// A modifier key changed; the set is every modifier held after the change.
    FlagsChanged(KeyCode, KeySet),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Down,
    Up,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub kind: ChangeKind,
    pub code: KeyCode,
    pub held: KeySet,
}

/// Tracks which physical keys are held and reports each individual change with the resulting set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyTracker {
    held: KeySet,
}

impl KeyTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn held(&self) -> &KeySet {
        &self.held
    }

    /// Key repeats produce no changes.
    pub fn apply(&mut self, event: &KeyEvent) -> Vec<Change> {
        let change = |kind, code, held: &KeySet| Change {
            kind,
            code,
            held: held.clone(),
        };
        match *event {
            KeyEvent::Down(code) => {
                if self.held.insert(code) {
                    vec![change(ChangeKind::Down, code, &self.held)]
                } else {
                    vec![]
                }
            }
            KeyEvent::Up(code) => {
                if self.held.remove(&code) {
                    vec![change(ChangeKind::Up, code, &self.held)]
                } else {
                    vec![]
                }
            }
            KeyEvent::FlagsChanged(_, ref modifiers) => {
                // Flags are authoritative for modifiers, so a missed event can't leave one stuck.
                let now: KeySet = modifiers
                    .iter()
                    .copied()
                    .filter(|c| c.is_modifier())
                    .collect();
                let before: KeySet = self
                    .held
                    .iter()
                    .copied()
                    .filter(|c| c.is_modifier())
                    .collect();
                let mut changes = Vec::new();
                for released in before.difference(&now) {
                    self.held.remove(released);
                    changes.push(change(ChangeKind::Up, *released, &self.held));
                }
                for pressed in now.difference(&before) {
                    self.held.insert(*pressed);
                    changes.push(change(ChangeKind::Down, *pressed, &self.held));
                }
                changes
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Pressed,
    Released,
    /// A modifier-only chord was pressed, then another key joined it (e.g. Ctrl then C):
    /// the user was typing a different shortcut, not dictating.
    Interrupted,
}

/// Detects when a shortcut chord is pressed and released.
///
/// The chord matches when exactly its keys are held. If it contains a non-modifier key, left and
/// right modifiers are interchangeable (Ctrl+Alt+D works with either Alt); modifier-only chords are
/// side-specific so e.g. Right Alt alone can be used without hijacking Left Alt.
#[derive(Debug, Clone)]
pub struct ChordMatcher {
    target: KeySet,
    side_agnostic: bool,
    modifier_only: bool,
    tracker: KeyTracker,
    active: bool,
    swallowing: KeySet,
}

impl ChordMatcher {
    pub fn new(shortcut: &Shortcut) -> Self {
        let codes = shortcut.codes();
        let modifier_only = codes.iter().all(|c| c.is_modifier());
        let side_agnostic = !modifier_only;
        let target = if side_agnostic {
            codes.iter().map(|c| c.left_side()).collect()
        } else {
            codes
        };
        Self {
            target,
            side_agnostic,
            modifier_only,
            tracker: KeyTracker::new(),
            active: false,
            swallowing: KeySet::new(),
        }
    }

    /// Returns the resulting actions and whether the event should be withheld from other apps.
    /// Only key down/up events of the chord's non-modifier keys are ever withheld.
    pub fn handle(&mut self, event: &KeyEvent) -> (Vec<Action>, bool) {
        let mut actions = Vec::new();
        for change in self.tracker.apply(event) {
            let held: KeySet = if self.side_agnostic {
                change.held.iter().map(|c| c.left_side()).collect()
            } else {
                change.held
            };
            let matches = held == self.target;
            if !self.active && matches && change.kind == ChangeKind::Down {
                self.active = true;
                actions.push(Action::Pressed);
            } else if self.active && !matches {
                self.active = false;
                let interrupted = self.modifier_only && change.kind == ChangeKind::Down;
                actions.push(if interrupted {
                    Action::Interrupted
                } else {
                    Action::Released
                });
            }
        }
        let swallow = self.swallow(event);
        (actions, swallow)
    }

    fn swallow(&mut self, event: &KeyEvent) -> bool {
        match *event {
            KeyEvent::Down(code)
                if self.active && self.target.contains(&code) && !code.is_modifier() =>
            {
                self.swallowing.insert(code);
                true
            }
            KeyEvent::Up(code) => self.swallowing.remove(&code),
            _ => false,
        }
    }
}

/// Records a new chord: every key held during one gesture, committed once all keys are released.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChordRecorder {
    tracker: KeyTracker,
    pressed: KeySet,
}

impl ChordRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keys currently held down.
    pub fn held(&self) -> &KeySet {
        self.tracker.held()
    }

    /// Returns the recorded chord once every key has been released.
    pub fn apply(&mut self, event: &KeyEvent) -> Option<KeySet> {
        for change in self.tracker.apply(event) {
            if change.kind == ChangeKind::Down {
                self.pressed.insert(change.code);
            }
            if change.held.is_empty() && !self.pressed.is_empty() {
                return Some(std::mem::take(&mut self.pressed));
            }
        }
        None
    }
}
