# Glossary

- **ModelSession**: The actor that loads one downloaded speech model and runs transcriptions on
  it one at a time (ADR 0008). It checks the model's files, loads the engine once, and turns a
  cancelled dictation into `CancellationError`.
- **LoadedSpeechModel**: A speech-model engine after loading. WhisperKit and Parakeet each supply
  one inside their own target; `ModelSession` is the only caller.
- **StopSignal**: The flag a `ModelSession` sets when a transcription is cancelled, so the engine
  can stop decoding instead of finishing text nobody will use.
- **ModelLibrary**: The app-level owner of which model is chosen, downloaded, deleted and in use
  (ADR 0007). It hands a ready model folder to a `ModelSession`.
- **Phonon-2**: Fermion Research's English re-training of NVIDIA Parakeet TDT 0.6B v3, offered as a
  third speech model. It runs through the same FluidAudio adapter as Parakeet v2 (ADR 0006 note,
  2026-10-23).
- **S1-mini by Superwhisper**: The optional transcript-cleanup model (ADR 0011). Its licence
  requires this exact name wherever it is shown.
- **TranscriptCleaning**: The port that turns a raw transcript into cleaned text for a given
  style. `S1MiniCleaner` implements it with llama.cpp; tests use a fake.
- **CleaningTranscriber**: The `Transcribing` decorator between the speech model and the
  `DictationController`. It cleans each transcript when a cleaner is set, and returns the raw
  transcript when none is, or when cleanup fails or times out.
- **CleanupSettings**: The app-level owner of the cleanup switch, style, download and loading.
- **macOS app / Windows app**: The two apps in this monorepo, in `apps/macos` (Swift) and
  `apps/windows` (Rust and Tauri) (ADR 0012). They share behaviour, version numbers and the
  changelog.
- **dictator-core**: The Windows app's port of DictationCore to Rust. A pure state machine:
  `DictationController::handle(event)` returns effects, and the app shell runs them.
- **Runner**: The Windows app's thread that owns the `DictationController`, runs its effects and
  feeds the results back (`apps/windows/crates/dictator-app/src/dictation.rs`).
- **Per-user install**: The Windows installer's only mode. Dictator goes into
  `%LOCALAPPDATA%\Programs\Dictator` with no administrator rights, and its data goes into
  `%LOCALAPPDATA%\Dictator`.
