//! Port of DictationControllerTests.swift. The controller is pure, so a small fake shell executes
//! its effects: immediate effects resolve at once, background work stays pending until the test
//! completes it (`settle`/`wait_until_ready`), and time only moves when a test moves it.

use std::collections::HashSet;
use std::time::Duration;

use dictator_core::dictation_controller::{
    Config, DictationController, DictationState, DictationTiming, Effect, Event,
    RECORDING_LIMIT_MESSAGE, Timer, TimerKind, WorkId, default_transcription_timeout,
};
use dictator_core::dictation_language::DictationLanguage;
use dictator_core::{PortError, PortResult};

fn boom() -> PortError {
    PortError::Failed("Boom".into())
}

struct PendingTranscription {
    id: WorkId,
}

struct Shell {
    controller: DictationController,
    now: Duration,
    // Recorder
    samples_to_return: Vec<f32>,
    start_error: Option<PortError>,
    is_recording: bool,
    start_count: usize,
    // Transcriber
    result: String,
    prepare_error: Option<PortError>,
    received: Vec<usize>,
    languages: Vec<Option<String>>,
    /// Holds transcriptions until released, like a slow model.
    hold_transcriptions: bool,
    /// Whether a held transcription stops when cancelled (`false`: the result arrives anyway).
    cooperative: bool,
    saw_cancellation: bool,
    pending_transcriptions: Vec<PendingTranscription>,
    // Inserter
    preflight_error: Option<PortError>,
    insert_error: Option<PortError>,
    /// Holds inserts until released, like a long text typed in chunks.
    hold_inserts: bool,
    pending_inserts: Vec<(WorkId, String)>,
    inserted: Vec<String>,
    deleted: Vec<usize>,
    // Preceding text reader
    field_text: Option<String>,
    requested_lengths: Vec<usize>,
    // Shell bookkeeping
    cancelled: HashSet<WorkId>,
    timers: Vec<(Timer, Duration)>,
    errors: Vec<String>,
    timings: Vec<DictationTiming>,
}

impl Shell {
    fn with_config(config: Config) -> Self {
        Self {
            controller: DictationController::new(config),
            now: Duration::from_secs(100),
            samples_to_return: vec![0.1; 16_000],
            start_error: None,
            is_recording: false,
            start_count: 0,
            result: "hello world".into(),
            prepare_error: None,
            received: Vec::new(),
            languages: Vec::new(),
            hold_transcriptions: false,
            cooperative: true,
            saw_cancellation: false,
            pending_transcriptions: Vec::new(),
            preflight_error: None,
            insert_error: None,
            hold_inserts: false,
            pending_inserts: Vec::new(),
            inserted: Vec::new(),
            deleted: Vec::new(),
            field_text: None,
            requested_lengths: Vec::new(),
            cancelled: HashSet::new(),
            timers: Vec::new(),
            errors: Vec::new(),
            timings: Vec::new(),
        }
    }

    /// Like the Swift tests: no stop tail, so the microphone closes as soon as recording stops.
    fn config() -> Config {
        Config {
            stop_tail: Duration::ZERO,
            ..Config::default()
        }
    }

    fn new() -> Self {
        Self::with_config(Self::config())
    }

    fn loaded(mut self) -> Self {
        self.send(Event::LoadModel);
        self
    }

    fn state(&self) -> &DictationState {
        self.controller.state()
    }

    fn advance(&mut self, by: Duration) {
        self.now += by;
    }

    fn press(&mut self) {
        self.send(Event::HotkeyPressed);
    }

    fn release(&mut self) {
        self.send(Event::HotkeyReleased);
    }

    fn send(&mut self, event: Event) {
        let effects = self.controller.handle(event, self.now);
        for effect in effects {
            self.run(effect);
        }
    }

