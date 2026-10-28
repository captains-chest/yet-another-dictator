//! What the app needs to build a `dictator_core::model_session::ModelSession` for a speech model:
//! the files that must be in its folder and the loader to call once they are.

use std::path::Path;

use dictator_core::{LoadedSpeechModel, PortResult};

use crate::{parakeet, whisper};

pub type Loader = fn(&Path) -> PortResult<Box<dyn LoadedSpeechModel>>;

/// One speech model this crate can run, keyed by its id in `model-manifest.txt`.
#[derive(Clone, Copy)]
pub struct SpeechEngine {
    pub model_id: &'static str,
    /// Checked before loading, so a half-installed folder is refused without touching the runtime.
    pub required_files: &'static [&'static str],
    /// Blocking; call off the UI thread.
    pub load: Loader,
}

pub const PARAKEET: SpeechEngine = SpeechEngine {
    model_id: parakeet::MODEL_ID,
    required_files: parakeet::REQUIRED_FILES,
    load: parakeet::load,
};

pub const WHISPER: SpeechEngine = SpeechEngine {
    model_id: whisper::MODEL_ID,
    required_files: whisper::REQUIRED_FILES,
    load: whisper::load,
};

/// Best first, like the manifest.
pub const SPEECH_ENGINES: [SpeechEngine; 2] = [PARAKEET, WHISPER];

pub fn speech_engine(model_id: &str) -> Option<SpeechEngine> {
    SPEECH_ENGINES
        .into_iter()
        .find(|engine| engine.model_id == model_id)
}

/// The transcript-cleanup model (ADR 0011); see [`crate::S1MiniLlamaCleaner`].
pub const CLEANUP_MODEL_ID: &str = crate::s1_mini::MODEL_ID;
pub const CLEANUP_REQUIRED_FILES: &[&str] = &[crate::s1_mini::MODEL_FILE_NAME];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every file listed here must be pinned in the manifest the installer verifies.
    #[test]
    fn required_files_are_in_the_manifest() {
        let manifest = include_str!("../../../model-manifest.txt");
        let pinned: Vec<(&str, &str)> = manifest
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                (fields.len() == 5).then(|| (fields[0], fields[4]))
            })
            .collect();
        let models = SPEECH_ENGINES
            .iter()
            .map(|engine| (engine.model_id, engine.required_files))
            .chain([(CLEANUP_MODEL_ID, CLEANUP_REQUIRED_FILES)]);
        for (model_id, files) in models {
            for file in files {
                assert!(
                    pinned.contains(&(model_id, file)),
                    "{model_id}/{file} is not in the manifest"
                );
            }
        }
    }

    #[test]
    fn engines_are_found_by_id() {
        assert_eq!(
            speech_engine("parakeet-tdt-0.6b-v2-int8").map(|engine| engine.model_id),
            Some(parakeet::MODEL_ID)
        );
        assert_eq!(
            speech_engine("whisper-large-v3-turbo-q5_0").map(|engine| engine.model_id),
            Some(whisper::MODEL_ID)
        );
        assert!(speech_engine("phonon-2").is_none());
    }
}
