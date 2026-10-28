//! The imperative shell around [`DictationController`]: one thread owns the controller, the
//! microphone and the text reader, and runs every effect the controller asks for. Slow work
//! (loading, transcribing, typing) runs on short-lived threads and reports back by message.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Instant;

use dictator_audio::MicrophoneRecorder;
use dictator_core::dictation_controller::{
    Config, DictationController, DictationState, Effect, Event, WorkId,
};
use dictator_core::dictation_language::DictationLanguage;
use dictator_core::{
    AudioRecording, PortError, PortResult, PrecedingTextReading, StopSignal, TextInserting,
    Transcribing,
};
use dictator_windows::platform;

const NO_MODEL: &str = "Choose a speech model in Dictator Settings.";

pub enum Message {
    Event(Event),
    /// The model to transcribe with; `None` until one is installed. Reloads it.
    SetTranscriber(Option<Arc<dyn Transcribing>>),
    SetMicrophone(Option<String>),
    SetKeepMicrophoneReady(bool),
    SetLanguage(DictationLanguage),
    ModelPrepared {
        generation: u64,
        result: PortResult<()>,
    },
}

/// What the tray and overlay show.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub state: DictationState,
    pub error: Option<String>,
    pub undelivered: bool,
    pub can_undo: bool,
}

pub struct Callbacks {
    pub on_change: Box<dyn Fn(&Snapshot) + Send>,
    /// Live microphone loudness while recording, 0…1.
    pub on_level: Box<dyn Fn(f32) + Send>,
}

#[derive(Clone)]
pub struct Runner {
    sender: Sender<Message>,
}

impl Runner {
    pub fn start(callbacks: Callbacks) -> Self {
        let (sender, receiver) = mpsc::channel();
        let runner_sender = sender.clone();
        thread::Builder::new()
            .name("dictator-dictation".into())
            .spawn(move || Loop::new(runner_sender, callbacks).run(receiver))
            .expect("the dictation thread starts");
        Self { sender }
    }

    pub fn send(&self, event: Event) {
        self.post(Message::Event(event));
    }

    pub fn post(&self, message: Message) {
        // Fails only once the app is shutting down.
        let _ = self.sender.send(message);
    }
}

type SharedInserter = Arc<Mutex<Box<dyn TextInserting + Send>>>;

struct Loop {
    sender: Sender<Message>,
    callbacks: Callbacks,
    controller: DictationController,
    started: Instant,
    recorder: MicrophoneRecorder,
    /// Preflight is a quick check; it must not wait for a long text being typed by `typist`.
    checker: Box<dyn TextInserting + Send>,
    typist: SharedInserter,
    reader: Box<dyn PrecedingTextReading + Send>,
    transcriber: Option<Arc<dyn Transcribing>>,
    /// Only the latest model load counts; earlier ones may finish after a model switch.
    generation: u64,
    stops: HashMap<WorkId, StopSignal>,
    published: Option<Snapshot>,
}

impl Loop {
    fn new(sender: Sender<Message>, callbacks: Callbacks) -> Self {
        let reader = platform::preceding_text_reader();
        let config = Config {
            uses_history_for_joining: reader.uses_history_for_joining(),
            ..Config::default()
        };
        let failed = sender.clone();
        let full = sender.clone();
        let recorder = MicrophoneRecorder::new(
            move |message| {
                let _ = failed.send(Message::Event(Event::RecordingFailed(message)));
            },
            move || {
                let _ = full.send(Message::Event(Event::RecordingReachedLimit));
            },
        );
        Self {
            sender,
            callbacks,
            controller: DictationController::new(config),
            started: Instant::now(),
            recorder,
            checker: platform::text_inserter(),
            typist: Arc::new(Mutex::new(platform::text_inserter())),
            reader,
            transcriber: None,
            generation: 0,
            stops: HashMap::new(),
            published: None,
        }
    }

    fn run(mut self, receiver: Receiver<Message>) {
        while let Ok(message) = receiver.recv() {
            match message {
                Message::Event(event) => self.handle(event),
                Message::SetTranscriber(transcriber) => {
                    self.transcriber = transcriber;
                    self.handle(Event::Cancel);
                    self.handle(Event::LoadModel);
                }
                Message::SetMicrophone(id) => self.recorder.set_device(id),
                Message::SetKeepMicrophoneReady(on) => self.recorder.set_keeps_ready(on),
                Message::SetLanguage(language) => self.controller.set_language(language),
                Message::ModelPrepared { generation, result } => {
                    if generation == self.generation {
                        self.handle(Event::ModelPrepared(result));
                    }
                }
            }
            self.publish();
        }
    }

