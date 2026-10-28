//! S1-mini by Superwhisper behind the `TranscriptCleaning` port (ADR 0011). The only code that uses
//! llama.cpp. Mirrors the macOS `S1MiniCleaner` and `LlamaModel`.
//!
//! The model is loaded once, on its own worker thread, which then cleans one transcript at a time
//! in the order they arrive: a cleanup abandoned after a timeout finishes (or stops at its next
//! token) before the next one starts. Dropping the cleaner ends the thread and frees the model.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;

use dictator_cleanup::s1_mini_format;
use dictator_core::{CleanupStyle, PortError, PortResult, StopSignal, TranscriptCleaning};
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

pub const MODEL_ID: &str = "s1-mini";
/// The GGUF inside the model folder, as named in the manifest.
pub const MODEL_FILE_NAME: &str = "s1-mini-q4_k_m.gguf";

/// Room for the prompt (system prompt plus a chunk of at most 1000 tokens) and its answer (at most
/// 1.3 × the chunk + 32).
const CONTEXT_LENGTH: u32 = 4096;
/// Every layer on the GPU with Vulkan; CPU only otherwise.
const GPU_LAYERS: u32 = if cfg!(feature = "vulkan") {
    u32::MAX
} else {
    0
};

/// Marks where the transcript goes in the prompt, so the fixed parts around it can be tokenised
/// with special tokens and the transcript without.
const TRANSCRIPT_MARKER: &str = "\u{0}";

struct Job {
    transcript: String,
    style: CleanupStyle,
    stop: StopSignal,
    reply: Sender<PortResult<String>>,
}

pub struct S1MiniLlamaCleaner {
    model_file: PathBuf,
    /// The loaded model's worker, once loaded. Cleared after a failed load, so a retry works.
    worker: Mutex<Option<Sender<Job>>>,
}

impl S1MiniLlamaCleaner {
    pub fn new(model_folder: &Path) -> Self {
        Self {
            model_file: model_folder.join(MODEL_FILE_NAME),
            worker: Mutex::new(None),
        }
    }

    /// The worker, loading the model first if needed. Concurrent callers wait for one load.
    fn worker(&self) -> PortResult<Sender<Job>> {
        let mut worker = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(jobs) = worker.as_ref() {
            return Ok(jobs.clone());
        }
        if !self.model_file.is_file() {
            let name = MODEL_FILE_NAME;
            return Err(PortError::Failed(format!(
                "The cleanup model file {name} is missing. Turn cleanup off and on to download it again."
            )));
        }
        let (jobs, receiver) = mpsc::channel();
        let (loaded, load_result) = mpsc::channel();
        let file = self.model_file.clone();
        thread::Builder::new()
            .name("s1-mini".into())
            .spawn(move || run_worker(&file, &loaded, receiver))
            .map_err(|_| PortError::Failed(Failure::Load.to_string()))?;
        load_result
            .recv()
            .unwrap_or(Err(PortError::Failed(Failure::Load.to_string())))?;
        *worker = Some(jobs.clone());
        Ok(jobs)
    }
}

impl TranscriptCleaning for S1MiniLlamaCleaner {
    fn prepare(&self) -> PortResult<()> {
        self.worker().map(drop)
    }

    fn clean(
        &self,
        transcript: &str,
        style: CleanupStyle,
        stop: &StopSignal,
    ) -> PortResult<String> {
        let jobs = self.worker()?;
        let (reply, result) = mpsc::channel();
        let job = Job {
            transcript: transcript.to_owned(),
            style,
            stop: stop.clone(),
            reply,
        };
        let stopped = || PortError::Failed(Failure::Load.to_string());
        jobs.send(job).map_err(|_| stopped())?;
        result.recv().map_err(|_| stopped())?
    }
}

fn backend() -> PortResult<&'static LlamaBackend> {
    static BACKEND: OnceLock<Option<LlamaBackend>> = OnceLock::new();
    BACKEND
        .get_or_init(|| {
            let mut backend = LlamaBackend::init().ok()?;
            // llama.cpp logs model and device details to stderr by default; it never sees our logs.
            backend.void_logs();
            Some(backend)
        })
        .as_ref()
        .ok_or_else(|| PortError::Failed(Failure::Load.to_string()))
}

