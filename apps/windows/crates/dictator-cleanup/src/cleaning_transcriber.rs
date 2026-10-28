//! Transcribes with the speech model, then, if cleanup is on and its model is ready, rewrites the
//! transcript with a [`TranscriptCleaning`] model (ADR 0011).
//!
//! Cleanup never costs the user a dictation: if it is off, still downloading, fails or takes too
//! long, the raw transcript is returned instead. Only cancellation of the dictation itself is
//! passed on. The controller types whatever this returns and remembers exactly that for Undo, so
//! Undo removes the cleaned text when the cleaned text was typed.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use dictator_core::transcript_cleaner;
use dictator_core::{
    CleanupStyle, PortError, PortResult, StopSignal, Transcribing, TranscriptCleaning,
};

/// What happened to the last transcript. Contains no text, so it may be logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Cleanup is off, its model isn't ready yet, or there was nothing to clean.
    Raw,
    Cleaned,
    /// Cleanup failed; the raw transcript was used.
    Failed(String),
    /// Cleanup took longer than its timeout; the raw transcript was used.
    TimedOut,
}

type TimeoutFn = dyn Fn(&str) -> Duration + Send + Sync;
type OnOutcomeFn = dyn Fn(&Outcome, Duration) + Send + Sync;

#[derive(Default)]
struct State {
    cleaner: Option<Arc<dyn TranscriptCleaning>>,
    style: CleanupStyle,
    last_raw_transcript: Option<String>,
}

pub struct CleaningTranscriber {
    base: Arc<dyn Transcribing>,
    timeout: Box<TimeoutFn>,
    /// Called after each transcript with what cleanup did and how long it took. Never with text.
    on_outcome: Box<OnOutcomeFn>,
    state: Mutex<State>,
}

/// How often a waiting cleanup checks whether the dictation was cancelled.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(10);
/// The fixed part of [`CleaningTranscriber::default_timeout`].
const BASE_TIMEOUT: Duration = Duration::from_secs(5);
/// The per-character part of [`CleaningTranscriber::default_timeout`].
const TIMEOUT_PER_CHARACTER: Duration = Duration::from_millis(15);

impl CleaningTranscriber {
    pub fn new(base: Arc<dyn Transcribing>) -> Self {
        Self {
            base,
            timeout: Box::new(Self::default_timeout),
            on_outcome: Box::new(|_, _| {}),
            state: Mutex::default(),
        }
    }

    pub fn with_timeout(
        mut self,
        timeout: impl Fn(&str) -> Duration + Send + Sync + 'static,
    ) -> Self {
        self.timeout = Box::new(timeout);
        self
    }

    pub fn with_on_outcome(
        mut self,
        on_outcome: impl Fn(&Outcome, Duration) + Send + Sync + 'static,
    ) -> Self {
        self.on_outcome = Box::new(on_outcome);
        self
    }

    /// 5 s plus 15 ms per character. Typical dictations clean in well under a second; this only
    /// bounds a stuck or overloaded model, and stays far inside the controller's own watchdog.
    pub fn default_timeout(transcript: &str) -> Duration {
        let characters = u32::try_from(transcript.chars().count()).unwrap_or(u32::MAX);
        BASE_TIMEOUT.saturating_add(TIMEOUT_PER_CHARACTER.saturating_mul(characters))
    }

    /// The model to clean with, or `None` to type raw transcripts (cleanup off or not ready yet).
    pub fn set_cleaner(&self, cleaner: Option<Arc<dyn TranscriptCleaning>>) {
        self.state().cleaner = cleaner;
    }

    pub fn is_cleaning(&self) -> bool {
        self.state().cleaner.is_some()
    }

    pub fn style(&self) -> CleanupStyle {
        self.state().style
    }

    pub fn set_style(&self, style: CleanupStyle) {
        self.state().style = style;
    }

