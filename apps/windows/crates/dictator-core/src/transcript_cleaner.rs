//! Port of `TranscriptCleaner.swift`. The Swift regex
//! `(?i)\[(?:blank_audio|music|noise|silence|inaudible)\]|<\|[^|]*\|>` is matched by hand.

use crate::text::trim_horizontal;

/// Whisper emits these for silence/noise; typing them into a field is never wanted.
const ANNOTATIONS: [&str; 5] = ["blank_audio", "music", "noise", "silence", "inaudible"];

/// Normalises raw transcriber output into text that is safe to type into another app.
///
/// Output is a single line: newlines/tabs become spaces so dictation can never submit a form
/// or move focus, and control characters are dropped so we never synthesise e.g. ESC.
pub fn clean(raw: &str) -> String {
    let without_annotations = remove_annotations(raw);
    let mut mapped = String::with_capacity(without_annotations.len());
    for c in without_annotations.chars() {
        if c.is_whitespace() {
            mapped.push(' ');
        } else if !c.is_control() {
            mapped.push(c);
        }
    }
    let mut collapsed = String::with_capacity(mapped.len());
    for c in mapped.chars() {
        if !(c == ' ' && collapsed.ends_with(' ')) {
            collapsed.push(c);
        }
    }
    trim_horizontal(&collapsed).to_string()
}

/// Length of an annotation match at the start of `s`, if any.
fn annotation_at(s: &str) -> Option<usize> {
    if let Some(rest) = s.strip_prefix('[') {
        for word in ANNOTATIONS {
            if rest.len() > word.len()
                && rest.is_char_boundary(word.len())
                && rest[..word.len()].eq_ignore_ascii_case(word)
                && rest[word.len()..].starts_with(']')
            {
                return Some(word.len() + 2);
            }
        }
    }
    let rest = s.strip_prefix("<|")?;
    let bar = rest.find('|')?;
    rest[bar..].starts_with("|>").then_some(2 + bar + 2)
}

fn remove_annotations(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        if let Some(length) = annotation_at(&raw[index..]) {
            index += length;
            continue;
        }
        let c = raw[index..].chars().next().unwrap_or_default();
        result.push(c);
        index += c.len_utf8();
    }
    result
}