/// Loads the model, reports the outcome on `loaded`, then serves jobs until the cleaner is dropped.
fn run_worker(file: &Path, loaded: &Sender<PortResult<()>>, jobs: Receiver<Job>) {
    let loading = backend().and_then(|backend| {
        let path = crate::utf8_path(file)?;
        let params = LlamaModelParams::default().with_n_gpu_layers(GPU_LAYERS);
        LlamaModel::load_from_file(backend, path, &params)
            .map_err(|_| Failure::Load.into())
            .map(|model| (backend, model))
    });
    let (backend, model) = match loading {
        Ok(loaded_model) => loaded_model,
        Err(error) => {
            let _ = loaded.send(Err(error));
            return;
        }
    };
    let context_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(CONTEXT_LENGTH))
        .with_n_batch(CONTEXT_LENGTH);
    let context = match model.new_context(backend, context_params) {
        Ok(context) => context,
        Err(_) => {
            let _ = loaded.send(Err(Failure::Context.into()));
            return;
        }
    };
    let mut llama = Llama {
        model: &model,
        context,
    };
    // Warm up: the first pass sets up the compute buffers, so the first dictation doesn't wait.
    let _ = llama.clean("um", CleanupStyle::default(), &StopSignal::new());
    let _ = loaded.send(Ok(()));
    for job in jobs {
        let result = job
            .stop
            .check()
            .and_then(|()| llama.clean(&job.transcript, job.style, &job.stop));
        let _ = job.reply.send(result);
    }
}

#[derive(Debug)]
enum Failure {
    Load,
    Context,
    Decode,
    InputTooLong(usize),
    AnswerTooLong,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load => f.write_str("The cleanup model could not be loaded."),
            Self::Context => f.write_str("The cleanup model could not start."),
            Self::Decode => f.write_str("The cleanup model stopped with an error."),
            Self::InputTooLong(count) => {
                write!(f, "A piece of {count} tokens is too long to clean.")
            }
            Self::AnswerTooLong => {
                f.write_str("The cleanup model's answer did not end within its token limit.")
            }
        }
    }
}

impl From<Failure> for PortError {
    fn from(failure: Failure) -> Self {
        Self::Failed(failure.to_string())
    }
}

/// One loaded S1-mini with one context. Lives on the worker thread only.
struct Llama<'model> {
    model: &'model LlamaModel,
    context: LlamaContext<'model>,
}

impl Llama<'_> {
    /// Cleans the transcript chunk by chunk and joins the non-empty answers with spaces.
    fn clean(
        &mut self,
        transcript: &str,
        style: CleanupStyle,
        stop: &StopSignal,
    ) -> PortResult<String> {
        let chunks = s1_mini_format::chunks(transcript, s1_mini_format::MAX_INPUT_TOKENS, |text| {
            self.tokenize(text, false).len()
        });
        let mut answers = Vec::new();
        for chunk in chunks {
            stop.check()?;
            let answer = self.generate(&chunk, style, stop)?;
            if !answer.is_empty() {
                answers.push(answer);
            }
        }
        Ok(answers.join(" "))
    }

    /// One greedy pass over one chunk.
    fn generate(
        &mut self,
        chunk: &str,
        style: CleanupStyle,
        stop: &StopSignal,
    ) -> PortResult<String> {
        // The transcript is tokenised as plain text, so words that look like control tokens can't
        // steer the model; only the fixed prompt around it may contain them.
        let prompt = s1_mini_format::prompt(TRANSCRIPT_MARKER, style);
        let (before, after) = prompt
            .split_once(TRANSCRIPT_MARKER)
            .unwrap_or((&prompt, ""));
        let input = self.tokenize(chunk, false);
        let tokens = [
            self.tokenize(before, true),
            input.clone(),
            self.tokenize(after, true),
        ]
        .concat();
        let max_new = s1_mini_format::max_new_tokens(input.len());
        if tokens.len() + max_new > self.context.n_ctx() as usize {
            return Err(Failure::InputTooLong(input.len()).into());
        }

        self.context.clear_kv_cache();
        let mut sampler = LlamaSampler::greedy();
        let mut batch = LlamaBatch::get_one(&tokens).map_err(|_| Failure::Decode)?;
        self.context
            .decode(&mut batch)
            .map_err(|_| Failure::Decode)?;

        let vocab = self.model.vocab();
        let mut output = Vec::new();
        for _ in 0..max_new {
            stop.check()?;
            let token = sampler.sample(&self.context, -1);
            if vocab.is_eog(token) {
                return Ok(s1_mini_format::answer(&String::from_utf8_lossy(&output)));
            }
            sampler.accept(token);
            output.extend(vocab.token_to_piece(token, false, None));
            let next = [token];
            let mut batch = LlamaBatch::get_one(&next).map_err(|_| Failure::Decode)?;
            self.context
                .decode(&mut batch)
                .map_err(|_| Failure::Decode)?;
        }
        // A cut-off answer could drop the end of the dictation; the caller types the raw text.
        Err(Failure::AnswerTooLong.into())
    }

    fn tokenize(&self, text: &str, special: bool) -> Vec<LlamaToken> {
        self.model.vocab().tokenize(text.as_bytes(), false, special)
    }
}
