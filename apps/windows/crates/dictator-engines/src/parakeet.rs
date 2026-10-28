//! NVIDIA Parakeet TDT 0.6B v2 (int8 ONNX export for sherpa-onnx) behind `LoadedSpeechModel`. The
//! only code that uses sherpa-onnx. Loads from the verified model folder; never downloads.

use std::path::Path;
use std::thread;

use dictator_core::{LoadedSpeechModel, PortError, PortResult, StopSignal};
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig};

use crate::{SAMPLE_RATE, long_audio_splitter, require_files, utf8_path};

pub const MODEL_ID: &str = "parakeet-tdt-0.6b-v2-int8";
pub const REQUIRED_FILES: &[&str] = &[
    "encoder.int8.onnx",
    "decoder.int8.onnx",
    "joiner.int8.onnx",
    "tokens.txt",
];

/// ONNX Runtime threads; more gives little for one short utterance and starves the UI.
const MAX_THREADS: usize = 4;
/// On macOS FluidAudio windows long audio itself; sherpa-onnx encodes the whole input at once,
/// with memory growing with its square. Longer audio is cut at pauses into pieces of at most 60 s,
/// which also lets a stop take effect between pieces.
const MAX_PIECE_LENGTH: usize = 60 * SAMPLE_RATE;
/// Shortest piece a split may leave: 2 s.
const MIN_PIECE_LENGTH: usize = 2 * SAMPLE_RATE;

/// A loaded Parakeet model.
pub struct ParakeetOnnxModel {
    recognizer: OfflineRecognizer,
}

impl ParakeetOnnxModel {
    pub fn load(folder: &Path) -> PortResult<Self> {
        require_files(folder, REQUIRED_FILES)?;
        let file = |name: &str| utf8_path(&folder.join(name)).map(|path| Some(path.to_owned()));
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer.encoder = file(REQUIRED_FILES[0])?;
        config.model_config.transducer.decoder = file(REQUIRED_FILES[1])?;
        config.model_config.transducer.joiner = file(REQUIRED_FILES[2])?;
        config.model_config.tokens = file(REQUIRED_FILES[3])?;
        config.model_config.model_type = Some("nemo_transducer".into());
        config.model_config.provider = Some("cpu".into());
        config.model_config.num_threads = thread_count();
        let recognizer = OfflineRecognizer::create(&config)
            .ok_or_else(|| PortError::Failed("The Parakeet model could not be loaded.".into()))?;
        let model = Self { recognizer };
        // Pay ONNX Runtime's one-off setup during "Loading model…", not on the first dictation.
        let _ = model.decode(&[0.0; SAMPLE_RATE]);
        Ok(model)
    }

    fn decode(&self, samples: &[f32]) -> String {
        if samples.is_empty() {
            return String::new();
        }
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE as i32, samples);
        self.recognizer.decode(&stream);
        stream
            .get_result()
            .map(|result| result.text.trim().to_owned())
            .unwrap_or_default()
    }
}

/// The model as the catalog loads it.
pub fn load(folder: &Path) -> PortResult<Box<dyn LoadedSpeechModel>> {
    Ok(Box::new(ParakeetOnnxModel::load(folder)?))
}

impl LoadedSpeechModel for ParakeetOnnxModel {
    /// `language` is ignored: the model is English only.
    fn transcribe(
        &self,
        samples: &[f32],
        _language: Option<&str>,
        stop: &StopSignal,
    ) -> PortResult<String> {
        let mut texts = Vec::new();
        for piece in long_audio_splitter::ranges(samples, MAX_PIECE_LENGTH, MIN_PIECE_LENGTH) {
            stop.check()?;
            let text = self.decode(&samples[piece]);
            if !text.is_empty() {
                texts.push(text);
            }
        }
        stop.check()?;
        Ok(texts.join(" "))
    }
}

fn thread_count() -> i32 {
    let available = thread::available_parallelism().map_or(1, usize::from);
    available.min(MAX_THREADS) as i32
}
