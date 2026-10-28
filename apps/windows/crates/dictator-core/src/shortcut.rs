//! Port of `Shortcut.swift`.
//!
//! Changed for Windows: codes are virtual-key codes, the display follows Windows conventions
//! ("Ctrl+Alt+D", modifiers ordered Ctrl, Alt, Shift, Win) and the default is Ctrl+Alt+D, mirroring
//! ⌃⌥D. The Swift `Codable` conformance (and its pre-chord legacy format, which only existed on
//! macOS) is left to the app's settings layer; [`Shortcut::keys`] and [`Shortcut::new`] round-trip.

use crate::chord::{KeyCode, KeySet};
use crate::key_labels;

/// One key of a shortcut, with the label the user saw when recording it, so displaying it needs no
/// keyboard-layout lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub label: String,
}

impl Key {
    pub fn new(code: KeyCode, label: &str) -> Self {
        Self {
            code,
            label: label.to_string(),
        }
    }
}

/// What pressing the shortcut does in the focused app when its keys can't be withheld, so every
/// key also reaches the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassThroughEffect {
    /// Only modifiers, Caps Lock or F-keys: nothing visible happens.
    Harmless,
    /// Includes Ctrl or Win, so the app may run its own command for the combination.
    AppShortcut,
    /// Types a character or acts on the text (letters, Space, Enter, arrows, Backspace, Esc…).
    Typing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    Empty,
}

const MODIFIER_ORDER: [KeyCode; 8] = [
    KeyCode::LEFT_CONTROL,
    KeyCode::RIGHT_CONTROL,
    KeyCode::LEFT_ALT,
    KeyCode::RIGHT_ALT,
    KeyCode::LEFT_SHIFT,
    KeyCode::RIGHT_SHIFT,
    KeyCode::LEFT_WIN,
    KeyCode::RIGHT_WIN,
];

fn order(code: KeyCode) -> u64 {
    MODIFIER_ORDER
        .iter()
        .position(|&m| m == code)
        .map_or(MODIFIER_ORDER.len() as u64 + u64::from(code.0), |i| {
            i as u64
        })
}

/// A global keyboard shortcut: any combination of physical keys held together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    keys: Vec<Key>,
}

impl Default for Shortcut {
    /// Ctrl+Alt+D — "D for dictate".
    fn default() -> Self {
        Self::new(vec![
            Key::new(KeyCode::LEFT_CONTROL, "Ctrl"),
            Key::new(KeyCode::LEFT_ALT, "Alt"),
            Key::new(KeyCode(0x44), "D"),
        ])
    }
}

impl Shortcut {
    /// Drops repeated codes and sorts in display order: modifiers first, then other keys by code.
    pub fn new(keys: Vec<Key>) -> Self {
        let mut seen = KeySet::new();
        let mut keys: Vec<Key> = keys.into_iter().filter(|k| seen.insert(k.code)).collect();
        keys.sort_by_key(|k| order(k.code));
        Self { keys }
    }

    pub fn keys(&self) -> &[Key] {
        &self.keys
    }

    pub fn codes(&self) -> KeySet {
        self.keys.iter().map(|k| k.code).collect()
    }

    /// e.g. "Ctrl+Alt+D", "Win+A+S", "Right Alt", "Left Ctrl + Left Shift".
    pub fn display_string(&self) -> String {
        let (modifiers, others): (Vec<&Key>, Vec<&Key>) =
            self.keys.iter().partition(|k| k.code.is_modifier());
        if others.is_empty() {
            return modifiers
                .iter()
                .map(|k| key_labels::sided_name(k.code))
                .collect::<Vec<_>>()
                .join(" + ");
        }
        let mut parts: Vec<String> = Vec::new();
        for key in modifiers {
            let symbol = key_labels::symbol(key.code);
            if !parts.contains(&symbol) {
                parts.push(symbol);
            }
        }
        parts.extend(others.iter().map(|k| k.label.clone()));
        parts.join("+")
    }

    pub fn pass_through_effect(&self) -> PassThroughEffect {
        let acting = self.keys.iter().any(|k| {
            !k.code.is_modifier() && k.code != KeyCode::CAPS_LOCK && !k.code.is_function_key()
        });
        if !acting {
            return PassThroughEffect::Harmless;
        }
        let command_like = [KeyCode::LEFT_CONTROL, KeyCode::LEFT_WIN];
        if self
            .keys
            .iter()
            .any(|k| command_like.contains(&k.code.left_side()))
        {
            return PassThroughEffect::AppShortcut;
        }
        PassThroughEffect::Typing
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.keys.is_empty() {
            Err(ValidationError::Empty)
        } else {
            Ok(())
        }
    }
}
