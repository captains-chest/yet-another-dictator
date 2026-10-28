//! Port of `DictationHistory` (in `SmartJoin.swift`).

use crate::text::grapheme_count;

/// What the last dictation typed, as long as the user hasn't typed, clicked or switched apps since.
/// Used as the text before the cursor when the focused field can't be read, and for undo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DictationHistory {
    last_inserted: Option<String>,
}

impl DictationHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn last_inserted(&self) -> Option<&str> {
        self.last_inserted.as_deref()
    }

    pub fn did_insert(&mut self, text: &str) {
        self.last_inserted = Some(text.to_string());
    }

    /// The user typed, clicked or switched apps: the cursor may be anywhere now.
    pub fn other_input(&mut self) {
        self.last_inserted = None;
    }

    /// The number of characters (as the user sees them) to delete to undo the last dictation,
    /// or `None` when it can't be undone safely. Clears the history.
    pub fn take_undo(&mut self) -> Option<usize> {
        self.last_inserted
            .take()
            .filter(|t| !t.is_empty())
            .map(|t| grapheme_count(&t))
    }
}
