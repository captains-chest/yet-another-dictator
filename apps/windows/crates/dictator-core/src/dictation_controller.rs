//! Port of `DictationController.swift` as a pure, deterministic state machine.
//!
//! Hotkey gesture: a quick tap toggles recording on/off; holding the shortcut records until
//! release (push-to-talk).
//!
//! # Protocol for the shell
//!
//! The controller does no I/O and reads no clock. The shell owns the ports (microphone, model,
//! text inserter, field reader) and timers, and drives the controller with
//! [`DictationController::handle`]`(event, now)`, where `now` is a monotonic time (e.g.
//! `Instant::now() - app_start`). Every call returns effects to execute **in order**. After each
//! call the shell may re-read the observable fields ([`state`](DictationController::state),
//! [`level`](DictationController::level), [`last_error`](DictationController::last_error),
//! [`undelivered_transcript`](DictationController::undelivered_transcript),
//! [`can_undo`](DictationController::can_undo)).
//!
//! Start-up: send [`Event::LoadModel`].
//!
//! **Immediate effects** are quick calls on the UI thread; run them and feed the result back
//! right away, before handling any other event:
//!
//! | Effect | Port call | Feed back |
//! |---|---|---|
//! | `Preflight` | `TextInserting::preflight` | `PreflightFinished(result)` |
//! | `StartMic` | `AudioRecording::start` (levels → `Level`) | `MicStarted(result)` |
//! | `StopMic` | `AudioRecording::stop`, samples dropped | — |
//! | `StopMicForTranscription { id }` | `AudioRecording::stop` | `MicStopped { id, samples }` |
//! | `ReadPrecedingText { id, max_length }` | `PrecedingTextReading::text_before_cursor` | `PrecedingTextRead { id, text }` |
//!
//! **Background work** runs off the UI thread; send the result when it finishes:
//!
//! | Effect | Port call | Feed back |
//! |---|---|---|
//! | `PrepareModel` | `Transcribing::prepare` | `ModelPrepared(result)` |
//! | `Transcribe { id, samples, language }` | `Transcribing::transcribe` | `TranscriptionFinished { id, result }` |
//! | `Insert { id, text }` | `TextInserting::insert` | `InsertionFinished { id, result }` |
//! | `DeleteBackward { count }` | `TextInserting::delete_backward` | `DeleteFinished(result)` |
//! | `RetryInsert { text }` | `TextInserting::insert` | `RetryFinished(result)` |
//!
//! Give each `id` (a generation) its own [`StopSignal`](crate::StopSignal); `CancelWork { id }`
//! sets it. Results for an old `id` must still be delivered: the controller ignores them (and a
//! late `InsertionFinished` makes the partly typed text non-undoable).
//!
//! **Timers**: `ScheduleTimer { timer, after }` fires [`Event::TimerFired`]`(timer)` once after
//! `after`; `CancelTimer(timer)` cancels it. Stale timers are ignored, so cancelling is best effort.
//!
//! **Notifications**: `StateChanged`, `ReportError` (show briefly; sent again even for a repeated
//! identical failure), `ClearError` and `Timing` (latency log, no text).
//!
//! User input maps to `HotkeyPressed`/`HotkeyReleased`, `Cancel`, `OtherInput` (any other key,
//! click or app switch), `UndoRequested`, `RetryUndelivered` and `DiscardUndelivered`; recorder
//! callbacks to `Level`, `RecordingFailed` and `RecordingReachedLimit`.

use std::time::Duration;

use crate::dictation_history::DictationHistory;
use crate::dictation_language::DictationLanguage;
use crate::ports::{PortError, PortResult};
use crate::{smart_join, speech_activity, transcript_cleaner};

