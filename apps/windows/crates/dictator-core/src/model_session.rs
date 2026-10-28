//! Port of the macOS `ModelSession` (ADR 0008): runs one speech model. Loads it once, shares that
//! load between concurrent `prepare` calls, and serialises transcriptions in arrival order so a
//! new one waits for an abandoned one. Blocking; call off the UI thread.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use crate::ports::{LoadedSpeechModel, PortError, PortResult, StopSignal, Transcribing};

pub type ModelLoader = Box<dyn Fn(&Path) -> PortResult<Box<dyn LoadedSpeechModel>> + Send + Sync>;

/// The user-facing message when required files are missing.
pub fn missing_model_files_message(files: &[String]) -> String {
    format!("Model is incomplete, missing: {}", files.join(", "))
}

enum Load {
    Idle,
    Loading,
    Loaded(Arc<dyn LoadedSpeechModel>),
}

struct LoadState {
    load: Load,
    /// Bumped by every load attempt, so waiters can tell their attempt failed.
    attempt: u64,
    last_failure: Option<PortError>,
}

/// FIFO ticket queue for transcriptions.
#[derive(Default)]
struct Queue {
    next_ticket: u64,
    serving: u64,
}

pub struct ModelSession {
    model_folder: PathBuf,
    required_files: Vec<String>,
    load_model: ModelLoader,
    load: Mutex<LoadState>,
    loaded: Condvar,
    queue: Mutex<Queue>,
    turn: Condvar,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl ModelSession {
    pub fn new(
        model_folder: impl Into<PathBuf>,
        required_files: Vec<String>,
        load: ModelLoader,
    ) -> Self {
        Self {
            model_folder: model_folder.into(),
            required_files,
            load_model: load,
            load: Mutex::new(LoadState {
                load: Load::Idle,
                attempt: 0,
                last_failure: None,
            }),
            loaded: Condvar::new(),
            queue: Mutex::new(Queue::default()),
            turn: Condvar::new(),
        }
    }

    fn ensure_loaded(&self) -> PortResult<Arc<dyn LoadedSpeechModel>> {
        let mut state = lock(&self.load);
        loop {
            match &state.load {
                Load::Loaded(model) => return Ok(model.clone()),
                Load::Loading => {
                    let attempt = state.attempt;
                    state = self
                        .loaded
                        .wait_while(state, |s| {
                            matches!(s.load, Load::Loading) && s.attempt == attempt
                        })
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if state.attempt == attempt && matches!(state.load, Load::Idle) {
                        return Err(state.last_failure.clone().unwrap_or(PortError::Cancelled));
                    }
                }
                Load::Idle => break,
            }
        }
        let missing: Vec<String> = self
            .required_files
            .iter()
            .filter(|f| !self.model_folder.join(f).exists())
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(PortError::Failed(missing_model_files_message(&missing)));
        }
        state.load = Load::Loading;
        state.attempt += 1;
        drop(state);

        let result = (self.load_model)(&self.model_folder).map(Arc::<dyn LoadedSpeechModel>::from);

        let mut state = lock(&self.load);
        match &result {
            Ok(model) => state.load = Load::Loaded(model.clone()),
            Err(error) => {
                state.load = Load::Idle;
                state.last_failure = Some(error.clone());
            }
        }
        drop(state);
        self.loaded.notify_all();
        result
    }
}

impl Transcribing for ModelSession {
    fn prepare(&self) -> PortResult<()> {
        self.ensure_loaded().map(|_| ())
    }

    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
        stop: &StopSignal,
    ) -> PortResult<String> {
        let model = self.ensure_loaded()?;
        let mut queue = lock(&self.queue);
        let ticket = queue.next_ticket;
        queue.next_ticket += 1;
        let queue = self
            .turn
            .wait_while(queue, |q| q.serving != ticket)
            .unwrap_or_else(|p| p.into_inner());
        drop(queue);

        // A transcription cancelled while queued never reaches the model.
        let result = stop
            .check()
            .and_then(|()| model.transcribe(samples, language, stop));

        lock(&self.queue).serving += 1;
        self.turn.notify_all();
        result
    }
}