    fn run(&mut self, effect: Effect) {
        match effect {
            Effect::PrepareModel => {
                let result = self.prepare_error.clone().map_or(Ok(()), Err);
                self.send(Event::ModelPrepared(result));
            }
            Effect::Preflight => {
                let result = self.preflight_error.clone().map_or(Ok(()), Err);
                self.send(Event::PreflightFinished(result));
            }
            Effect::StartMic => {
                let result = match self.start_error.clone() {
                    Some(error) => Err(error),
                    None => {
                        self.is_recording = true;
                        self.start_count += 1;
                        Ok(())
                    }
                };
                self.send(Event::MicStarted(result));
            }
            Effect::StopMic => self.is_recording = false,
            Effect::StopMicForTranscription { id } => {
                self.is_recording = false;
                let samples = self.samples_to_return.clone();
                self.send(Event::MicStopped { id, samples });
            }
            Effect::ScheduleTimer { timer, after } => self.timers.push((timer, self.now + after)),
            Effect::CancelTimer(timer) => self.timers.retain(|(t, _)| *t != timer),
            Effect::Transcribe {
                id,
                samples,
                language,
            } => {
                self.received.push(samples.len());
                self.languages.push(language);
                self.pending_transcriptions
                    .push(PendingTranscription { id });
                if !self.hold_transcriptions {
                    self.finish_transcriptions();
                }
            }
            Effect::ReadPrecedingText { id, max_length } => {
                self.requested_lengths.push(max_length);
                let text = self.field_text.clone();
                self.send(Event::PrecedingTextRead { id, text });
            }
            Effect::Insert { id, text } => {
                self.pending_inserts.push((id, text));
                if !self.hold_inserts {
                    self.finish_inserts();
                }
            }
            Effect::DeleteBackward { count } => {
                let result = self.insert_result();
                if result.is_ok() {
                    self.deleted.push(count);
                }
                self.send(Event::DeleteFinished(result));
            }
            Effect::RetryInsert { text } => {
                let result = self.insert_result();
                if result.is_ok() {
                    self.inserted.push(text);
                }
                self.send(Event::RetryFinished(result));
            }
            Effect::CancelWork { id } => {
                self.cancelled.insert(id);
            }
            Effect::StateChanged(_) | Effect::ClearError => {}
            Effect::ReportError(message) => self.errors.push(message),
            Effect::Timing(timing) => self.timings.push(timing),
        }
    }

    fn insert_result(&self) -> PortResult<()> {
        self.insert_error.clone().map_or(Ok(()), Err)
    }

    fn finish_transcriptions(&mut self) {
        for PendingTranscription { id } in std::mem::take(&mut self.pending_transcriptions) {
            let result = if self.cancelled.contains(&id) && self.cooperative {
                self.saw_cancellation = true;
                Err(PortError::Cancelled)
            } else {
                Ok(self.result.clone())
            };
            self.send(Event::TranscriptionFinished { id, result });
        }
    }

    fn finish_inserts(&mut self) {
        for (id, text) in std::mem::take(&mut self.pending_inserts) {
            let result = if self.cancelled.contains(&id) {
                Err(PortError::Cancelled)
            } else {
                self.insert_result()
            };
            if result.is_ok() {
                self.inserted.push(text);
            }
            self.send(Event::InsertionFinished { id, result });
        }
    }

    /// Lets held work finish and due timers fire.
    fn settle(&mut self) {
        self.hold_transcriptions = false;
        self.hold_inserts = false;
        self.finish_transcriptions();
        self.finish_inserts();
    }

    /// Fires the earliest pending timer, moving the clock to it.
    fn fire_next_timer(&mut self) -> bool {
        let Some(index) = (0..self.timers.len()).min_by_key(|&i| self.timers[i].1) else {
            return false;
        };
        let (timer, due) = self.timers.remove(index);
        self.now = self.now.max(due);
        self.send(Event::TimerFired(timer));
        true
    }

    /// Runs work that isn't held, then timers, until Ready or nothing is left to do.
    fn wait_until_ready(&mut self) {
        loop {
            if !self.hold_transcriptions {
                self.finish_transcriptions();
            }
            if !self.hold_inserts {
                self.finish_inserts();
            }
            if *self.state() == DictationState::Ready || !self.fire_next_timer() {
                return;
            }
        }
    }

