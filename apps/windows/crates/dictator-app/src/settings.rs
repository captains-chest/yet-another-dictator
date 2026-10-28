//! The user's choices, kept as JSON in `%LOCALAPPDATA%\Dictator\settings.json` (per user, like the
//! install). A missing or unreadable file gives the defaults.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use dictator_core::Styling;
use dictator_core::chord::KeyCode;
use dictator_core::dictation_language::DictationLanguage;
use dictator_core::shortcut::{Key, Shortcut};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredKey {
    pub code: u32,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Microphone {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// The installed speech model in use; `None` until the first one is downloaded.
    pub speech_model: Option<String>,
    pub language: Option<String>,
    /// `None` is the default shortcut.
    pub shortcut: Option<Vec<StoredKey>>,
    pub undo_shortcut: Option<Vec<StoredKey>>,
    pub cleanup_enabled: bool,
    pub cleanup_styling: Option<String>,
    /// `None` follows the Windows default input.
    pub microphone: Option<Microphone>,
    pub keep_microphone_ready: bool,
    pub check_for_updates: bool,
    pub last_seen_version: Option<String>,
    /// Seconds since the Unix epoch.
    pub last_update_check: Option<u64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            speech_model: None,
            language: None,
            shortcut: None,
            undo_shortcut: None,
            cleanup_enabled: false,
            cleanup_styling: None,
            microphone: None,
            keep_microphone_ready: false,
            check_for_updates: true,
            last_seen_version: None,
            last_update_check: None,
        }
    }
}

fn to_shortcut(keys: &[StoredKey]) -> Option<Shortcut> {
    let shortcut = Shortcut::new(
        keys.iter()
            .map(|key| Key::new(KeyCode(key.code), &key.label))
            .collect(),
    );
    shortcut.validate().ok().map(|()| shortcut)
}

pub fn stored(shortcut: &Shortcut) -> Vec<StoredKey> {
    shortcut
        .keys()
        .iter()
        .map(|key| StoredKey {
            code: key.code.0,
            label: key.label.clone(),
        })
        .collect()
}

impl Settings {
    /// `settings.json` next to the model folder: `%LOCALAPPDATA%\Dictator`.
    pub fn default_path() -> Option<PathBuf> {
        dictator_models::installer::default_root()?
            .parent()
            .map(|dir| dir.join("settings.json"))
    }

    pub fn load(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Writes a temporary file and renames it, so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, path)
    }

    pub fn shortcut(&self) -> Shortcut {
        self.shortcut
            .as_deref()
            .and_then(to_shortcut)
            .unwrap_or_default()
    }

    pub fn undo_shortcut(&self) -> Option<Shortcut> {
        self.undo_shortcut.as_deref().and_then(to_shortcut)
    }

    pub fn language(&self) -> DictationLanguage {
        DictationLanguage::from_stored(self.language.as_deref())
    }

    pub fn styling(&self) -> Styling {
        Styling::ALL
            .into_iter()
            .find(|styling| Some(styling.name()) == self.cleanup_styling.as_deref())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_dictate_with_ctrl_alt_d_and_check_for_updates() {
        let settings = Settings::default();
        assert_eq!(settings.shortcut(), Shortcut::default());
        assert_eq!(settings.undo_shortcut(), None);
        assert!(settings.check_for_updates);
        assert!(!settings.cleanup_enabled);
        assert_eq!(settings.styling(), Styling::SemiFormal);
        assert_eq!(settings.language(), DictationLanguage::Automatic);
    }

    #[test]
    fn round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Dictator").join("settings.json");
        let settings = Settings {
            speech_model: Some("parakeet-tdt-0.6b-v2-int8".into()),
            language: Some("english".into()),
            undo_shortcut: Some(vec![
                StoredKey {
                    code: KeyCode::LEFT_CONTROL.0,
                    label: "Ctrl".into(),
                },
                StoredKey {
                    code: 0x5A,
                    label: "Z".into(),
                },
            ]),
            cleanup_enabled: true,
            cleanup_styling: Some("casual".into()),
            microphone: Some(Microphone {
                id: "usb".into(),
                name: "USB Mic".into(),
            }),
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded, settings);
        assert_eq!(loaded.styling(), Styling::Casual);
        assert_eq!(loaded.language(), DictationLanguage::English);
        assert_eq!(
            loaded.undo_shortcut().unwrap().display_string(),
            Shortcut::new(vec![
                Key::new(KeyCode::LEFT_CONTROL, "Ctrl"),
                Key::new(KeyCode(0x5A), "Z"),
            ])
            .display_string()
        );
    }

    #[test]
    fn a_missing_or_unreadable_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn unknown_fields_and_values_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            r#"{"futureSetting":1,"cleanupStyling":"shouty","language":"danish","shortcut":[]}"#,
        )
        .unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.styling(), Styling::SemiFormal);
        assert_eq!(settings.language(), DictationLanguage::Automatic);
        assert_eq!(
            settings.shortcut(),
            Shortcut::default(),
            "an empty shortcut is invalid"
        );
        assert!(
            settings.check_for_updates,
            "missing fields keep their defaults"
        );
    }
}
