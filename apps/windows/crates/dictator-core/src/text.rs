//! Swift's `Character` is a grapheme cluster and Foundation's character sets differ from Rust's
//! `char` helpers; these bridge the gap without third-party crates.
//!
//! The grapheme segmentation is a simplified UAX #29: it keeps CR LF, combining marks, variation
//! selectors, emoji modifiers, tags, ZWJ sequences and regional-indicator pairs together, which
//! covers what dictation produces. It does not implement Hangul syllable or Indic conjunct rules.

/// Foundation's `CharacterSet.whitespaces`: horizontal whitespace (Zs plus tab), no newlines.
pub fn is_horizontal_whitespace(c: char) -> bool {
    c.is_whitespace() && !is_newline(c)
}

/// Swift's `Character.isNewline`.
pub fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\u{B}' | '\u{C}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// Trims Foundation's `.whitespaces` from both ends.
pub fn trim_horizontal(s: &str) -> &str {
    s.trim_matches(is_horizontal_whitespace)
}

const ZWJ: char = '\u{200D}';

const EXTEND_RANGES: &[(u32, u32)] = &[
    (0x0300, 0x036F),
    (0x0483, 0x0489),
    (0x0591, 0x05BD),
    (0x05BF, 0x05BF),
    (0x05C1, 0x05C2),
    (0x05C4, 0x05C5),
    (0x05C7, 0x05C7),
    (0x0610, 0x061A),
    (0x064B, 0x065F),
    (0x0670, 0x0670),
    (0x06D6, 0x06DC),
    (0x06DF, 0x06E4),
    (0x06E7, 0x06E8),
    (0x06EA, 0x06ED),
    (0x0900, 0x0903),
    (0x093A, 0x094F),
    (0x0951, 0x0957),
    (0x0962, 0x0963),
    (0x1AB0, 0x1AFF),
    (0x1DC0, 0x1DFF),
    (0x200C, 0x200C),
    (0x20D0, 0x20FF),
    (0x3099, 0x309A),
    (0xFE00, 0xFE0F),
    (0xFE20, 0xFE2F),
    (0x1F3FB, 0x1F3FF),
    (0xE0020, 0xE007F),
    (0xE0100, 0xE01EF),
];

fn is_extend(c: char) -> bool {
    let code = c as u32;
    c == ZWJ
        || EXTEND_RANGES
            .iter()
            .any(|&(low, high)| (low..=high).contains(&code))
}

fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// Splits `s` into (approximate) extended grapheme clusters.
pub fn graphemes(s: &str) -> Vec<&str> {
    let mut clusters = Vec::new();
    let mut start = 0;
    let mut previous: Option<char> = None;
    let mut regional_run = 0;
    for (index, c) in s.char_indices() {
        let joins = match previous {
            None => true,
            Some('\r') => c == '\n',
            Some(p) if is_regional_indicator(p) && is_regional_indicator(c) => {
                regional_run % 2 == 1
            }
            Some(p) => !is_newline(p) && !p.is_control() && (is_extend(c) || p == ZWJ),
        };
        if !joins {
            clusters.push(&s[start..index]);
            start = index;
        }
        regional_run = if is_regional_indicator(c) {
            regional_run + 1
        } else {
            0
        };
        previous = Some(c);
    }
    if start < s.len() {
        clusters.push(&s[start..]);
    }
    clusters
}

/// Swift's `String.count`: characters as the user sees them.
pub fn grapheme_count(s: &str) -> usize {
    graphemes(s).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_emoji_sequences_and_accents_whole() {
        assert_eq!(grapheme_count("👋🏽"), 1);
        assert_eq!(grapheme_count("👩‍👩‍👧‍👦"), 1);
        assert_eq!(grapheme_count("e\u{301}"), 1);
        assert_eq!(grapheme_count("\r\n"), 1);
        assert_eq!(grapheme_count("🇩🇰🇬🇧"), 2);
        assert_eq!(grapheme_count(" Hej 👋🏽 café"), 11);
    }

    #[test]
    fn horizontal_whitespace_excludes_newlines() {
        assert!(is_horizontal_whitespace('\t'));
        assert!(is_horizontal_whitespace('\u{A0}'));
        assert!(!is_horizontal_whitespace('\n'));
        assert_eq!(trim_horizontal("\t a b \u{3000}"), "a b");
    }
}