pub const SAMPLE_RATE_HZ: u32 = 16_000;
/// How much text before the cursor is read for spacing and capitalization.
pub const PRECEDING_TEXT_LENGTH: usize = 64;
/// A press held at least this long is push-to-talk; shorter is a toggle tap.
pub const HOLD_THRESHOLD: Duration = Duration::from_millis(350);
/// Shorter recordings are dropped: nothing was said, and the model would only invent text.
pub const MINIMUM_DURATION: Duration = Duration::from_millis(300);
/// How long the microphone stays open after the user stops (see [`Event::TimerFired`]).
pub const STOP_TAIL: Duration = Duration::from_millis(250);
/// The transcription timeout before the audio's own length is added.
pub const BASE_TRANSCRIPTION_TIMEOUT: Duration = Duration::from_secs(30);

pub const RECORDING_LIMIT_MESSAGE: &str = "Recording reached the 10-minute limit and was stopped.";
pub const TIMEOUT_MESSAGE: &str = "Transcription took too long and was stopped. Please try again.";
const UNDELIVERED_SUFFIX: &str = "Your dictation is kept in the Dictator tray menu.";

/// 30 s plus the length of the audio: far beyond normal (a few seconds), but bounded, so a stuck
/// model can never leave dictation disabled.
pub fn default_transcription_timeout(sample_count: usize) -> Duration {
    BASE_TRANSCRIPTION_TIMEOUT
        + Duration::from_secs_f64(sample_count as f64 / f64::from(SAMPLE_RATE_HZ))
}

/// Identifies one dictation; bumped on cancel/timeout so a late result is ignored.
pub type WorkId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationState {
    LoadingModel,
    Ready,
    Recording,
    Transcribing,
    /// The model could not be loaded; dictation is disabled.
    Unavailable(String),
}

/// How long one dictation took, for latency logging. Contains no text or audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DictationTiming {
    /// Length of the recorded audio.
    pub audio_seconds: f64,
    /// From the moment the user stopped (key release or second tap) to the text being typed.
    pub release_to_typed_seconds: f64,
    /// Time spent in the transcriber alone.
    pub transcription_seconds: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimerKind {
    /// The microphone stays open this long after the user stops.
    StopTail,
    /// Abandons a transcription that takes too long.
    Watchdog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timer {
    pub kind: TimerKind,
    pub id: WorkId,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    LoadModel,
    ModelPrepared(PortResult<()>),
    HotkeyPressed,
    HotkeyReleased,
    PreflightFinished(PortResult<()>),
    MicStarted(PortResult<()>),
    /// Live input loudness, 0...1, from the recorder's level callback.
    Level(f32),
    /// Stops recording, or abandons the running transcription, without typing anything. While
    /// the text is being typed, typing stops between chunks; what was typed stays, but isn't
    /// offered for undo.
    Cancel,
    /// The user typed, clicked or switched apps, so the cursor may have moved since the last
    /// dictation. Send for any input that isn't the dictation shortcut.
    OtherInput,
    /// The microphone stopped on its own (device unplugged, audio error) during recording.
    /// Whatever was captured is dropped, since it may be cut off mid-word.
    RecordingFailed(String),
    /// The capture buffer is full (10 minutes). What was captured is transcribed as usual.
    RecordingReachedLimit,
    TimerFired(Timer),
    MicStopped {
        id: WorkId,
        samples: Vec<f32>,
    },
    TranscriptionFinished {
        id: WorkId,
        result: PortResult<String>,
    },
    PrecedingTextRead {
        id: WorkId,
        text: Option<String>,
    },
    InsertionFinished {
        id: WorkId,
        result: PortResult<()>,
    },
    /// Deletes exactly the characters the last dictation typed, if nothing else happened since.
    UndoRequested,
    DeleteFinished(PortResult<()>),
    /// Types the undelivered transcript again, e.g. after the user fixed the problem.
    RetryUndelivered,
    RetryFinished(PortResult<()>),
    DiscardUndelivered,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    PrepareModel,
    Preflight,
    StartMic,
    StopMic,
    StopMicForTranscription {
        id: WorkId,
    },
    ScheduleTimer {
        timer: Timer,
        after: Duration,
    },
    CancelTimer(Timer),
    /// `language` is a Whisper code, or `None` to detect it.
    Transcribe {
        id: WorkId,
        samples: Vec<f32>,
        language: Option<String>,
    },
    ReadPrecedingText {
        id: WorkId,
        max_length: usize,
    },
    Insert {
        id: WorkId,
        text: String,
    },
    DeleteBackward {
        count: usize,
    },
    RetryInsert {
        text: String,
    },
    CancelWork {
        id: WorkId,
    },
    StateChanged(DictationState),
    ReportError(String),
    ClearError,
    Timing(DictationTiming),
}

/// Tunables; `Default` gives the app's values.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub hold_threshold: Duration,
    pub minimum_duration: Duration,
    pub stop_tail: Duration,
    pub transcription_timeout: fn(usize) -> Duration,
    pub contains_speech: fn(&[f32]) -> bool,
    /// [`PrecedingTextReading::uses_history_for_joining`](crate::PrecedingTextReading) of the reader.
    pub uses_history_for_joining: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hold_threshold: HOLD_THRESHOLD,
            minimum_duration: MINIMUM_DURATION,
            stop_tail: STOP_TAIL,
            transcription_timeout: default_transcription_timeout,
            contains_speech: speech_activity::contains_speech,
            uses_history_for_joining: true,
        }
    }
}