    fn dictate(&mut self, result: &str) {
        self.result = result.into();
        self.press();
        self.press();
        self.wait_until_ready();
    }
}

#[test]
fn loads_model_then_becomes_ready() {
    let mut shell = Shell::new();
    assert_eq!(*shell.state(), DictationState::LoadingModel);
    shell.send(Event::LoadModel);
    assert_eq!(*shell.state(), DictationState::Ready);
}

#[test]
fn failed_model_load_disables_dictation() {
    let mut shell = Shell::new();
    shell.prepare_error = Some(boom());
    let mut shell = shell.loaded();
    assert!(
        matches!(shell.state(), DictationState::Unavailable(_)),
        "{:?}",
        shell.state()
    );
    shell.press();
    assert!(!shell.is_recording);
}

#[test]
fn tap_toggles_recording_and_inserts_text() {
    let mut shell = Shell::new().loaded();

    shell.press();
    shell.advance(Duration::from_millis(100));
    shell.release();
    assert_eq!(*shell.state(), DictationState::Recording);

    shell.hold_transcriptions = true;
    shell.press();
    shell.release();
    assert_eq!(*shell.state(), DictationState::Transcribing);

    shell.settle();
    assert_eq!(*shell.state(), DictationState::Ready);
    assert_eq!(shell.inserted, ["hello world"]);
    assert_eq!(shell.received.len(), 1);
}

#[test]
fn hold_is_push_to_talk() {
    let mut shell = Shell::new().loaded();
    shell.hold_transcriptions = true;

    shell.press();
    shell.advance(Duration::from_secs(1));
    shell.release();

    assert_eq!(*shell.state(), DictationState::Transcribing);
    shell.settle();
    assert_eq!(shell.inserted, ["hello world"]);
}

#[test]
fn too_short_recording_is_discarded() {
    let mut shell = Shell::new().loaded();
    shell.samples_to_return = vec![0.0; 100];

    shell.press();
    shell.press();
    shell.wait_until_ready();

    assert_eq!(*shell.state(), DictationState::Ready);
    assert!(!shell.is_recording);
    assert!(shell.received.is_empty());
}

#[test]
fn cancel_discards_audio() {
    let mut shell = Shell::new().loaded();

    shell.press();
    shell.send(Event::Cancel);

    assert_eq!(*shell.state(), DictationState::Ready);
    assert!(!shell.is_recording);
    assert!(shell.received.is_empty());
    assert!(shell.inserted.is_empty());
}

#[test]
fn keeps_recording_for_the_tail_after_stop() {
    let mut shell = Shell::with_config(Config {
        stop_tail: Duration::from_millis(50),
        ..Shell::config()
    })
    .loaded();

    shell.press();
    shell.press();
    assert_eq!(*shell.state(), DictationState::Transcribing);
    assert!(
        shell.is_recording,
        "the microphone must stay open during the tail"
    );

    shell.wait_until_ready();
    assert!(!shell.is_recording);
    assert_eq!(shell.inserted, ["hello world"]);
}

#[test]
fn cancel_during_transcription_types_nothing() {
    let mut shell = Shell::new().loaded();
    shell.hold_transcriptions = true;
    shell.cooperative = false;

    shell.press();
    shell.press();
    assert_eq!(*shell.state(), DictationState::Transcribing);

    shell.send(Event::Cancel);
    assert_eq!(*shell.state(), DictationState::Ready);

    shell.settle(); // the late result arrives meanwhile
    assert!(shell.inserted.is_empty());
    assert_eq!(shell.controller.last_error(), None);
    assert_eq!(*shell.state(), DictationState::Ready);
}

#[test]
fn cancel_stops_a_cooperative_transcriber() {
    let mut shell = Shell::new().loaded();
    shell.hold_transcriptions = true;

    shell.press();
    shell.press();
    assert_eq!(shell.received.len(), 1, "the transcriber is running");
    shell.send(Event::Cancel);

    shell.settle();
    assert!(shell.saw_cancellation);
    assert_eq!(shell.controller.last_error(), None);
}

