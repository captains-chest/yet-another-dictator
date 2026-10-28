//! OpenAI Whisper Large v3 Turbo (whisper.cpp ggml Q5_0) behind `LoadedSpeechModel`. The only code
//! that uses whisper.cpp. Mirrors the macOS `LoadedWhisperKit`: the punctuation prompt, splitting
//! long audio at pauses, Whisper's quality checks and dropping repetitive or echoed output.

use std::ffi::{c_int, c_void};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use dictator_core::{
    LoadedSpeechModel, PortError, PortResult, StopSignal, dictation_prompt, speech_activity,
};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use crate::{SAMPLE_RATE, long_audio_splitter, require_files, utf8_path};

pub const MODEL_ID: &str = "whisper-large-v3-turbo-q5_0";
pub const MODEL_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";
pub const REQUIRED_FILES: &[&str] = &[MODEL_FILE];

/// Output that compresses better than this is too repetitive to be real speech. Same value as
/// WhisperKit's and OpenAI's default.
pub const COMPRESSION_RATIO_THRESHOLD: f32 = 2.4;
/// Whisper's standard quality checks, as WhisperKit uses them on macOS. A window that fails one is
/// decoded again at a temperature higher by `TEMPERATURE_INCREMENT`.
const LOG_PROB_THRESHOLD: f32 = -1.0;
const NO_SPEECH_THRESHOLD: f32 = 0.6;
const TEMPERATURE_INCREMENT: f32 = 0.2;
/// Candidates per fallback temperature; greedy (one) at temperature 0. WhisperKit's default.
const BEST_OF: c_int = 5;
/// Room for the prompt tokens; the prompt is far shorter.
const MAX_PROMPT_TOKENS: usize = 128;
/// whisper.cpp's value for "detect the language".
const AUTO_LANGUAGE: &str = "auto";

/// A loaded whisper.cpp model with one decoding state. `ModelSession` serialises transcriptions,
/// so the lock is never contended.
pub struct WhisperCppModel {
    // Declared before `context` so it is dropped first.
    state: Mutex<WhisperState>,
    /// `dictation_prompt::TEXT` tokenised once at load.
    prompt_tokens: Vec<c_int>,
    _context: WhisperContext,
}

impl WhisperCppModel {
    pub fn load(folder: &Path) -> PortResult<Self> {
        require_files(folder, REQUIRED_FILES)?;
        // Without a log backend this keeps whisper.cpp and ggml quiet; it never sees our logs.
        whisper_rs::install_logging_hooks();
        let path = folder.join(MODEL_FILE);
        let context =
            WhisperContext::new_with_params(utf8_path(&path)?, WhisperContextParameters::default())
                .map_err(|_| PortError::Failed("The Whisper model could not be loaded.".into()))?;
        let eot = context.token_eot();
        // Leading space: the prompt reads as preceding text, as in Whisper's own prompting.
        let prompt_tokens = context
            .tokenize(&format!(" {}", dictation_prompt::TEXT), MAX_PROMPT_TOKENS)
            .map_err(|_| PortError::Failed("The Whisper prompt could not be prepared.".into()))?
            .into_iter()
            .filter(|&token| token < eot)
            .collect();
        let state = context
            .create_state()
            .map_err(|_| PortError::Failed("The Whisper model could not start.".into()))?;
        let model = Self {
            state: Mutex::new(state),
            prompt_tokens,
            _context: context,
        };
        // Pay the first decode's one-off setup during "Loading model…".
        let _ = model.transcribe_piece(&[0.0; SAMPLE_RATE], Some("en"), &StopSignal::new());
        Ok(model)
    }

