//! Pure dictation logic shared by every Windows adapter. A port of captains-chest/DictationCore
//! 0.3.0 plus the macOS ModelSession; keep behaviour identical to the Swift originals.

pub mod app_version;
pub mod audio_ring_buffer;
pub mod capture_buffer;
pub mod chord;
pub mod dictation_controller;
pub mod dictation_history;
pub mod dictation_language;
pub mod dictation_prompt;
pub mod key_labels;
pub mod model_session;
pub mod ports;
pub mod shortcut;
pub mod smart_join;
pub mod speech_activity;
pub mod text;
pub mod text_chunker;
pub mod transcript_cleaner;
pub mod update_check;
pub mod whats_new;

pub use dictation_controller::SAMPLE_RATE_HZ;
pub use ports::*;
