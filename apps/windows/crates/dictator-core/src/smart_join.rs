//! Port of `SmartJoin.swift`. Works on `char`s where Swift uses `Character`s; the two only differ
//! for clusters (combining marks, emoji) that never sit in the punctuation sets below.

use crate::text::is_newline;

const TERMINATORS: &[char] = &['.', '!', '?', '…'];
const CLOSERS: &[char] = &['"', '\'', ')', ']', '}', '»', '”', '’'];
const OPENERS: &[char] = &['(', '[', '{', '«', '“', '‘', '/'];
const ATTACHES_TO_PREVIOUS: &[char] = &[
    '.', ',', ';', ':', '!', '?', '…', ')', ']', '}', '%', '»', '”', '’',
];

/// Fits a dictation to the text it is typed after: adds the separating space, and capitalizes
/// or lowercases the first word depending on whether a new sentence starts there.
///
/// `text` is the cleaned transcript; `preceding` the text just before the insertion point (only
/// its end matters), or `None` when unknown, which leaves the transcript as transcribed.
pub fn adjust(text: &str, preceding: Option<&str>) -> String {
    let (Some(preceding), Some(first)) = (preceding, text.chars().next()) else {
        return text.to_string();
    };
    if first.is_whitespace() {
        return text.to_string();
    }
    let mut result = if starts_sentence(preceding) {
        capitalizing_first_letter(text)
    } else {
        lowering_first_word(text).unwrap_or_else(|| text.to_string())
    };
    if needs_space(preceding, first) {
        result.insert(0, ' ');
    }
    result
}

/// True when the insertion point is at the start of a sentence: the field is empty, the line is
/// new, or the previous sentence ended (possibly followed by a closing quote or bracket).
pub fn starts_sentence(preceding: &str) -> bool {
    let tail = preceding.trim_end_matches(|c: char| c.is_whitespace() && !is_newline(c));
    let tail = tail.trim_end_matches(CLOSERS);
    match tail.chars().last() {
        None => true,
        Some(last) => is_newline(last) || TERMINATORS.contains(&last),
    }
}

fn needs_space(preceding: &str, next: char) -> bool {
    let mut reversed = preceding.chars().rev();
    let Some(previous) = reversed.next() else {
        return false;
    };
    if previous.is_whitespace() || OPENERS.contains(&previous) {
        return false;
    }
    // A straight quote opens when it follows a space or starts the text, and closes otherwise.
    if previous == '"' || previous == '\'' {
        match reversed.next() {
            None => return false,
            Some(before) if before.is_whitespace() || OPENERS.contains(&before) => return false,
            Some(_) => {}
        }
    }
    !ATTACHES_TO_PREVIOUS.contains(&next)
}

fn capitalizing_first_letter(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => first.to_uppercase().chain(chars).collect(),
        _ => text.to_string(),
    }
}

/// Lowercases a sentence-style capital in the middle of a sentence ("Hello, How" → "Hello, how").
/// Leaves words that look intentionally capitalized: "I", acronyms ("NASA", "TV"), and words with
/// capitals inside ("iPhone", "McDonald"). Proper nouns can't be told apart and get lowercased.
fn lowering_first_word(text: &str) -> Option<String> {
    let word: Vec<char> = text.chars().take_while(|c| c.is_alphabetic()).collect();
    let (&first, rest) = word.split_first()?;
    if word.len() < 2 || !first.is_uppercase() || !rest.iter().all(|c| c.is_lowercase()) {
        return None;
    }
    Some(first.to_lowercase().chain(text.chars().skip(1)).collect())
}