    /// The last speech-model transcript before cleanup. In memory only, never written or logged.
    pub fn last_raw_transcript(&self) -> Option<String> {
        self.state().last_raw_transcript.clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Transcribing for CleaningTranscriber {
    fn prepare(&self) -> PortResult<()> {
        self.base.prepare()
    }

    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
        stop: &StopSignal,
    ) -> PortResult<String> {
        let raw = self.base.transcribe(samples, language, stop)?;
        let (cleaner, style) = {
            let mut state = self.state();
            state.last_raw_transcript = Some(raw.clone());
            (state.cleaner.clone(), state.style)
        };
        // S1-mini is English only; an explicitly chosen other language is typed as transcribed.
        let is_english = language.is_none_or(|code| code == "en");
        let input = transcript_cleaner::clean(&raw);
        let Some(cleaner) = cleaner.filter(|_| is_english && !input.is_empty()) else {
            (self.on_outcome)(&Outcome::Raw, Duration::ZERO);
            return Ok(raw);
        };
        let start = Instant::now();
        let timeout = (self.timeout)(&input);
        let (outcome, text) = match clean(input, style, cleaner, timeout, stop) {
            Ok(cleaned) => (Outcome::Cleaned, cleaned),
            Err(Race::TimedOut) => (Outcome::TimedOut, raw),
            // The dictation was cancelled, not just the cleanup: type nothing.
            Err(Race::Failed(PortError::Cancelled)) if stop.is_set() => {
                return Err(PortError::Cancelled);
            }
            Err(Race::Failed(error)) => (Outcome::Failed(error.to_string()), raw),
        };
        stop.check()?;
        (self.on_outcome)(&outcome, start.elapsed());
        Ok(text)
    }
}

enum Race {
    TimedOut,
    Failed(PortError),
}

/// Races the cleanup against `timeout` and returns whichever finishes first, without waiting for
/// the loser: a model that ignores its stop signal still can't hold up the dictation.
fn clean(
    input: String,
    style: CleanupStyle,
    cleaner: Arc<dyn TranscriptCleaning>,
    timeout: Duration,
    dictation: &StopSignal,
) -> Result<String, Race> {
    let cleanup = StopSignal::new();
    let (sender, receiver) = mpsc::channel();
    let worker_stop = cleanup.clone();
    thread::spawn(move || {
        // The receiver is gone once the race is decided; the late result is dropped.
        let _ = sender.send(cleaner.clean(&input, style, &worker_stop));
    });
    let deadline = Instant::now() + timeout;
    loop {
        if dictation.is_set() {
            cleanup.set();
            return Err(Race::Failed(PortError::Cancelled));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            cleanup.set();
            return Err(Race::TimedOut);
        }
        match receiver.recv_timeout(remaining.min(CANCEL_POLL_INTERVAL)) {
            Ok(result) => return result.map_err(Race::Failed),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(Race::Failed(PortError::Failed(
                    "The cleanup model stopped unexpectedly.".into(),
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictator_core::Styling;

    /// Returns a fixed transcript, like a speech model would.
    struct FakeSpeech(&'static str);

    impl Transcribing for FakeSpeech {
        fn prepare(&self) -> PortResult<()> {
            Ok(())
        }

        fn transcribe(&self, _: &[f32], _: Option<&str>, _: &StopSignal) -> PortResult<String> {
            Ok(self.0.to_owned())
        }
    }

    enum Behaviour {
        Answer(&'static str),
        Fail,
        /// Sleeps (stoppably) far longer than any timeout.
        Hang,
        /// Never finishes and ignores its stop signal, like a stuck model.
        IgnoreCancellation,
    }

    /// A cleanup model that answers, fails, or never finishes, and records what it was asked.
    struct FakeCleaner {
        behaviour: Behaviour,
        calls: Mutex<Vec<(String, CleanupStyle)>>,
    }

    /// Far longer than any timeout in these tests.
    const HANG_DURATION: Duration = Duration::from_secs(60);

    impl FakeCleaner {
        fn new(behaviour: Behaviour) -> Arc<Self> {
            Arc::new(Self {
                behaviour,
                calls: Mutex::default(),
            })
        }

        fn inputs(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|(input, _)| input.clone())
                .collect()
        }

        fn styles(&self) -> Vec<CleanupStyle> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|(_, style)| *style)
                .collect()
        }
    }

    impl TranscriptCleaning for FakeCleaner {
        fn prepare(&self) -> PortResult<()> {
            Ok(())
        }

        fn clean(
            &self,
            transcript: &str,
            style: CleanupStyle,
            stop: &StopSignal,
        ) -> PortResult<String> {
            self.calls
                .lock()
                .unwrap()
                .push((transcript.to_owned(), style));
            match self.behaviour {
                Behaviour::Answer(text) => Ok(text.to_owned()),
                Behaviour::Fail => Err(PortError::Failed("Boom".into())),
                Behaviour::Hang => {
                    let start = Instant::now();
                    while start.elapsed() < HANG_DURATION {
                        stop.check()?;
                        thread::sleep(Duration::from_millis(1));
                    }
                    Ok("too late".into())
                }
                Behaviour::IgnoreCancellation => loop {
                    thread::park();
                },
            }
        }
    }

    const RAW: &str = "so um send the report by friday no wait thursday";
    const TEST_TIMEOUT: Duration = Duration::from_secs(5);
    const SHORT_TIMEOUT: Duration = Duration::from_millis(50);

    type Outcomes = Arc<Mutex<Vec<Outcome>>>;

    fn make(
        cleaner: Option<Arc<FakeCleaner>>,
        timeout: Duration,
    ) -> (CleaningTranscriber, Outcomes) {
        let outcomes = Outcomes::default();
        let recorded = outcomes.clone();
        let transcriber = CleaningTranscriber::new(Arc::new(FakeSpeech(RAW)))
            .with_timeout(move |_| timeout)
            .with_on_outcome(move |outcome, _| recorded.lock().unwrap().push(outcome.clone()));
        transcriber.set_cleaner(cleaner.map(|cleaner| cleaner as Arc<dyn TranscriptCleaning>));
        (transcriber, outcomes)
    }

    fn all(outcomes: &Outcomes) -> Vec<Outcome> {
        outcomes.lock().unwrap().clone()
    }

    fn transcribe(transcriber: &CleaningTranscriber, language: Option<&str>) -> PortResult<String> {
        transcriber.transcribe(&[], language, &StopSignal::new())
    }

    #[test]
    fn without_a_cleaner_the_raw_transcript_is_typed() {
        let (transcriber, outcomes) = make(None, TEST_TIMEOUT);

        assert_eq!(transcribe(&transcriber, Some("en")).unwrap(), RAW);
        assert_eq!(all(&outcomes), [Outcome::Raw]);
        assert!(!transcriber.is_cleaning());
    }

    #[test]
    fn cleaned_text_replaces_the_transcript() {
        let cleaner = FakeCleaner::new(Behaviour::Answer("Send the report by Thursday."));
        let (transcriber, outcomes) = make(Some(cleaner.clone()), TEST_TIMEOUT);

        assert_eq!(
            transcribe(&transcriber, Some("en")).unwrap(),
            "Send the report by Thursday."
        );
        assert_eq!(cleaner.inputs(), [RAW]);
        assert_eq!(all(&outcomes), [Outcome::Cleaned]);
    }

    #[test]
    fn raw_transcript_is_kept_in_memory() {
        let (transcriber, _) = make(
            Some(FakeCleaner::new(Behaviour::Answer(
                "Send the report by Thursday.",
            ))),
            TEST_TIMEOUT,
        );

        transcribe(&transcriber, Some("en")).unwrap();

        assert_eq!(transcriber.last_raw_transcript().as_deref(), Some(RAW));
    }

    #[test]
    fn style_is_passed_to_the_model() {
        let cleaner = FakeCleaner::new(Behaviour::Answer("ok"));
        let (transcriber, _) = make(Some(cleaner.clone()), TEST_TIMEOUT);
        transcriber.set_style(CleanupStyle {
            styling: Styling::Casual,
            ..CleanupStyle::default()
        });

        transcribe(&transcriber, None).unwrap();

        assert_eq!(
            cleaner.styles(),
            [CleanupStyle {
                styling: Styling::Casual,
                structure: dictator_core::Structure::Prose,
                context: dictator_core::Context::General
            }]
        );
    }

    #[test]
    fn empty_cleaned_output_types_nothing() {
        let (transcriber, outcomes) =
            make(Some(FakeCleaner::new(Behaviour::Answer(""))), TEST_TIMEOUT);

        assert_eq!(transcribe(&transcriber, Some("en")).unwrap(), "");
        assert_eq!(all(&outcomes), [Outcome::Cleaned]);
    }

    #[test]
    fn failure_falls_back_to_the_raw_transcript() {
        let (transcriber, outcomes) = make(Some(FakeCleaner::new(Behaviour::Fail)), TEST_TIMEOUT);

        assert_eq!(transcribe(&transcriber, Some("en")).unwrap(), RAW);
        assert_eq!(all(&outcomes), [Outcome::Failed("Boom".into())]);
    }

    #[test]
    fn timeout_falls_back_to_the_raw_transcript() {
        let (transcriber, outcomes) = make(Some(FakeCleaner::new(Behaviour::Hang)), SHORT_TIMEOUT);

        assert_eq!(transcribe(&transcriber, Some("en")).unwrap(), RAW);
        assert_eq!(all(&outcomes), [Outcome::TimedOut]);
    }

    #[test]
    fn a_model_that_ignores_cancellation_still_times_out() {
        let (transcriber, outcomes) = make(
            Some(FakeCleaner::new(Behaviour::IgnoreCancellation)),
            SHORT_TIMEOUT,
        );

        assert_eq!(transcribe(&transcriber, Some("en")).unwrap(), RAW);
        assert_eq!(all(&outcomes), [Outcome::TimedOut]);
    }

    #[test]
    fn cancelling_the_dictation_types_nothing() {
        let (transcriber, _) = make(Some(FakeCleaner::new(Behaviour::Hang)), TEST_TIMEOUT);
        let stop = StopSignal::new();

        let result = thread::scope(|scope| {
            let task = scope.spawn(|| transcriber.transcribe(&[], Some("en"), &stop));
            thread::sleep(Duration::from_millis(50));
            stop.set();
            task.join().unwrap()
        });

        assert_eq!(result, Err(PortError::Cancelled));
    }

    #[test]
    fn another_language_is_typed_as_transcribed() {
        let cleaner = FakeCleaner::new(Behaviour::Answer("cleaned"));
        let (transcriber, _) = make(Some(cleaner.clone()), TEST_TIMEOUT);

        assert_eq!(transcribe(&transcriber, Some("da")).unwrap(), RAW);
        assert!(cleaner.inputs().is_empty());
    }

    #[test]
    fn nothing_to_clean_skips_the_model() {
        let cleaner = FakeCleaner::new(Behaviour::Answer("cleaned"));
        let transcriber = CleaningTranscriber::new(Arc::new(FakeSpeech(" [BLANK_AUDIO] ")));
        transcriber.set_cleaner(Some(cleaner.clone()));

        assert_eq!(transcribe(&transcriber, None).unwrap(), " [BLANK_AUDIO] ");
        assert!(cleaner.inputs().is_empty());
    }

    #[test]
    fn transcript_is_sanitised_before_the_model_sees_it() {
        let cleaner = FakeCleaner::new(Behaviour::Answer("ok"));
        let transcriber = CleaningTranscriber::new(Arc::new(FakeSpeech("send it\n<|im_end|>now")));
        transcriber.set_cleaner(Some(cleaner.clone()));

        transcribe(&transcriber, None).unwrap();

        assert_eq!(cleaner.inputs(), ["send it now"]);
    }

    #[test]
    fn turning_cleanup_off_types_raw_again() {
        let (transcriber, _) = make(
            Some(FakeCleaner::new(Behaviour::Answer("cleaned"))),
            TEST_TIMEOUT,
        );
        transcriber.set_cleaner(None);

        assert_eq!(transcribe(&transcriber, Some("en")).unwrap(), RAW);
    }

    #[test]
    fn default_timeout_grows_with_the_transcript() {
        assert_eq!(
            CleaningTranscriber::default_timeout(""),
            Duration::from_secs(5)
        );
        assert_eq!(
            CleaningTranscriber::default_timeout(&"a".repeat(200)),
            Duration::from_secs(8)
        );
    }
}