#[test]
fn stuck_transcription_times_out() {
    fn timeout(_: usize) -> Duration {
        Duration::from_millis(50)
    }
    let mut shell = Shell::with_config(Config {
        transcription_timeout: timeout,
        ..Shell::config()
    })
    .loaded();
    shell.hold_transcriptions = true;

    shell.press();
    shell.press();
    assert!(
        shell
            .timers
            .iter()
            .any(|(t, due)| t.kind == TimerKind::Watchdog
                && *due == shell.now + Duration::from_millis(50))
    );
    shell.wait_until_ready();

    assert_eq!(*shell.state(), DictationState::Ready);
    assert!(
        shell
            .controller
            .last_error()
            .is_some_and(|e| e.contains("too long"))
    );
    assert!(shell.inserted.is_empty());
}

#[test]
fn dictation_works_again_after_cancel() {
    let mut shell = Shell::new().loaded();
    shell.hold_transcriptions = true;
    shell.cooperative = false;
    shell.press();
    shell.press();
    shell.send(Event::Cancel);

    shell.hold_transcriptions = false;
    shell.press();
    shell.press();
    shell.wait_until_ready();
    shell.settle(); // let the abandoned one finish too

    assert_eq!(shell.inserted, ["hello world"]);
}

#[test]
fn default_timeout_grows_with_audio_length() {
    assert_eq!(default_transcription_timeout(0), Duration::from_secs(30));
    assert_eq!(
        default_transcription_timeout(16_000 * 60),
        Duration::from_secs(90)
    );
}

#[test]
fn silent_recording_is_not_transcribed() {
    let mut shell = Shell::new().loaded();
    shell.samples_to_return = vec![0.0; 32_000];

    shell.press();
    shell.press();
    shell.wait_until_ready();

    assert_eq!(*shell.state(), DictationState::Ready);
    assert!(shell.received.is_empty());
    assert!(shell.inserted.is_empty());
}

#[test]
fn microphone_failure_during_recording_returns_to_ready_and_reports() {
    let mut shell = Shell::new().loaded();
    shell.press();
    assert_eq!(*shell.state(), DictationState::Recording);

    shell.send(Event::RecordingFailed(
        "The microphone was disconnected.".into(),
    ));

    assert_eq!(*shell.state(), DictationState::Ready);
    assert!(!shell.is_recording);
    assert_eq!(
        shell.controller.last_error(),
        Some("The microphone was disconnected.")
    );
    assert!(shell.received.is_empty());
}

#[test]
fn microphone_failure_outside_recording_is_ignored() {
    let mut shell = Shell::new().loaded();
    shell.send(Event::RecordingFailed(
        "The microphone was disconnected.".into(),
    ));
    assert_eq!(*shell.state(), DictationState::Ready);
    assert_eq!(shell.controller.last_error(), None);
}

#[test]
fn passes_the_chosen_language_to_the_transcriber() {
    let mut shell = Shell::new().loaded();

    shell.press();
    shell.press();
    shell.wait_until_ready();
    shell.controller.set_language(DictationLanguage::English);
    shell.press();
    shell.press();
    shell.wait_until_ready();

    assert_eq!(shell.languages, [None, Some("en".to_string())]);
}

#[test]
fn consecutive_dictations_are_spaced_and_capitalized() {
    let mut shell = Shell::new().loaded();
    shell.dictate("Hello world.");
    shell.dictate("and more");
    shell.dictate("So that's it.");
    assert_eq!(
        shell.inserted,
        ["Hello world.", " And more", " so that's it."]
    );
}

#[test]
fn other_input_makes_the_context_unknown() {
    let mut shell = Shell::new().loaded();
    shell.dictate("Hello world.");
    shell.send(Event::OtherInput);
    shell.dictate("and more.");
    assert_eq!(shell.inserted, ["Hello world.", "and more."]);
}

#[test]
fn readable_field_text_wins_over_history() {
    let mut shell = Shell::new().loaded();
    shell.dictate("Hello world.");
    shell.field_text = Some("Dear Sir,".into());
    shell.dictate("Thanks for writing.");
    assert_eq!(
        shell.inserted.last().map(String::as_str),
        Some(" thanks for writing.")
    );
}

