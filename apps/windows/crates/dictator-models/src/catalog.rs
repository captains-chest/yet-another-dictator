//! The speech models the Windows app can download, best first (mirrors the macOS `SpeechModel`).
//! Each id is a model-id in `model-manifest.txt`. Phonon-2 is not offered on Windows.

use dictator_core::dictation_language::DictationLanguage;

/// The optional transcript-cleanup model (ADR 0011); not a speech model.
pub const CLEANUP_MODEL_ID: &str = "s1-mini";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SpeechModel {
    /// Stays first: its accuracy is measured on the official Open ASR Leaderboard.
    #[default]
    ParakeetV2,
    WhisperLargeV3Turbo,
}

impl SpeechModel {
    /// Best first: the ranking shown in the picker.
    pub const RANKED: [Self; 2] = [Self::ParakeetV2, Self::WhisperLargeV3Turbo];

    pub fn id(self) -> &'static str {
        match self {
            Self::ParakeetV2 => "parakeet-tdt-0.6b-v2-int8",
            Self::WhisperLargeV3Turbo => "whisper-large-v3-turbo-q5_0",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::RANKED.into_iter().find(|model| model.id() == id)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::ParakeetV2 => "NVIDIA Parakeet TDT 0.6B v2",
            Self::WhisperLargeV3Turbo => "Whisper Large v3 Turbo",
        }
    }

    pub fn accuracy_note(self) -> &'static str {
        match self {
            Self::ParakeetV2 => {
                "Most accurate for English (6.1% average word error rate on the Open ASR Leaderboard)."
            }
            Self::WhisperLargeV3Turbo => "Accurate (7.8% average word error rate in English).",
        }
    }

    /// Whose model it is, for the picker.
    pub fn source(self) -> &'static str {
        match self {
            Self::ParakeetV2 => "NVIDIA, CC-BY-4.0",
            Self::WhisperLargeV3Turbo => "OpenAI, MIT",
        }
    }

    pub fn is_english_only(self) -> bool {
        self != Self::WhisperLargeV3Turbo
    }

    pub fn supported_languages(self) -> &'static [DictationLanguage] {
        if self.is_english_only() {
            &[DictationLanguage::English]
        } else {
            &DictationLanguage::ALL
        }
    }

    pub fn language_note(self) -> &'static str {
        if self.is_english_only() {
            "English only"
        } else {
            "Automatic or English"
        }
    }

    /// The files the engine needs in the model folder.
    pub fn required_files(self) -> &'static [&'static str] {
        match self {
            Self::ParakeetV2 => &[
                "encoder.int8.onnx",
                "decoder.int8.onnx",
                "joiner.int8.onnx",
                "tokens.txt",
            ],
            Self::WhisperLargeV3Turbo => &["ggml-large-v3-turbo-q5_0.bin"],
        }
    }
}
