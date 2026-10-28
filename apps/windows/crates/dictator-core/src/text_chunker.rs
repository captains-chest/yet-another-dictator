//! Port of `TextChunker.swift`.

use crate::text::graphemes;

/// The macOS event payload limit (20 UTF-16 code units), kept so both apps type in the same steps.
pub const MAX_UTF16_PER_EVENT: usize = 20;

/// Splits text into chunks small enough for one synthesized key event, never splitting a
/// grapheme cluster (so emoji and combined accents survive).
pub fn chunks(text: &str, max_utf16: usize) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut current_count = 0;
    for character in graphemes(text) {
        let count = character.encode_utf16().count();
        if current_count + count > max_utf16 && !current.is_empty() {
            result.push(std::mem::take(&mut current));
            current_count = 0;
        }
        current.push_str(character);
        current_count += count;
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}