#[test]
fn empty_field_reading_does_not_override_history() {
    let mut shell = Shell::new().loaded();
    shell.field_text = Some(String::new());
    shell.dictate("hello world.");
    shell.dictate("and more.");
    assert_eq!(shell.inserted, ["Hello world.", " And more."]);
}

#[test]
fn unreadable_field_falls_back_to_history() {
    let mut shell = Shell::new().loaded();
    shell.field_text = None;
    shell.dictate("Hello world.");
    shell.dictate("and more");
    assert_eq!(shell.inserted, ["Hello world.", " And more"]);
    assert_eq!(shell.requested_lengths, [64, 64]);
}

#[test]
fn unreadable_field_without_history_types_as_transcribed() {
    let mut shell = Shell::new().loaded();
    shell.field_text = None;
    shell.dictate("hello world");
    assert_eq!(shell.inserted, ["hello world"]);
}

#[test]
fn opting_out_of_history_leaves_consecutive_dictations_raw() {
    let mut shell = Shell::with_config(Config {
        uses_history_for_joining: false,
        ..Shell::config()
    })
    .loaded();
    shell.field_text = None;
    shell.dictate("Hello world.");
    shell.dictate("and more");
    assert_eq!(shell.inserted, ["Hello world.", "and more"]);
}

#[test]
fn reports_timing_from_release_to_typed_text() {
    let mut shell = Shell::new().loaded();
    shell.hold_transcriptions = true;

    shell.press();
    shell.advance(Duration::from_secs(2));
    shell.release(); // hold: stops here, at t = 102
    shell.advance(Duration::from_millis(500)); // transcription and typing take 0.5 s
    shell.settle();

    assert_eq!(shell.timings.len(), 1);
    assert_eq!(shell.timings[0].audio_seconds, 1.0); // the fake recorder returns 16 000 samples
    assert_eq!(shell.timings[0].release_to_typed_seconds, 0.5);
    assert_eq!(shell.timings[0].transcription_seconds, 0.5);
}

#[test]
fn no_timing_when_nothing_is_typed() {
    let mut shell = Shell::new().loaded();
    shell.result = "[BLANK_AUDIO]".into();

    shell.press();
    shell.press();
    shell.wait_until_ready();

    assert!(shell.timings.is_empty());
}

#[test]
fn undo_deletes_exactly_what_was_typed() {
    let mut shell = Shell::new().loaded();
    shell.dictate("Hello world.");
    shell.dictate("and more 👋🏽.");
    assert!(shell.controller.can_undo());

    shell.send(Event::UndoRequested);

    assert_eq!(shell.deleted, [12]); // " And more 👋🏽.": the emoji with skin tone is one character
    assert!(!shell.controller.can_undo());
    shell.send(Event::UndoRequested); // only once
    assert_eq!(shell.deleted.len(), 1);
}

#[test]
fn undo_is_unavailable_after_other_input() {
    let mut shell = Shell::new().loaded();
    shell.dictate("Hello world.");
    shell.send(Event::OtherInput);
    assert!(!shell.controller.can_undo());
    shell.send(Event::UndoRequested);
    assert!(shell.deleted.is_empty());
}

#[test]
fn undo_is_unavailable_before_any_dictation_and_while_recording() {
    let mut shell = Shell::new().loaded();
    assert!(!shell.controller.can_undo());
    shell.dictate("Hello.");
    shell.press();
    assert!(!shell.controller.can_undo());
    shell.send(Event::UndoRequested);
    assert!(shell.deleted.is_empty());
}

#[test]
fn reaching_the_limit_still_transcribes_and_types() {
    let mut shell = Shell::new().loaded();
    shell.press();
    assert_eq!(*shell.state(), DictationState::Recording);

    shell.hold_transcriptions = true;
    shell.send(Event::RecordingReachedLimit);
    assert_eq!(shell.controller.last_error(), Some(RECORDING_LIMIT_MESSAGE));
    assert_eq!(
        shell.controller.last_error(),
        Some("Recording reached the 10-minute limit and was stopped.")
    );
    shell.settle();

    assert_eq!(*shell.state(), DictationState::Ready);
    assert!(!shell.is_recording);
    assert_eq!(shell.received.len(), 1);
    assert_eq!(shell.inserted, ["hello world"]);
    assert_eq!(shell.controller.last_error(), Some(RECORDING_LIMIT_MESSAGE));
}

