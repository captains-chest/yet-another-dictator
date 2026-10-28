//! The catalog must match the shipped Windows manifest.

use dictator_core::dictation_language::DictationLanguage;
use dictator_models::{CLEANUP_MODEL_ID, ModelManifest, SpeechModel};

const WINDOWS_MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../model-manifest.txt");
const MACOS_MANIFEST: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../macos/scripts/model-manifest.txt"
);

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn every_model_and_required_file_is_in_the_manifest() {
    let manifest = ModelManifest::parse(&read(WINDOWS_MANIFEST)).unwrap();
    for model in SpeechModel::RANKED {
        let files: Vec<&str> = manifest
            .files(model.id())
            .iter()
            .map(|e| e.local_path.as_str())
            .collect();
        assert!(!files.is_empty(), "{} missing", model.id());
        for required in model.required_files() {
            assert!(files.contains(required), "{} lacks {required}", model.id());
        }
    }
    assert!(!manifest.files(CLEANUP_MODEL_ID).is_empty());
}

#[test]
fn the_default_model_is_listed_first() {
    let manifest = ModelManifest::parse(&read(WINDOWS_MANIFEST)).unwrap();
    assert_eq!(manifest.model_ids()[0], SpeechModel::default().id());
    assert_eq!(SpeechModel::default().id(), "parakeet-tdt-0.6b-v2-int8");
}

#[test]
fn every_remote_path_is_pinned_to_a_commit() {
    let manifest = ModelManifest::parse(&read(WINDOWS_MANIFEST)).unwrap();
    for entry in manifest.entries() {
        let revision = entry
            .remote_path
            .split("/resolve/")
            .nth(1)
            .and_then(|rest| rest.split('/').next());
        assert!(
            revision.is_some_and(|r| r.len() == 40 && r.bytes().all(|b| b.is_ascii_hexdigit())),
            "{} is not pinned",
            entry.remote_path
        );
    }
}

#[test]
fn phonon_2_is_not_offered() {
    let manifest = ModelManifest::parse(&read(WINDOWS_MANIFEST)).unwrap();
    assert!(
        manifest
            .model_ids()
            .iter()
            .all(|id| !id.to_lowercase().contains("phonon"))
    );
    assert!(
        SpeechModel::RANKED
            .iter()
            .all(|m| !m.id().contains("phonon"))
    );
}

#[test]
fn s1_mini_lines_match_the_macos_manifest() {
    let s1_lines = |text: String| -> Vec<String> {
        text.lines()
            .filter(|l| l.starts_with(&format!("{CLEANUP_MODEL_ID} ")))
            .map(String::from)
            .collect()
    };
    let windows = s1_lines(read(WINDOWS_MANIFEST));
    assert_eq!(windows.len(), 3);
    assert_eq!(windows, s1_lines(read(MACOS_MANIFEST)));
}

#[test]
fn names_and_languages_match_the_macos_app() {
    assert_eq!(
        SpeechModel::ParakeetV2.display_name(),
        "NVIDIA Parakeet TDT 0.6B v2"
    );
    assert_eq!(
        SpeechModel::WhisperLargeV3Turbo.display_name(),
        "Whisper Large v3 Turbo"
    );
    assert_eq!(
        SpeechModel::ParakeetV2.supported_languages(),
        [DictationLanguage::English]
    );
    assert_eq!(
        SpeechModel::WhisperLargeV3Turbo.supported_languages(),
        [DictationLanguage::Automatic, DictationLanguage::English]
    );
    assert_eq!(SpeechModel::ParakeetV2.language_note(), "English only");
    assert_eq!(
        SpeechModel::WhisperLargeV3Turbo.language_note(),
        "Automatic or English"
    );
    for model in SpeechModel::RANKED {
        assert_eq!(SpeechModel::from_id(model.id()), Some(model));
    }
    assert_eq!(SpeechModel::from_id(CLEANUP_MODEL_ID), None);
}
