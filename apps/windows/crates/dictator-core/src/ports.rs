//! The ports of DictationCore (captains-chest/DictationCore 0.3.0, `DictationController.swift`)
//! and of the macOS app's `ModelSession` and `TranscriptCleanup` targets, in Rust. Keep names and
//! semantics aligned with the Swift originals.
//!
//! Swift's task cancellation becomes a [`StopSignal`] that blocking engines poll. Engines run on
//! worker threads, never on the UI thread.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A user-facing failure message. Engines and adapters return it; the controller shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortError {
    /// The caller gave up (Cancel Dictation, timeout). Never shown to the user.
    Cancelled,
    /// Anything else, with a message fit to show.
    Failed(String),
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("Cancelled"),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PortError {}

pub type PortResult<T> = Result<T, PortError>;

/// Set when the caller gives up on a transcription, cleanup or typing; long work checks it.
#[derive(Debug, Clone, Default)]
pub struct StopSignal(Arc<AtomicBool>);

impl StopSignal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// `Err(Cancelled)` once set, for `?` between steps.
    pub fn check(&self) -> PortResult<()> {
        if self.is_set() {
            Err(PortError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Captures microphone audio as 16 kHz mono f32 samples.
pub trait AudioRecording {
    /// `on_level` receives a 0...1 loudness value from the audio thread.
    fn start(&mut self, on_level: Box<dyn Fn(f32) + Send + Sync>) -> PortResult<()>;
    fn stop(&mut self) -> Vec<f32>;
}

/// Turns 16 kHz mono samples into text. Must work fully offline. Blocking; call off the UI thread.
pub trait Transcribing: Send + Sync {
    fn prepare(&self) -> PortResult<()>;
    /// `language` is a Whisper language code ("da", "en"), or `None` to detect it from the audio.
    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
        stop: &StopSignal,
    ) -> PortResult<String>;
}

/// Delivers text into whatever field currently has keyboard focus.
pub trait TextInserting {
    /// A user-facing error if `insert` is known to fail. Checked before recording.
    fn preflight(&mut self) -> PortResult<()>;
    /// Types `text` in chunks; stops between chunks once `stop` is set (some text may be typed).
    fn insert(&mut self, text: &str, stop: &StopSignal) -> PortResult<()>;
    /// Deletes `count` characters before the cursor, like pressing Backspace `count` times.
    fn delete_backward(&mut self, count: usize, stop: &StopSignal) -> PortResult<()>;
}

/// Reads the text just before the insertion point in the focused field. Best effort: `None` when
/// it can't be read. The text is never stored or logged.
pub trait PrecedingTextReading {
    /// Empty string means cursor at start / empty field.
    fn text_before_cursor(&mut self, max_length: usize) -> Option<String>;

    /// When the field can't be read, fall back to the last dictation for SmartJoin.
    fn uses_history_for_joining(&self) -> bool {
        true
    }
}

/// A speech model once loaded; the only thing an engine adapter implements (ADR 0008).
pub trait LoadedSpeechModel: Send + Sync {
    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
        stop: &StopSignal,
    ) -> PortResult<String>;
}

/// Rewrites a raw transcript into the text the user meant (ADR 0011). Must work fully offline.
pub trait TranscriptCleaning: Send + Sync {
    fn prepare(&self) -> PortResult<()>;
    /// An empty result is valid (the transcript held only "um").
    fn clean(&self, transcript: &str, style: CleanupStyle, stop: &StopSignal)
    -> PortResult<String>;
}

/// The settings S1-mini was trained on. `name()` gives the exact model-card spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CleanupStyle {
    pub styling: Styling,
    pub structure: Structure,
    pub context: Context,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Styling {
    Casual,
    SemiCasual,
    #[default]
    SemiFormal,
    Formal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Structure {
    #[default]
    Prose,
    Lists,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Context {
    #[default]
    General,
    Email,
}

impl Styling {
    pub const ALL: [Self; 4] = [
        Self::Casual,
        Self::SemiCasual,
        Self::SemiFormal,
        Self::Formal,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Casual => "casual",
            Self::SemiCasual => "semi-casual",
            Self::SemiFormal => "semi-formal",
            Self::Formal => "formal",
        }
    }
}

impl Structure {
    pub const ALL: [Self; 2] = [Self::Prose, Self::Lists];

    pub fn name(self) -> &'static str {
        match self {
            Self::Prose => "prose",
            Self::Lists => "lists",
        }
    }
}

impl Context {
    pub const ALL: [Self; 2] = [Self::General, Self::Email];

    pub fn name(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Email => "email",
        }
    }
}