#[test]
fn reaching_the_limit_after_hold_release_is_ignored() {
    let mut shell = Shell::new().loaded();
    shell.hold_transcriptions = true;
    shell.press();
    shell.advance(Duration::from_secs(2));
    shell.send(Event::RecordingReachedLimit);
    shell.release(); // the hold already ended by the limit: no second stop
    shell.settle();
    assert_eq!(shell.received.len(), 1);
    assert_eq!(shell.inserted, ["hello world"]);
}

#[test]
fn reaching_the_limit_is_a_no_op_when_ready() {
    let mut shell = Shell::new().loaded();
    shell.send(Event::RecordingReachedLimit);
    assert_eq!(*shell.state(), DictationState::Ready);
    assert_eq!(shell.controller.last_error(), None);
    assert!(shell.received.is_empty());
}

#[test]
fn reaching_the_limit_is_a_no_op_while_transcribing() {
    let mut shell = Shell::new().loaded();
    shell.hold_transcriptions = true;
    shell.press();
    shell.press();
    assert_eq!(*shell.state(), DictationState::Transcribing);
    shell.send(Event::RecordingReachedLimit);
    assert_eq!(shell.controller.last_error(), None);
    shell.settle();
    assert_eq!(shell.received.len(), 1);
    assert_eq!(shell.inserted.len(), 1);
}

/// Starts a dictation whose typing is still in progress.
fn start_typing(shell: &mut Shell, result: &str) -> WorkId {
    shell.result = result.into();
    shell.hold_inserts = true;
    shell.press();
    shell.press();
    assert_eq!(shell.pending_inserts.len(), 1, "typing has started");
    shell.pending_inserts[0].0
}

#[test]
fn cancel_while_typing_stops_posting_and_ends_ready() {
    let mut shell = Shell::new().loaded();
    let id = start_typing(
        &mut shell,
        "This is a long dictation that takes a while to type.",
    );
    assert_eq!(*shell.state(), DictationState::Transcribing);

    shell.send(Event::Cancel);
    assert_eq!(*shell.state(), DictationState::Ready);
    assert!(
        shell.cancelled.contains(&id),
        "the inserter is told to stop posting"
    );
    shell.settle();

    assert!(shell.inserted.is_empty());
    assert_eq!(*shell.state(), DictationState::Ready);
    assert_eq!(shell.controller.last_error(), None);
    assert_eq!(shell.controller.undelivered_transcript(), None);
    assert!(!shell.controller.can_undo()); // a partial insert is never undoable
}

#[test]
fn cancel_while_typing_reports_no_timing_and_doesnt_join_the_next_dictation() {
    let mut shell = Shell::new().loaded();
    start_typing(&mut shell, "First dictation that is cancelled.");
    shell.send(Event::Cancel);
    shell.settle();

    shell.dictate("next one.");
    assert_eq!(shell.timings.len(), 1); // only the second dictation
    assert_eq!(shell.inserted, ["next one."]); // no smart join with the partial text
}

#[test]
fn a_new_dictation_started_right_after_cancelling_typing_is_not_stopped() {
    let mut shell = Shell::new().loaded();
    start_typing(&mut shell, "A long dictation being typed slowly.");
    shell.send(Event::Cancel);
    shell.press();
    shell.settle(); // the cancelled insert finishes meanwhile
    assert_eq!(*shell.state(), DictationState::Recording); // the old work didn't reset it to Ready
    assert!(shell.is_recording);
}

#[test]
fn empty_transcript_inserts_nothing() {
    let mut shell = Shell::new().loaded();
    shell.result = " [BLANK_AUDIO] ".into();

    shell.press();
    shell.press();
    shell.wait_until_ready();

    assert!(shell.inserted.is_empty());
}