/// The dictation in flight for the current generation.
#[derive(Debug, Default)]
struct Work {
    stopped_at: Duration,
    sample_count: usize,
    transcription_start: Duration,
    transcription_end: Duration,
    /// The cleaned transcript, kept if typing fails.
    text: String,
    /// What was (being) typed: the transcript fitted to the text before the cursor.
    typed: String,
}

pub struct DictationController {
    config: Config,
    state: DictationState,
    level: f32,
    last_error: Option<String>,
    undelivered_transcript: Option<String>,
    language: DictationLanguage,
    history: DictationHistory,
    pressed_at: Option<Duration>,
    /// Between `HotkeyPressed` and `MicStarted`.
    starting: bool,
    /// True while the microphone is still open for the stop tail.
    in_stop_tail: bool,
    watchdog_armed: bool,
    generation: WorkId,
    work: Option<Work>,
    retrying: Option<String>,
    effects: Vec<Effect>,
}

impl Default for DictationController {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

impl DictationController {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: DictationState::LoadingModel,
            level: 0.0,
            last_error: None,
            undelivered_transcript: None,
            language: DictationLanguage::Automatic,
            history: DictationHistory::new(),
            pressed_at: None,
            starting: false,
            in_stop_tail: false,
            watchdog_armed: false,
            generation: 0,
            work: None,
            retrying: None,
            effects: Vec::new(),
        }
    }

    pub fn state(&self) -> &DictationState {
        &self.state
    }

    /// Live input loudness while recording, 0...1.
    pub fn level(&self) -> f32 {
        self.level
    }

    /// Last non-fatal error (e.g. microphone denied), cleared on the next successful start.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// A transcript that could not be typed, kept in memory only so it isn't lost. Replaced by the
    /// next dictation and cleared once typed or discarded.
    pub fn undelivered_transcript(&self) -> Option<&str> {
        self.undelivered_transcript.as_deref()
    }

    /// Language for the next transcriptions.
    pub fn language(&self) -> DictationLanguage {
        self.language
    }

    pub fn set_language(&mut self, language: DictationLanguage) {
        self.language = language;
    }

    /// Whether the last dictation can still be undone: nothing was typed, clicked or switched since.
    pub fn can_undo(&self) -> bool {
        self.state == DictationState::Ready && self.history.last_inserted().is_some()
    }

    pub fn handle(&mut self, event: Event, now: Duration) -> Vec<Effect> {
        match event {
            Event::LoadModel => {
                self.set_state(DictationState::LoadingModel);
                self.emit(Effect::PrepareModel);
            }
            Event::ModelPrepared(Ok(())) => self.set_state(DictationState::Ready),
            Event::ModelPrepared(Err(error)) => {
                self.set_state(DictationState::Unavailable(error.to_string()))
            }
            Event::HotkeyPressed => self.hotkey_pressed(now),
            Event::HotkeyReleased => self.hotkey_released(now),
            Event::PreflightFinished(result) => self.preflight_finished(result),
            Event::MicStarted(result) => self.mic_started(result),
            Event::Level(level) => {
                if self.state == DictationState::Recording || self.in_stop_tail {
                    self.level = level;
                }
            }
            Event::Cancel => self.cancel(),
            Event::OtherInput => self.history.other_input(),
            Event::RecordingFailed(message) => self.recording_failed(message),
            Event::RecordingReachedLimit => {
                if self.state == DictationState::Recording {
                    self.pressed_at = None;
                    self.finish_recording(now);
                    self.report(RECORDING_LIMIT_MESSAGE.to_string());
                }
            }
            Event::TimerFired(timer) => self.timer_fired(timer),
            Event::MicStopped { id, samples } => self.mic_stopped(id, samples, now),
            Event::TranscriptionFinished { id, result } => {
                self.transcription_finished(id, result, now)
            }
            Event::PrecedingTextRead { id, text } => self.preceding_text_read(id, text),
            Event::InsertionFinished { id, result } => self.insertion_finished(id, result, now),
            Event::UndoRequested => {
                if self.state == DictationState::Ready
                    && let Some(count) = self.history.take_undo()
                {
                    self.emit(Effect::DeleteBackward { count });
                }
            }
            Event::DeleteFinished(result) => {
                if let Err(PortError::Failed(message)) = result {
                    self.report(message);
                }
            }
            Event::RetryUndelivered => {
                if self.state == DictationState::Ready
                    && let Some(text) = self.undelivered_transcript.clone()
                {
                    self.retrying = Some(text.clone());
                    self.emit(Effect::RetryInsert { text });
                }
            }
            Event::RetryFinished(result) => self.retry_finished(result),
            Event::DiscardUndelivered => self.undelivered_transcript = None,
        }
        std::mem::take(&mut self.effects)
    }

    fn emit(&mut self, effect: Effect) {
        self.effects.push(effect);
    }

    fn set_state(&mut self, state: DictationState) {
        if self.state != state {
            self.state = state.clone();
            self.emit(Effect::StateChanged(state));
        }
    }

    /// Reported every time, so a repeated identical failure is shown again.
    fn report(&mut self, message: String) {
        self.last_error = Some(message.clone());
        self.emit(Effect::ReportError(message));
    }

    fn clear_error(&mut self) {
        if self.last_error.take().is_some() {
            self.emit(Effect::ClearError);
        }
    }

    fn hotkey_pressed(&mut self, now: Duration) {
        match self.state {
            DictationState::Ready if !self.starting => {
                self.starting = true;
                self.pressed_at = Some(now);
                self.emit(Effect::Preflight);
            }
            DictationState::Recording => {
                self.pressed_at = None;
                self.finish_recording(now);
            }
            _ => {}
        }
    }

    fn hotkey_released(&mut self, now: Duration) {
        let Some(pressed_at) = self.pressed_at.take() else {
            return;
        };
        if self.state == DictationState::Recording
            && now.saturating_sub(pressed_at) >= self.config.hold_threshold
        {
            self.finish_recording(now);
        }
    }

    fn preflight_finished(&mut self, result: PortResult<()>) {
        if !self.starting {
            return;
        }
        match result {
            Ok(()) => self.emit(Effect::StartMic),
            Err(error) => {
                self.starting = false;
                self.pressed_at = None;
                self.report(error.to_string());
            }
        }
    }

    fn mic_started(&mut self, result: PortResult<()>) {
        if !self.starting {
            return;
        }
        self.starting = false;
        match result {
            Ok(()) if self.state != DictationState::Ready => self.emit(Effect::StopMic),
            Ok(()) => {
                self.clear_error();
                self.set_state(DictationState::Recording);
            }
            Err(error) => {
                self.pressed_at = None;
                self.report(format!("Could not start microphone: {error}"));
            }
        }
    }

    fn cancel(&mut self) {
        match self.state {
            DictationState::Recording => {
                self.pressed_at = None;
                self.emit(Effect::StopMic);
                self.level = 0.0;
                self.set_state(DictationState::Ready);
            }
            DictationState::Transcribing => {
                self.close_stop_tail();
                self.abandon_transcription();
                self.set_state(DictationState::Ready);
            }
            _ => {}
        }
    }

    fn recording_failed(&mut self, message: String) {
        if self.state != DictationState::Recording {
            return;
        }
        self.pressed_at = None;
        self.emit(Effect::StopMic);
        self.level = 0.0;
        self.set_state(DictationState::Ready);
        self.report(message);
    }

    fn finish_recording(&mut self, now: Duration) {
        self.set_state(DictationState::Transcribing);
        self.generation += 1;
        let id = self.generation;
        self.in_stop_tail = true;
        self.work = Some(Work {
            stopped_at: now,
            ..Work::default()
        });
        // People release the shortcut on (not after) their last syllable, and the newest audio
        // buffer is still in flight; cutting there clips the last word.
        if self.config.stop_tail.is_zero() {
            self.end_stop_tail(id);
        } else {
            let timer = Timer {
                kind: TimerKind::StopTail,
                id,
            };
            self.emit(Effect::ScheduleTimer {
                timer,
                after: self.config.stop_tail,
            });
        }
    }

    /// Closes the microphone if it's still open for the stop tail, dropping what it recorded.
    fn close_stop_tail(&mut self) {
        if !self.in_stop_tail {
            return;
        }
        self.in_stop_tail = false;
        self.level = 0.0;
        self.emit(Effect::CancelTimer(Timer {
            kind: TimerKind::StopTail,
            id: self.generation,
        }));
        self.emit(Effect::StopMic);
    }

    fn end_stop_tail(&mut self, id: WorkId) {
        if !self.in_stop_tail {
            return;
        }
        self.in_stop_tail = false;
        self.level = 0.0;
        self.emit(Effect::StopMicForTranscription { id });
    }

    fn timer_fired(&mut self, timer: Timer) {
        if timer.id != self.generation {
            return;
        }
        match timer.kind {
            TimerKind::StopTail => self.end_stop_tail(timer.id),
            TimerKind::Watchdog => {
                if !self.watchdog_armed || self.state != DictationState::Transcribing {
                    return;
                }
                self.watchdog_armed = false;
                self.abandon_transcription();
                self.report(TIMEOUT_MESSAGE.to_string());
                self.set_state(DictationState::Ready);
            }
        }
    }

    /// Cancels the running transcription and makes sure its result, if it still arrives, is dropped.
    fn abandon_transcription(&mut self) {
        let id = self.generation;
        self.generation += 1;
        self.emit(Effect::CancelWork { id });
        self.disarm_watchdog(id);
        self.work = None;
    }

    fn disarm_watchdog(&mut self, id: WorkId) {
        if self.watchdog_armed {
            self.watchdog_armed = false;
            self.emit(Effect::CancelTimer(Timer {
                kind: TimerKind::Watchdog,
                id,
            }));
        }
    }

    /// The current transcription is done (typed, empty or failed): back to Ready.
    fn finish_transcription(&mut self) {
        self.work = None;
        self.set_state(DictationState::Ready);
    }

    fn current_work(&mut self, id: WorkId) -> Option<&mut Work> {
        if id == self.generation && self.state == DictationState::Transcribing {
            self.work.as_mut()
        } else {
            None
        }
    }

    fn mic_stopped(&mut self, id: WorkId, samples: Vec<f32>, now: Duration) {
        let minimum_samples =
            (self.config.minimum_duration.as_secs_f64() * f64::from(SAMPLE_RATE_HZ)) as usize;
        let contains_speech = self.config.contains_speech;
        let timeout = (self.config.transcription_timeout)(samples.len());
        let Some(work) = self.current_work(id) else {
            return;
        };
        // Too short or silent: nothing was said, and the model would only invent text.
        if samples.len() < minimum_samples || !contains_speech(&samples) {
            self.finish_transcription();
            return;
        }
        work.sample_count = samples.len();
        work.transcription_start = now;
        self.watchdog_armed = true;
        self.emit(Effect::ScheduleTimer {
            timer: Timer {
                kind: TimerKind::Watchdog,
                id,
            },
            after: timeout,
        });
        let language = self.language.whisper_code().map(String::from);
        self.emit(Effect::Transcribe {
            id,
            samples,
            language,
        });
    }

    fn transcription_finished(&mut self, id: WorkId, result: PortResult<String>, now: Duration) {
        // Cancelled or timed out meanwhile: the user has moved on, so type nothing.
        let Some(work) = self.current_work(id) else {
            return;
        };
        work.transcription_end = now;
        self.disarm_watchdog(id);
        let text = match result {
            Ok(raw) => transcript_cleaner::clean(&raw),
            Err(error) => {
                self.finish_transcription();
                self.report(error.to_string());
                return;
            }
        };
        if text.is_empty() {
            self.finish_transcription();
            return;
        }
        if let Some(work) = self.work.as_mut() {
            work.text = text;
        }
        self.emit(Effect::ReadPrecedingText {
            id,
            max_length: PRECEDING_TEXT_LENGTH,
        });
    }

    fn preceding_text_read(&mut self, id: WorkId, field_text: Option<String>) {
        let uses_history = self.config.uses_history_for_joining;
        let last_inserted = self.history.last_inserted().map(String::from);
        let Some(work) = self.current_work(id) else {
            return;
        };
        // The field's own text when it can be read; otherwise our previous dictation, as long as
        // nothing else was typed since; otherwise unknown, and the text is typed as transcribed.
        // An empty field reading next to a dictation we just typed means the app reports its text
        // wrongly (some web views do), so the history is trusted over it — unless the reader opts
        // out, which means another layer will join the transcript itself.
        let preceding = match field_text {
            Some(text) if !text.is_empty() => Some(text),
            field_text if uses_history => last_inserted.or(field_text),
            _ => None,
        };
        work.typed = smart_join::adjust(&work.text, preceding.as_deref());
        let text = work.typed.clone();
        // Typing stays in Transcribing, so Cancel can still stop it between chunks.
        self.emit(Effect::Insert { id, text });
    }

    fn insertion_finished(&mut self, id: WorkId, result: PortResult<()>, now: Duration) {
        // Cancelled while typing: whatever got typed is only part of the dictation, so it can't
        // be undone safely.
        if self.current_work(id).is_none() {
            self.history.other_input();
            return;
        }
        let Some(work) = self.work.take() else { return };
        self.finish_transcription();
        match result {
            Ok(()) => {
                self.history.did_insert(&work.typed);
                self.undelivered_transcript = None;
                self.emit(Effect::Timing(DictationTiming {
                    audio_seconds: work.sample_count as f64 / f64::from(SAMPLE_RATE_HZ),
                    release_to_typed_seconds: now.saturating_sub(work.stopped_at).as_secs_f64(),
                    transcription_seconds: work
                        .transcription_end
                        .saturating_sub(work.transcription_start)
                        .as_secs_f64(),
                }));
            }
            Err(error) => {
                self.history.other_input();
                self.undelivered_transcript = Some(work.text);
                self.report(format!("{error} {UNDELIVERED_SUFFIX}"));
            }
        }
    }

    fn retry_finished(&mut self, result: PortResult<()>) {
        let Some(text) = self.retrying.take() else {
            return;
        };
        match result {
            Ok(()) => {
                self.history.did_insert(&text);
                self.undelivered_transcript = None;
                self.clear_error();
            }
            // Partly typed at most: not undoable, and the full text stays in the menu.
            Err(PortError::Cancelled) => self.history.other_input(),
            Err(PortError::Failed(message)) => {
                self.history.other_input();
                self.report(message);
            }
        }
    }
}