    /// Handles `event` and, before anything else, the results of the immediate effects it causes.
    fn handle(&mut self, event: Event) {
        let is_level = matches!(event, Event::Level(_));
        let mut pending = VecDeque::from([event]);
        while let Some(event) = pending.pop_front() {
            self.forget_finished(&event);
            let now = self.started.elapsed();
            for effect in self.controller.handle(event, now) {
                pending.extend(self.execute(effect));
            }
        }
        if is_level {
            (self.callbacks.on_level)(self.controller.level());
        }
    }

    fn forget_finished(&mut self, event: &Event) {
        match event {
            Event::InsertionFinished { id, .. }
            | Event::TranscriptionFinished { id, result: Err(_) } => {
                self.stops.remove(id);
            }
            _ => {}
        }
    }

    fn stop_signal(&mut self, id: WorkId) -> StopSignal {
        self.stops.entry(id).or_default().clone()
    }

    fn background(&self, work: impl FnOnce() -> Message + Send + 'static) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let _ = sender.send(work());
        });
    }

    /// Runs one effect; immediate effects return the event to feed back.
    fn execute(&mut self, effect: Effect) -> Option<Event> {
        match effect {
            Effect::Preflight => return Some(Event::PreflightFinished(self.checker.preflight())),
            Effect::StartMic => {
                let sender = self.sender.clone();
                let on_level = Box::new(move |level| {
                    let _ = sender.send(Message::Event(Event::Level(level)));
                });
                return Some(Event::MicStarted(self.recorder.start(on_level)));
            }
            Effect::StopMic => drop(self.recorder.stop()),
            Effect::StopMicForTranscription { id } => {
                return Some(Event::MicStopped {
                    id,
                    samples: self.recorder.stop(),
                });
            }
            Effect::ReadPrecedingText { id, max_length } => {
                return Some(Event::PrecedingTextRead {
                    id,
                    text: self.reader.text_before_cursor(max_length),
                });
            }
            Effect::PrepareModel => {
                self.generation += 1;
                let generation = self.generation;
                match self.transcriber.clone() {
                    Some(transcriber) => self.background(move || Message::ModelPrepared {
                        generation,
                        result: transcriber.prepare(),
                    }),
                    None => {
                        return Some(Event::ModelPrepared(Err(PortError::Failed(
                            NO_MODEL.into(),
                        ))));
                    }
                }
            }
            Effect::Transcribe {
                id,
                samples,
                language,
            } => {
                let stop = self.stop_signal(id);
                let Some(transcriber) = self.transcriber.clone() else {
                    return Some(Event::TranscriptionFinished {
                        id,
                        result: Err(PortError::Failed(NO_MODEL.into())),
                    });
                };
                self.background(move || {
                    let result = transcriber.transcribe(&samples, language.as_deref(), &stop);
                    Message::Event(Event::TranscriptionFinished { id, result })
                });
            }
            Effect::Insert { id, text } => {
                let stop = self.stop_signal(id);
                let typist = self.typist.clone();
                self.background(move || {
                    let result = lock(&typist).insert(&text, &stop);
                    Message::Event(Event::InsertionFinished { id, result })
                });
            }
            Effect::DeleteBackward { count } => {
                let typist = self.typist.clone();
                self.background(move || {
                    Message::Event(Event::DeleteFinished(
                        lock(&typist).delete_backward(count, &StopSignal::new()),
                    ))
                });
            }
            Effect::RetryInsert { text } => {
                let typist = self.typist.clone();
                self.background(move || {
                    Message::Event(Event::RetryFinished(
                        lock(&typist).insert(&text, &StopSignal::new()),
                    ))
                });
            }
            Effect::CancelWork { id } => {
                if let Some(stop) = self.stops.remove(&id) {
                    stop.set();
                }
            }
            Effect::ScheduleTimer { timer, after } => self.background(move || {
                thread::sleep(after);
                Message::Event(Event::TimerFired(timer))
            }),
            // The controller ignores timers it no longer waits for.
            Effect::CancelTimer(_) => {}
            // Read from the controller by `publish`.
            Effect::StateChanged(_)
            | Effect::ReportError(_)
            | Effect::ClearError
            | Effect::Timing(_) => {}
        }
        None
    }

    fn publish(&mut self) {
        let snapshot = Snapshot {
            state: self.controller.state().clone(),
            error: self.controller.last_error().map(str::to_owned),
            undelivered: self.controller.undelivered_transcript().is_some(),
            can_undo: self.controller.can_undo(),
        };
        if self.published.as_ref() != Some(&snapshot) {
            (self.callbacks.on_change)(&snapshot);
            self.published = Some(snapshot);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