#[test]
fn microphone_failure_surfaces_error_and_stays_ready() {
    let mut shell = Shell::new().loaded();
    shell.start_error = Some(boom());

    shell.press();

    assert_eq!(*shell.state(), DictationState::Ready);
    assert_eq!(
        shell.controller.last_error(),
        Some("Could not start microphone: Boom")
    );
}

#[test]
fn refuses_to_record_when_text_cannot_be_inserted() {
    let mut shell = Shell::new().loaded();
    shell.preflight_error = Some(boom());

    shell.press();
    shell.release();

    assert_eq!(*shell.state(), DictationState::Ready);
    assert!(!shell.is_recording);
    assert!(shell.controller.last_error().is_some());
    assert_eq!(shell.errors.len(), 1);

    shell.press();
    assert_eq!(shell.errors.len(), 2, "a repeated failure is shown again");
}

#[test]
fn failed_insert_keeps_transcript_for_retry() {
    let mut shell = Shell::new().loaded();
    shell.insert_error = Some(boom());

    shell.press();
    shell.press();
    shell.wait_until_ready();

    assert_eq!(
        shell.controller.undelivered_transcript(),
        Some("hello world")
    );
    assert_eq!(
        shell.controller.last_error(),
        Some("Boom Your dictation is kept in the Dictator tray menu.")
    );

    shell.send(Event::RetryUndelivered);
    assert_eq!(
        shell.controller.undelivered_transcript(),
        Some("hello world"),
        "still failing, still kept"
    );

    shell.insert_error = None;
    shell.send(Event::RetryUndelivered);
    assert_eq!(shell.inserted, ["hello world"]);
    assert_eq!(shell.controller.undelivered_transcript(), None);
    assert_eq!(shell.controller.last_error(), None);
}

#[test]
fn next_successful_dictation_replaces_undelivered_transcript() {
    let mut shell = Shell::new().loaded();
    shell.insert_error = Some(boom());
    shell.press();
    shell.press();
    shell.wait_until_ready();
    assert!(shell.controller.undelivered_transcript().is_some());

    shell.insert_error = None;
    shell.press();
    shell.press();
    shell.wait_until_ready();

    assert_eq!(shell.controller.undelivered_transcript(), None);
    assert_eq!(shell.inserted, ["hello world"]);
}

#[test]
fn discard_forgets_undelivered_transcript() {
    let mut shell = Shell::new().loaded();
    shell.insert_error = Some(boom());
    shell.press();
    shell.press();
    shell.wait_until_ready();

    shell.send(Event::DiscardUndelivered);

    assert_eq!(shell.controller.undelivered_transcript(), None);
}

#[test]
fn hotkey_ignored_while_transcribing() {
    let mut shell = Shell::new().loaded();
    shell.hold_transcriptions = true;
    shell.press();
    shell.press();
    assert_eq!(*shell.state(), DictationState::Transcribing);
    assert!(!shell.is_recording);

    shell.press();
    assert_eq!(shell.start_count, 1);
    assert_eq!(*shell.state(), DictationState::Transcribing);
    shell.settle();
    assert_eq!(shell.received.len(), 1);
}

#[test]
fn cancel_during_stop_tail_closes_the_microphone_at_once() {
    let mut shell = Shell::with_config(Config {
        stop_tail: Duration::from_millis(200),
        ..Shell::config()
    })
    .loaded();

    shell.press();
    shell.press();
    assert!(shell.is_recording); // still in the tail
    let (tail, _) = shell.timers[0];
    shell.send(Event::Cancel);
    assert!(!shell.is_recording);
    assert_eq!(*shell.state(), DictationState::Ready);

    // A new dictation right away isn't cut off by the abandoned tail.
    shell.press();
    shell.advance(Duration::from_millis(300));
    shell.send(Event::TimerFired(tail)); // even if its cancellation lost the race
    while shell.fire_next_timer() {}
    assert!(shell.is_recording);
    assert_eq!(*shell.state(), DictationState::Recording);
    assert!(shell.received.is_empty());
}
