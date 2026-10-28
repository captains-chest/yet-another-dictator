//! The app's state and everything Settings can change. Shared between Tauri commands, the tray,
//! the keyboard hook and download threads, so it lives behind one mutex that is never held long.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use dictator_audio::{InputDevice, input_devices};
use dictator_cleanup::cleaning_transcriber::CleaningTranscriber;
use dictator_core::chord::KeySet;
use dictator_core::dictation_controller::Event;
use dictator_core::dictation_language::DictationLanguage;
use dictator_core::key_labels;
use dictator_core::model_session::ModelSession;
use dictator_core::shortcut::{Key, Shortcut};
use dictator_core::{CleanupStyle, StopSignal, Styling, Transcribing, TranscriptCleaning};
use dictator_engines::catalog::speech_engine;
use dictator_engines::s1_mini::S1MiniLlamaCleaner;
use dictator_models::catalog::{CLEANUP_MODEL_ID, SpeechModel};
use dictator_models::installer::{InstallError, ModelInstaller};
use dictator_models::manifest::ModelManifest;
use dictator_models::ranged_transport::{DEFAULT_CHUNK_BYTES, RangedTransport};
use dictator_windows::hotkey::{HotkeyAction, HotkeyListener};
use dictator_windows::platform;
use serde::{Deserialize, Serialize};

