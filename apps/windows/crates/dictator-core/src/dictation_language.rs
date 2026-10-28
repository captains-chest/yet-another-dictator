//! Port of `DictationLanguage.swift`.

/// The language Whisper should transcribe. Automatic detection is unreliable for short clips, so a
/// fixed language can be chosen. Dictator is English-only: Danish is not offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DictationLanguage {
    #[default]
    Automatic,
    English,
}

impl DictationLanguage {
    pub const ALL: [Self; 2] = [Self::Automatic, Self::English];

    /// The stored value (Swift raw value).
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::English => "english",
        }
    }

    /// Whisper's language code, or `None` to detect it from the audio.
    pub fn whisper_code(self) -> Option<&'static str> {
        match self {
            Self::Automatic => None,
            Self::English => Some("en"),
        }
    }

    /// Shown in Settings.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::English => "English",
        }
    }

    /// Reads a stored value, falling back to automatic for anything unknown (including the removed
    /// `danish` case).
    pub fn from_stored(stored: Option<&str>) -> Self {
        Self::ALL
            .into_iter()
            .find(|l| Some(l.raw_value()) == stored)
            .unwrap_or_default()
    }
}
