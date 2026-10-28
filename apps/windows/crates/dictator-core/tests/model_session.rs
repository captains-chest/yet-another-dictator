//! Port of ModelSessionTests.swift, with threads instead of tasks.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use dictator_core::model_session::ModelSession;
use dictator_core::{LoadedSpeechModel, PortError, PortResult, StopSignal, Transcribing};

#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    opened: Condvar,
}

impl Gate {
    fn wait(&self) {
        let open = self.open.lock().unwrap();
        drop(self.opened.wait_while(open, |open| !*open).unwrap());
    }

    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.opened.notify_all();
    }
}

#[derive(Default)]
struct EventLog(Mutex<Vec<String>>);

impl EventLog {
    fn add(&self, event: impl Into<String>) {
        self.0.lock().unwrap().push(event.into());
    }
    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
    fn count(&self, event: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|e| *e == event)
            .count()
    }
    fn wait_for(&self, event: &str) {
        for _ in 0..500 {
            if self.count(event) > 0 {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

struct FakeModel {
    log: Arc<EventLog>,
    hold: Option<Arc<Gate>>,
}

impl LoadedSpeechModel for FakeModel {
    fn transcribe(
        &self,
        samples: &[f32],
        _language: Option<&str>,
        stop: &StopSignal,
    ) -> PortResult<String> {
        let tag = samples.first().map_or(0, |&s| s as i32);
        self.log.add(format!("start{tag}"));
        if let Some(hold) = &self.hold {
            hold.wait();
        }
        // As in the adapters: a stop set while running cancels the transcription.
        self.log.add(format!("end{tag}"));
        stop.check()?;
        Ok(format!("fake {}", stop.is_set()))
    }
}

struct Fixture {
    root: tempfile::TempDir,
    log: Arc<EventLog>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            log: Arc::default(),
        }
    }

    fn session(
        &self,
        gate: Option<Arc<Gate>>,
        fail_first_load: bool,
        model_hold: Option<Arc<Gate>>,
    ) -> Arc<ModelSession> {
        let folder = self.root.path().join("model");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("needed.txt"), b"").unwrap();
        let attempts = AtomicUsize::new(0);
        let log = self.log.clone();
        Arc::new(ModelSession::new(
            folder,
            vec!["needed.txt".into()],
            Box::new(move |_| {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                log.add("load");
                if let Some(gate) = &gate {
                    gate.wait();
                }
                if fail_first_load && attempt == 1 {
                    return Err(PortError::Failed("bad server response".into()));
                }
                Ok(Box::new(FakeModel {
                    log: log.clone(),
                    hold: model_hold.clone(),
                }))
            }),
        ))
    }
}

fn transcribe_on_thread(
    session: &Arc<ModelSession>,
    tag: f32,
    stop: &StopSignal,
) -> thread::JoinHandle<PortResult<String>> {
    let (session, stop) = (session.clone(), stop.clone());
    thread::spawn(move || session.transcribe(&[tag], None, &stop))
}

#[test]
fn missing_files_fails_and_never_loads() {
    let fixture = Fixture::new();
    let log = fixture.log.clone();
    let empty: PathBuf = fixture.root.path().join("empty");
    let session = ModelSession::new(
        empty,
        vec!["needed.txt".into()],
        Box::new(move |_| {
            log.add("load2");
            Ok(Box::new(FakeModel {
                log: log.clone(),
                hold: None,
            }))
        }),
    );
    assert_eq!(
        session.prepare(),
        Err(PortError::Failed(
            "Model is incomplete, missing: needed.txt".into()
        ))
    );
    assert_eq!(fixture.log.count("load2"), 0);
}

#[test]
fn concurrent_prepare_shares_one_load() {
    let fixture = Fixture::new();
    let gate = Arc::new(Gate::default());
    let session = fixture.session(Some(gate.clone()), false, None);
    let prepare = || {
        let session = session.clone();
        thread::spawn(move || session.prepare())
    };
    let (one, two) = (prepare(), prepare());
    fixture.log.wait_for("load");
    thread::sleep(Duration::from_millis(20)); // let the second caller start waiting
    gate.open();
    assert_eq!(one.join().unwrap(), Ok(()));
    assert_eq!(two.join().unwrap(), Ok(()));
    assert_eq!(fixture.log.count("load"), 1);
}

#[test]
fn failed_load_fails_waiters_then_retries() {
    let fixture = Fixture::new();
    let gate = Arc::new(Gate::default());
    let session = fixture.session(Some(gate.clone()), true, None);
    let prepare = || {
        let session = session.clone();
        thread::spawn(move || session.prepare())
    };
    let (one, two) = (prepare(), prepare());
    fixture.log.wait_for("load");
    thread::sleep(Duration::from_millis(20)); // let the second caller join the first load
    gate.open();
    let failure = Err(PortError::Failed("bad server response".into()));
    assert_eq!(one.join().unwrap(), failure);
    assert_eq!(two.join().unwrap(), failure);
    assert_eq!(session.prepare(), Ok(()));
    assert_eq!(fixture.log.count("load"), 2);
}

#[test]
fn transcribe_without_prepare_loads_first() {
    let fixture = Fixture::new();
    let session = fixture.session(None, false, None);
    assert_eq!(
        session.transcribe(&[42.0], None, &StopSignal::new()),
        Ok("fake false".into())
    );
    assert_eq!(fixture.log.count("load"), 1);
}

#[test]
fn cancelled_held_transcription_and_next_sequenced() {
    let fixture = Fixture::new();
    let hold = Arc::new(Gate::default());
    let session = fixture.session(None, false, Some(hold.clone()));
    let (stop1, stop2) = (StopSignal::new(), StopSignal::new());
    let first = transcribe_on_thread(&session, 1.0, &stop1);
    fixture.log.wait_for("start1");
    let second = transcribe_on_thread(&session, 2.0, &stop2);
    stop1.set();
    hold.open();
    assert_eq!(first.join().unwrap(), Err(PortError::Cancelled));
    assert_eq!(second.join().unwrap(), Ok("fake false".into()));
    let sequence: Vec<String> = fixture
        .log
        .all()
        .into_iter()
        .filter(|e| e.starts_with("start") || e.starts_with("end"))
        .collect();
    assert_eq!(sequence, ["start1", "end1", "start2", "end2"]);
}

#[test]
fn queued_cancellation_never_reaches_model() {
    let fixture = Fixture::new();
    let hold = Arc::new(Gate::default());
    let session = fixture.session(None, false, Some(hold.clone()));
    let (stop1, stop2) = (StopSignal::new(), StopSignal::new());
    let first = transcribe_on_thread(&session, 1.0, &stop1);
    fixture.log.wait_for("start1");
    let second = transcribe_on_thread(&session, 2.0, &stop2);
    thread::sleep(Duration::from_millis(20));
    stop2.set();
    hold.open();
    assert_eq!(first.join().unwrap(), Ok("fake false".into()));
    assert_eq!(second.join().unwrap(), Err(PortError::Cancelled));
    assert_eq!(fixture.log.count("start2"), 0);
}

#[test]
fn completed_transcription_never_sees_stop() {
    let fixture = Fixture::new();
    let session = fixture.session(None, false, None);
    assert_eq!(
        session.transcribe(&[3.0], None, &StopSignal::new()),
        Ok("fake false".into())
    );
}