    /// Transcribes audio of at most one 30 s window. Returns the text and the detected language.
    fn transcribe_piece(
        &self,
        samples: &[f32],
        language: Option<&str>,
        stop: &StopSignal,
    ) -> PortResult<(String, Option<String>)> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: BEST_OF });
        params.set_translate(false);
        params.set_language(Some(language.unwrap_or(AUTO_LANGUAGE)));
        params.set_no_timestamps(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        // A punctuated prompt makes Whisper write complete sentences, including the final full
        // stop it otherwise often drops on short dictations. See `dictation_prompt`.
        params.set_tokens(&self.prompt_tokens);
        // Whisper's standard quality checks, spelled out so they're deliberate. whisper.cpp's
        // entropy threshold is its stand-in for the compression-ratio check during decoding.
        params.set_temperature(0.0);
        params.set_temperature_inc(TEMPERATURE_INCREMENT);
        params.set_entropy_thold(COMPRESSION_RATIO_THRESHOLD);
        params.set_logprob_thold(LOG_PROB_THRESHOLD);
        params.set_no_speech_thold(NO_SPEECH_THRESHOLD);
        // whisper-rs 0.16's `set_abort_callback_safe` casts its boxed closure back to the wrong
        // type (so it aborts at random), hence the raw callback. `stop` outlives `full` below.
        // SAFETY: the callback only reads the `StopSignal` behind the pointer.
        unsafe {
            params.set_abort_callback(Some(stop_is_set));
            params.set_abort_callback_user_data(std::ptr::from_ref(stop).cast_mut().cast());
        }

        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = state.full(params, samples);
        stop.check()?;
        result
            .map_err(|error| PortError::Failed(format!("Whisper stopped with error {error:?}.")))?;
        // A segment this repetitive after every fallback is a hallucination loop ("tak tak tak …"),
        // not dictation.
        let text = state
            .as_iter()
            .filter_map(|segment| {
                segment
                    .to_str_lossy()
                    .ok()
                    .map(|text| text.trim().to_owned())
            })
            .filter(|text| compression_ratio(text) <= COMPRESSION_RATIO_THRESHOLD)
            .collect::<Vec<_>>()
            .join(" ");
        let detected = match language {
            Some(code) => Some(code.to_owned()),
            None => whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_owned),
        };
        Ok((text, detected))
    }
}

/// whisper.cpp's abort callback: `user_data` is the `StopSignal` of the running `full` call.
unsafe extern "C" fn stop_is_set(user_data: *mut c_void) -> bool {
    // SAFETY: `transcribe_piece` passes a `&StopSignal` that outlives the call.
    unsafe { &*user_data.cast::<StopSignal>() }.is_set()
}

/// The model as the catalog loads it.
pub fn load(folder: &Path) -> PortResult<Box<dyn LoadedSpeechModel>> {
    Ok(Box::new(WhisperCppModel::load(folder)?))
}

impl LoadedSpeechModel for WhisperCppModel {
    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
        stop: &StopSignal,
    ) -> PortResult<String> {
        // Audio over 30 s is transcribed piece by piece, cut at pauses, so no word is split at a
        // window boundary and the end isn't skipped. See `long_audio_splitter`.
        let pieces = long_audio_splitter::ranges(
            samples,
            long_audio_splitter::MAX_LENGTH,
            long_audio_splitter::MIN_LENGTH,
        );
        let mut language = language.map(str::to_owned);
        let mut texts = Vec::new();
        for piece in &pieces {
            stop.check()?;
            let audio = &samples[piece.clone()];
            // The recording as a whole was checked for speech; a silent piece of a long one would
            // only give Whisper room to invent text.
            if pieces.len() > 1 && !speech_activity::contains_speech(audio) {
                continue;
            }
            let (text, detected) = self.transcribe_piece(audio, language.as_deref(), stop)?;
            // One dictation is one language: later pieces use the language detected in the first
            // piece that held speech.
            if language.is_none() && !text.is_empty() {
                language = detected;
            }
            // On silence Whisper may repeat the prompt instead of returning nothing.
            if !dictation_prompt::is_echo(&text) {
                texts.push(text);
            }
        }
        Ok(texts.join(" "))
    }
}

/// How well `text` compresses with zlib, as OpenAI's Whisper measures repetitiveness.
pub fn compression_ratio(text: &str) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    let compressed = encoder
        .write_all(text.as_bytes())
        .and_then(|()| encoder.finish());
    compressed.map_or(0.0, |bytes| text.len() as f32 / bytes.len() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repetitive_text_compresses_past_the_threshold() {
        assert!(compression_ratio(&"tak ".repeat(50)) > COMPRESSION_RATIO_THRESHOLD);
        assert!(
            compression_ratio("The quick brown fox jumps over the lazy dog.")
                < COMPRESSION_RATIO_THRESHOLD
        );
        assert_eq!(compression_ratio(""), 0.0);
    }
}