use crate::dictation::{Message, Runner, Snapshot};
use crate::settings::{self, Microphone, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShortcutKind {
    Dictate,
    Undo,
}

/// What the app needs from the UI layer, so this module doesn't depend on Tauri.
pub trait Shell: Send + Sync {
    /// Settings changed: re-render the windows.
    fn view_changed(&self, view: &View);
    /// The dictation state changed: update the tray and overlay.
    fn dictation_changed(&self, snapshot: &Snapshot, shortcut: &str);
    fn level(&self, level: f32);
    fn set_open_at_login(&self, on: bool) -> Result<(), String>;
    fn open_at_login(&self) -> bool;
}

struct Download {
    progress: f64,
    stop: StopSignal,
}

struct State {
    settings: Settings,
    downloads: HashMap<String, Download>,
    errors: HashMap<String, String>,
    microphones: Vec<InputDevice>,
    recording: Option<ShortcutKind>,
    recording_held: Vec<String>,
    shortcut_error: Option<String>,
    cleaner: Option<Arc<dyn TranscriptCleaning>>,
    transcriber: Option<Arc<CleaningTranscriber>>,
    snapshot: Option<Snapshot>,
}

pub struct App {
    settings_path: Option<PathBuf>,
    models_root: Option<PathBuf>,
    manifest: Option<ModelManifest>,
    pub version: String,
    state: Mutex<State>,
    pub runner: Runner,
    hotkeys: Arc<Mutex<HotkeyListener>>,
    shell: Arc<dyn Shell>,
}

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl App {
    pub fn new(
        shell: Arc<dyn Shell>,
        manifest: Option<ModelManifest>,
        version: String,
    ) -> Arc<Self> {
        let settings_path = Settings::default_path();
        let settings = settings_path
            .as_deref()
            .map(Settings::load)
            .unwrap_or_default();
        Arc::new_cyclic(|this: &std::sync::Weak<App>| {
            let on_change = this.clone();
            let on_level = shell.clone();
            let runner = Runner::start(crate::dictation::Callbacks {
                on_change: Box::new(move |snapshot| {
                    if let Some(app) = on_change.upgrade() {
                        app.dictation_changed(snapshot);
                    }
                }),
                on_level: Box::new(move |level| on_level.level(level)),
            });
            Self {
                settings_path,
                models_root: dictator_models::installer::default_root(),
                manifest,
                version,
                hotkeys: Arc::new(Mutex::new(HotkeyListener::new(
                    Some(settings.shortcut()),
                    settings.undo_shortcut(),
                ))),
                state: Mutex::new(State {
                    settings,
                    downloads: HashMap::new(),
                    errors: HashMap::new(),
                    microphones: Vec::new(),
                    recording: None,
                    recording_held: Vec::new(),
                    shortcut_error: None,
                    cleaner: None,
                    transcriber: None,
                    snapshot: None,
                }),
                runner,
                shell,
            }
        })
    }

    /// Applies the saved settings and loads the model. Returns whether a first model must be chosen.
    pub fn start(self: &Arc<Self>) -> bool {
        let settings = self.settings();
        self.runner
            .post(Message::SetMicrophone(settings.microphone.map(|m| m.id)));
        self.runner.post(Message::SetKeepMicrophoneReady(
            settings.keep_microphone_ready,
        ));
        self.apply_language();
        if settings.cleanup_enabled {
            self.enable_cleanup();
        }
        self.rebuild_transcriber();
        self.selected_model().is_none()
    }

    pub fn hotkeys(&self) -> Arc<Mutex<HotkeyListener>> {
        self.hotkeys.clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    pub fn settings(&self) -> Settings {
        self.state().settings.clone()
    }

    fn update_settings(&self, change: impl FnOnce(&mut Settings)) {
        let settings = {
            let mut state = self.state();
            change(&mut state.settings);
            state.settings.clone()
        };
        if let Some(path) = &self.settings_path
            && let Err(error) = settings.save(path)
        {
            eprintln!("Couldn't save settings: {error}");
        }
    }

    fn changed(&self) {
        self.shell.view_changed(&self.view());
    }

    // MARK: Dictation

    fn dictation_changed(&self, snapshot: &Snapshot) {
        self.state().snapshot = Some(snapshot.clone());
        self.shell
            .dictation_changed(snapshot, &self.settings().shortcut().display_string());
    }

    pub fn snapshot(&self) -> Option<Snapshot> {
        self.state().snapshot.clone()
    }

    /// Runs on the hotkey thread for each action the listener reports.
    pub fn hotkey_action(&self, action: HotkeyAction) {
        let event = match action {
            HotkeyAction::Pressed => Event::HotkeyPressed,
            HotkeyAction::Released => Event::HotkeyReleased,
            // A modifier-only shortcut held while another key is pressed: no release will come.
            HotkeyAction::Interrupted => Event::Cancel,
            HotkeyAction::OtherInput => Event::OtherInput,
            HotkeyAction::Undo => Event::UndoRequested,
            HotkeyAction::RecordingHeld(keys) => {
                self.state().recording_held = labels(&keys);
                return self.changed();
            }
            HotkeyAction::Recorded(keys) => return self.recorded(&keys),
        };
        self.runner.send(event);
    }

    // MARK: Shortcuts

    pub fn record_shortcut(&self, kind: ShortcutKind) {
        {
            let mut state = self.state();
            state.recording = Some(kind);
            state.recording_held.clear();
            state.shortcut_error = None;
        }
        lock(&self.hotkeys).start_recording();
        self.changed();
    }

    /// Settings closed mid-recording: give the keyboard back.
    pub fn cancel_recording(&self) {
        if self.state().recording.take().is_some() {
            lock(&self.hotkeys).stop_recording();
            self.changed();
        }
    }

    fn recorded(&self, keys: &KeySet) {
        let shortcut = shortcut_from(keys);
        let kind = self.state().recording.take();
        let settings = self.settings();
        let error = match kind {
            _ if shortcut.validate().is_err() => Some("Press at least one key.".to_owned()),
            Some(ShortcutKind::Undo) if shortcut == settings.shortcut() => {
                Some("That is already the dictation shortcut.".to_owned())
            }
            Some(ShortcutKind::Dictate) if Some(&shortcut) == settings.undo_shortcut().as_ref() => {
                Some("That is already the undo shortcut.".to_owned())
            }
            _ => None,
        };
        match (kind, &error) {
            (Some(ShortcutKind::Dictate), None) => {
                self.update_settings(|s| s.shortcut = Some(settings::stored(&shortcut)))
            }
            (Some(ShortcutKind::Undo), None) => {
                self.update_settings(|s| s.undo_shortcut = Some(settings::stored(&shortcut)))
            }
            _ => {}
        }
        self.state().shortcut_error = error;
        self.apply_shortcuts();
    }

    pub fn clear_undo_shortcut(&self) {
        self.update_settings(|s| s.undo_shortcut = None);
        self.apply_shortcuts();
    }

    fn apply_shortcuts(&self) {
        let settings = self.settings();
        *lock(&self.hotkeys) =
            HotkeyListener::new(Some(settings.shortcut()), settings.undo_shortcut());
        self.changed();
        if let Some(snapshot) = self.snapshot() {
            self.shell
                .dictation_changed(&snapshot, &settings.shortcut().display_string());
        }
    }

    // MARK: Speech models

    fn installer(&self, id: &str) -> Option<ModelInstaller> {
        Some(ModelInstaller::new(
            self.manifest.clone()?,
            id,
            self.models_root.clone()?,
        ))
    }

    fn is_installed(&self, id: &str) -> bool {
        self.installer(id)
            .is_some_and(|installer| installer.is_installed())
    }

    /// The chosen speech model, if it is installed.
    fn selected_model(&self) -> Option<SpeechModel> {
        let model = SpeechModel::from_id(self.settings().speech_model.as_deref()?)?;
        self.is_installed(model.id()).then_some(model)
    }

    /// Uses `id` now if it is installed, otherwise downloads it and then uses it.
    pub fn select_model(self: &Arc<Self>, id: &str) {
        if SpeechModel::from_id(id).is_none() {
            return;
        }
        if self.is_installed(id) {
            self.use_model(id);
        } else {
            let id = id.to_owned();
            self.download(&id.clone(), move |app| app.use_model(&id));
        }
    }

    fn use_model(&self, id: &str) {
        self.update_settings(|s| s.speech_model = Some(id.to_owned()));
        self.apply_language();
        self.rebuild_transcriber();
        self.changed();
    }

    pub fn delete_model(&self, id: &str) {
        if self.settings().speech_model.as_deref() == Some(id) && self.is_installed(id) {
            return;
        }
        self.uninstall(id);
    }

    fn uninstall(&self, id: &str) {
        if let Some(installer) = self.installer(id)
            && let Err(error) = installer.uninstall()
        {
            self.state()
                .errors
                .insert(id.to_owned(), format!("Couldn't delete the model: {error}"));
        }
        self.changed();
    }

    /// A transcriber for the selected model, with cleanup layered on. The model loads on first use.
    fn rebuild_transcriber(&self) {
        let transcriber = self.selected_model().and_then(|model| {
            let engine = speech_engine(model.id())?;
            let folder = self.installer(model.id())?.folder();
            let required = engine
                .required_files
                .iter()
                .map(|file| (*file).to_owned())
                .collect();
            let session = ModelSession::new(folder, required, Box::new(engine.load));
            Some(Arc::new(CleaningTranscriber::new(Arc::new(session))))
        });
        let mut state = self.state();
        if let Some(transcriber) = &transcriber {
            transcriber.set_cleaner(state.cleaner.clone());
            transcriber.set_style(CleanupStyle {
                styling: state.settings.styling(),
                ..CleanupStyle::default()
            });
        }
        state.transcriber = transcriber.clone();
        drop(state);
        self.runner.post(Message::SetTranscriber(
            transcriber.map(|t| t as Arc<dyn Transcribing>),
        ));
    }

    /// Downloads a model in the background, then runs `done` if it completed.
    fn download(self: &Arc<Self>, id: &str, done: impl FnOnce(&Arc<Self>) + Send + 'static) {
        let Some(installer) = self.installer(id) else {
            self.state().errors.insert(
                id.to_owned(),
                "The model list is missing; reinstall Dictator.".into(),
            );
            return self.changed();
        };
        let stop = StopSignal::new();
        {
            let mut state = self.state();
            if state.downloads.contains_key(id) {
                return;
            }
            state.errors.remove(id);
            state.downloads.insert(
                id.to_owned(),
                Download {
                    progress: 0.0,
                    stop: stop.clone(),
                },
            );
        }
        self.changed();
        let app = self.clone();
        let id = id.to_owned();
        thread::spawn(move || {
            let mut shown = 0.0;
            let result = installer.install(
                &RangedTransport::new(DEFAULT_CHUNK_BYTES),
                &mut |progress| {
                    let fraction = progress.fraction();
                    if let Some(download) = app.state().downloads.get_mut(&id) {
                        download.progress = fraction;
                    }
                    // Re-render every half percent, not for every chunk.
                    if fraction - shown >= 0.005 {
                        shown = fraction;
                        app.changed();
                    }
                },
                &stop,
            );
            app.state().downloads.remove(&id);
            match result {
                Ok(()) => done(&app),
                Err(InstallError::Cancelled) => {}
                Err(error) => {
                    app.state().errors.insert(id, error.to_string());
                }
            }
            app.changed();
        });
    }

    fn cancel_download(&self, id: &str) {
        if let Some(download) = self.state().downloads.remove(id) {
            download.stop.set();
        }
    }

    // MARK: Language

    pub fn set_language(&self, code: Option<&str>) {
        self.update_settings(|s| s.language = code.map(str::to_owned));
        self.apply_language();
        self.changed();
    }

    fn effective_language(&self) -> DictationLanguage {
        let chosen = self.settings().language();
        let supported = self.selected_model().map_or(
            &DictationLanguage::ALL[..],
            SpeechModel::supported_languages,
        );
        if supported.contains(&chosen) {
            chosen
        } else {
            supported.first().copied().unwrap_or_default()
        }
    }

    fn apply_language(&self) {
        self.runner
            .post(Message::SetLanguage(self.effective_language()));
    }

    // MARK: Cleanup

    pub fn set_cleanup_enabled(self: &Arc<Self>, enabled: bool) {
        self.update_settings(|s| s.cleanup_enabled = enabled);
        if enabled {
            self.enable_cleanup();
        } else {
            self.cancel_download(CLEANUP_MODEL_ID);
            self.set_cleaner(None);
        }
        self.changed();
    }

    fn enable_cleanup(self: &Arc<Self>) {
        if self.is_installed(CLEANUP_MODEL_ID) {
            self.load_cleaner();
        } else {
            self.download(CLEANUP_MODEL_ID, |app| {
                if app.settings().cleanup_enabled {
                    app.load_cleaner();
                }
            });
        }
    }

    /// Loads the cleanup model off the dictation path; transcripts stay raw until it is ready.
    fn load_cleaner(self: &Arc<Self>) {
        let Some(folder) = self
            .installer(CLEANUP_MODEL_ID)
            .map(|installer| installer.folder())
        else {
            return;
        };
        let app = self.clone();
        thread::spawn(move || {
            let cleaner: Arc<dyn TranscriptCleaning> = Arc::new(S1MiniLlamaCleaner::new(&folder));
            match cleaner.prepare() {
                Ok(()) if app.settings().cleanup_enabled => app.set_cleaner(Some(cleaner)),
                Ok(()) => {}
                Err(error) => {
                    app.state().errors.insert(
                        CLEANUP_MODEL_ID.to_owned(),
                        format!("Cleanup couldn't start: {error}"),
                    );
                }
            }
            app.changed();
        });
    }

    fn set_cleaner(&self, cleaner: Option<Arc<dyn TranscriptCleaning>>) {
        let mut state = self.state();
        if let Some(transcriber) = &state.transcriber {
            transcriber.set_cleaner(cleaner.clone());
        }
        state.cleaner = cleaner;
    }

    pub fn set_cleanup_styling(&self, styling: &str) {
        self.update_settings(|s| s.cleanup_styling = Some(styling.to_owned()));
        let state = self.state();
        if let Some(transcriber) = &state.transcriber {
            transcriber.set_style(CleanupStyle {
                styling: state.settings.styling(),
                ..transcriber.style()
            });
        }
        drop(state);
        self.changed();
    }

    pub fn delete_cleanup_model(&self) {
        if !self.settings().cleanup_enabled {
            self.uninstall(CLEANUP_MODEL_ID);
        }
    }

    // MARK: Microphone

    pub fn refresh_microphones(&self) {
        let devices = input_devices();
        self.state().microphones = devices;
    }

    pub fn set_microphone(&self, id: Option<&str>) {
        let microphone = id.map(|id| {
            let name = self
                .state()
                .microphones
                .iter()
                .find(|d| d.id == id)
                .map(|d| d.name.clone());
            Microphone {
                id: id.to_owned(),
                name: name.unwrap_or_else(|| id.to_owned()),
            }
        });
        self.update_settings(|s| s.microphone = microphone);
        self.runner
            .post(Message::SetMicrophone(id.map(str::to_owned)));
        self.changed();
    }

    pub fn set_keep_microphone_ready(&self, on: bool) {
        self.update_settings(|s| s.keep_microphone_ready = on);
        self.runner.post(Message::SetKeepMicrophoneReady(on));
        self.changed();
    }

    // MARK: System

    pub fn set_open_at_login(&self, on: bool) {
        if let Err(error) = self.shell.set_open_at_login(on) {
            eprintln!("Couldn't change open at login: {error}");
        }
        self.changed();
    }

    pub fn set_check_for_updates(&self, on: bool) {
        self.update_settings(|s| s.check_for_updates = on);
        self.changed();
    }

    pub fn set_last_seen_version(&self, version: &str) {
        self.update_settings(|s| s.last_seen_version = Some(version.to_owned()));
    }

    pub fn set_last_update_check(&self, seconds: u64) {
        self.update_settings(|s| s.last_update_check = Some(seconds));
    }

    // MARK: View

    pub fn view(&self) -> View {
        let state = self.state();
        let settings = &state.settings;
        let installed = |id: &str| {
            self.installer(id)
                .is_some_and(|installer| installer.is_installed())
        };
        let bytes = |id: &str| {
            self.manifest
                .as_ref()
                .map_or(0, |manifest| manifest.total_bytes(id))
        };
        let progress = |id: &str| state.downloads.get(id).map(|download| download.progress);
        let error = |id: &str| state.errors.get(id).cloned();

        let models = SpeechModel::RANKED
            .iter()
            .enumerate()
            .map(|(rank, &model)| ModelView {
                id: model.id(),
                name: model.display_name(),
                recommended: rank == 0,
                bytes: bytes(model.id()),
                note: model.accuracy_note(),
                source: model.source(),
                installed: installed(model.id()),
                progress: progress(model.id()),
                error: error(model.id()),
            })
            .collect();
        let selected = settings
            .speech_model
            .as_deref()
            .and_then(SpeechModel::from_id)
            .filter(|m| installed(m.id()));
        let supported = selected.map_or(
            &DictationLanguage::ALL[..],
            SpeechModel::supported_languages,
        );
        let chosen_language = settings.language();
        let language = if supported.contains(&chosen_language) {
            chosen_language
        } else {
            supported[0]
        };

        let microphone = settings.microphone.clone();
        let connected = microphone
            .as_ref()
            .is_none_or(|m| state.microphones.iter().any(|d| d.id == m.id));
        View {
            version: self.version.clone(),
            recording_shortcut: state.recording,
            recording_held: state.recording_held.clone(),
            shortcut: labels_of(&settings.shortcut()),
            undo_shortcut: settings.undo_shortcut().as_ref().map(labels_of),
            shortcut_error: state.shortcut_error.clone(),
            models,
            selected_model: selected.map(SpeechModel::id),
            languages: supported
                .iter()
                .map(|language| LanguageView {
                    code: language.raw_value(),
                    name: language.display_name(),
                })
                .collect(),
            language: language.raw_value(),
            language_note: selected.map(SpeechModel::language_note),
            cleanup: CleanupView {
                enabled: settings.cleanup_enabled,
                progress: progress(CLEANUP_MODEL_ID),
                stylings: Styling::ALL
                    .iter()
                    .map(|styling| StylingView {
                        id: styling.name(),
                        name: capitalized(styling.name()),
                    })
                    .collect(),
                styling: settings.styling().name(),
                bytes: bytes(CLEANUP_MODEL_ID),
                error: error(CLEANUP_MODEL_ID),
                installed: installed(CLEANUP_MODEL_ID),
            },
            microphones: state
                .microphones
                .iter()
                .map(|device| MicrophoneView {
                    id: device.id.clone(),
                    name: device.name.clone(),
                })
                .collect(),
            microphone_name: microphone.as_ref().map(|m| m.name.clone()),
            microphone: microphone.map(|m| m.id),
            microphone_connected: connected,
            keep_microphone_ready: settings.keep_microphone_ready,
            open_at_login: self.shell.open_at_login(),
            check_for_updates: settings.check_for_updates,
        }
    }

    pub fn retry_undelivered(&self) {
        self.runner.send(Event::RetryUndelivered);
    }
}

fn shortcut_from(keys: &KeySet) -> Shortcut {
    Shortcut::new(
        keys.iter()
            .map(|&code| {
                Key::new(
                    code,
                    &key_labels::label(code, platform::typed_character(code.0).as_deref()),
                )
            })
            .collect(),
    )
}

/// Labels in the shortcut's display order.
fn labels(keys: &KeySet) -> Vec<String> {
    labels_of(&shortcut_from(keys))
}

fn labels_of(shortcut: &Shortcut) -> Vec<String> {
    shortcut
        .keys()
        .iter()
        .map(|key| key.label.clone())
        .collect()
}

fn capitalized(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    version: String,
    recording_shortcut: Option<ShortcutKind>,
    recording_held: Vec<String>,
    shortcut: Vec<String>,
    undo_shortcut: Option<Vec<String>>,
    shortcut_error: Option<String>,
    models: Vec<ModelView>,
    selected_model: Option<&'static str>,
    languages: Vec<LanguageView>,
    language: &'static str,
    language_note: Option<&'static str>,
    cleanup: CleanupView,
    microphones: Vec<MicrophoneView>,
    microphone: Option<String>,
    microphone_name: Option<String>,
    microphone_connected: bool,
    keep_microphone_ready: bool,
    open_at_login: bool,
    check_for_updates: bool,
}

#[derive(Serialize)]
struct ModelView {
    id: &'static str,
    name: &'static str,
    recommended: bool,
    bytes: u64,
    note: &'static str,
    source: &'static str,
    installed: bool,
    progress: Option<f64>,
    error: Option<String>,
}

#[derive(Serialize)]
struct LanguageView {
    code: &'static str,
    name: &'static str,
}

#[derive(Serialize)]
struct CleanupView {
    enabled: bool,
    progress: Option<f64>,
    stylings: Vec<StylingView>,
    styling: &'static str,
    bytes: u64,
    error: Option<String>,
    installed: bool,
}

#[derive(Serialize)]
struct StylingView {
    id: &'static str,
    name: String,
}

#[derive(Serialize)]
struct MicrophoneView {
    id: String,
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictator_core::chord::KeyCode;

    #[test]
    fn labels_follow_the_shortcut_order() {
        let keys = KeySet::from([KeyCode(0x44), KeyCode::LEFT_ALT, KeyCode::LEFT_CONTROL]);
        assert_eq!(
            labels(&keys),
            Shortcut::default()
                .keys()
                .iter()
                .map(|k| k.label.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn styling_names_are_capitalized() {
        assert_eq!(capitalized("semi-formal"), "Semi-formal");
        assert_eq!(capitalized(""), "");
    }
}
