//! Port of `KeyLabels.swift`, with a Windows virtual-key table instead of the macOS one.

use crate::chord::KeyCode;

/// Fixed labels for keys whose typed character is invisible, ambiguous or layout independent.
const SPECIAL: &[(u32, &str)] = &[
    (0x08, "Backspace"),
    (0x09, "Tab"),
    (0x0D, "Enter"),
    (0x13, "Pause"),
    (0x14, "Caps Lock"),
    (0x1B, "Esc"),
    (0x20, "Space"),
    (0x21, "Page Up"),
    (0x22, "Page Down"),
    (0x23, "End"),
    (0x24, "Home"),
    (0x25, "←"),
    (0x26, "↑"),
    (0x27, "→"),
    (0x28, "↓"),
    (0x2C, "Print Screen"),
    (0x2D, "Insert"),
    (0x2E, "Delete"),
    (0x5D, "Menu"),
    (0x6A, "Num *"),
    (0x6B, "Num +"),
    (0x6D, "Num -"),
    (0x6E, "Num ."),
    (0x6F, "Num /"),
    (0x90, "Num Lock"),
    (0x91, "Scroll Lock"),
];

const VK_0: u32 = 0x30;
const VK_9: u32 = 0x39;
const VK_A: u32 = 0x41;
const VK_Z: u32 = 0x5A;
const VK_NUMPAD0: u32 = 0x60;
const VK_NUMPAD9: u32 = 0x69;

/// Layout-independent label of a key, if it has one: modifiers, special keys, F-keys, digits,
/// letters and the numeric keypad.
pub fn fixed_label(code: KeyCode) -> Option<String> {
    if code.is_modifier() {
        return Some(symbol(code));
    }
    if let Some((_, label)) = SPECIAL.iter().find(|(vk, _)| *vk == code.0) {
        return Some((*label).to_string());
    }
    let vk = code.0;
    if code.is_function_key() {
        return Some(format!("F{}", vk - KeyCode::F1.0 + 1));
    }
    if (VK_0..=VK_9).contains(&vk) || (VK_A..=VK_Z).contains(&vk) {
        return char::from_u32(vk).map(String::from);
    }
    if (VK_NUMPAD0..=VK_NUMPAD9).contains(&vk) {
        return Some(format!("Num {}", vk - VK_NUMPAD0));
    }
    None
}

/// Prefers fixed labels, otherwise the typed character uppercased (punctuation keys depend on the
/// layout), otherwise "Key <code>".
pub fn label(code: KeyCode, typed: Option<&str>) -> String {
    if let Some(label) = fixed_label(code) {
        return label;
    }
    let trimmed = typed.unwrap_or_default().trim();
    if trimmed.is_empty() {
        format!("Key {}", code.0)
    } else {
        trimmed.to_uppercase()
    }
}

/// Side-less name, e.g. "Ctrl" for both Control keys.
pub fn symbol(code: KeyCode) -> String {
    match code.left_side() {
        KeyCode::LEFT_CONTROL => "Ctrl".into(),
        KeyCode::LEFT_ALT => "Alt".into(),
        KeyCode::LEFT_SHIFT => "Shift".into(),
        KeyCode::LEFT_WIN => "Win".into(),
        _ => format!("Key {}", code.0),
    }
}

/// Side-specific name, e.g. "Right Alt".
pub fn sided_name(code: KeyCode) -> String {
    let side = if code.left_side() == code {
        "Left "
    } else {
        "Right "
    };
    side.to_string() + &symbol(code)
}
