//! The only code that touches model runtimes (ADR 0008, 0011): sherpa-onnx for Parakeet,
//! whisper.cpp for Whisper and llama.cpp for S1-mini. Everything here loads from a local, verified
//! model folder and never downloads. See README.md for native-library sourcing and packaging.

pub mod catalog;
pub mod long_audio_splitter;
pub mod parakeet;
pub mod s1_mini;
pub mod whisper;

pub use parakeet::ParakeetOnnxModel;
pub use s1_mini::S1MiniLlamaCleaner;
pub use whisper::WhisperCppModel;

use dictator_core::PortError;
use std::path::Path;

/// 16 kHz mono, what every model here expects.
pub(crate) const SAMPLE_RATE: usize = 16_000;

/// A path as UTF-8 for a C API, or a failure naming it.
pub(crate) fn utf8_path(path: &Path) -> Result<&str, PortError> {
    path.to_str().ok_or_else(|| {
        PortError::Failed(format!(
            "The model path {} is not valid Unicode.",
            path.display()
        ))
    })
}

/// Fails, naming every missing file, unless all `required` files are in `folder`. Each loader
/// checks first, so a half-installed folder never reaches a runtime (ADR 0008).
pub(crate) fn require_files(folder: &Path, required: &[&str]) -> Result<(), PortError> {
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|name| !folder.join(name).is_file())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(PortError::Failed(format!(
            "The model is incomplete; missing: {}.",
            missing.join(", ")
        )))
    }
}
