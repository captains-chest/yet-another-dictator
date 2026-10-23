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
