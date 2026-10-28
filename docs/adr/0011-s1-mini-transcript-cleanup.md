# 11. Optional transcript cleanup with S1-mini by Superwhisper

- Status: Accepted
- Date: 2026-10-23

## Context

Speech models write what was said, including "um", false starts and spoken corrections
("by friday no wait thursday"), and they spell out numbers, times and email addresses. Users
asked for text that reads as if it had been typed. Superwhisper published S1-mini, a 0.6B
model (fine-tuned from Qwen3-0.6B) trained for exactly this: it removes fillers, applies
corrections, and formats numbers, dates, times and addresses. Its Q4_K_M GGUF is 484 MB, with
94.8% token accuracy in Superwhisper's own evaluation. It is Apache 2.0 with one added term: the
model must always be identified as "S1-mini" by "Superwhisper", with that exact capitalization.

Dictation must stay fast, offline and safe: nothing may be lost or typed twice, and undo must
still remove exactly what was typed.

ADR numbers 0009 and 0010 are taken by the open PRs #60 and #61, so this is 0011.

## Decision

- Cleanup is opt-in: Settings → Cleanup, off by default. Nothing is downloaded until it is
  turned on. Then the model is downloaded once with the existing manifest machinery (ADR 0004,
  0007): `scripts/model-manifest.txt` pins the GGUF, its LICENSE and NOTICE to one Hugging Face
  commit with SHA-256 and size per file, and the manifest is bundled and signed with the app,
  never fetched. Inference runs on the Mac, offline.
- The pipeline is ASR → S1-mini → typed text. A `CleaningTranscriber` decorates the
  `Transcribing` the `DictationController` uses, so the controller, SmartJoin, history and undo
  are unchanged and see the cleaned text as the transcript. Undo deletes what was typed. The
  realtime audio path is untouched.
- A `TranscriptCleaning` port (in the `TranscriptCleanup` target, DictationCore and Foundation
  only) has a fake in tests, like the `Transcribing` seams of ADR 0007 and 0008. The real
  adapter, `S1MiniCleaner` in the `S1MiniCleanup` target, is the only code that imports
  llama.cpp.
- Prompting follows the model card exactly: its system prompt, a control line
  `[Styling: X] [Structure: Y] [Context: Z]` using trained values only, the transcript, and the
  assistant prefix with an empty think block. Decoding is greedy with at most
  ceil(1.3 × input tokens) + 32 new tokens; an answer that reaches the limit counts as a
  failure, so the raw transcript is typed rather than a cut-off one. Inputs over 1000 tokens are cut at sentence ends
  (words, for a single over-long sentence) and cleaned chunk by chunk. An empty answer is valid
  and means type nothing ("um" → "").
- Settings exposes Styling (casual, semi-casual, semi-formal, formal; default semi-formal).
  Structure stays prose and Context general in v1: the controller's `TranscriptCleaner`
  flattens newlines, so lists and email layout would not survive.
- Safety: the raw transcript is kept (`lastRawTranscript`). Cleanup runs only for English or
  automatic language and for a non-empty transcript. If no cleaner is ready, it throws, or it
  takes longer than 5 s + 15 ms per character, the raw transcript is typed. The timeout races
  an unstructured task, so a model that ignores cancellation cannot hold up dictation.
  Cancelling the dictation still types nothing. The transcript is tokenized with special
  tokens off, so dictated text cannot inject chat markup. Logs record only the outcome and the
  duration, never text.
- Runtime: llama.cpp, official `llama.xcframework` release b11838 (MIT), as a SwiftPM binary
  target pinned by URL and checksum. It runs on Metal with every layer on the GPU.
  `scripts/build-app.sh` embeds `llama.framework` (thinned to arm64) in `Contents/Frameworks`
  and signs it inside out with the app. No new entitlements: the sandbox and network-client
  entitlement the model download already uses are enough.
- The UI always names the model "S1-mini by Superwhisper". The model's LICENSE and NOTICE, and
  llama.cpp's licence, ship in `Contents/Resources/ThirdParty` and in `THIRD_PARTY_NOTICES.md`.

## Rejected

- **Cleanup in DictationCore.** It would add a model dependency to the shared core and to iOS;
  a decorator in the Mac app reaches the same seam without changing the core.
- **Always-on or auto-downloaded cleanup.** 484 MB and a behaviour change nobody asked for. It
  stays opt-in.
- **MLX or Core ML.** S1-mini is published as GGUF; llama.cpp runs it as is, with Metal, and has
  an official signed xcframework release.
- **A larger model or a cloud LLM.** Slower, or not offline.

## Consequences

- Turning cleanup on costs one 484 MB download and roughly 0.5 GB of memory while it is on.
  Turning it off unloads the model; deleting it frees the disk space.
- Dictation gets a little slower with cleanup on (one short greedy generation per dictation);
  it is never slower than the timeout, after which the raw text is typed.
- The app bundle gains `llama.framework` (about 12 MB for arm64).
- End-to-end tests against the real model run only when it is present (`S1_MINI_MODEL_FOLDER`,
  or `scripts/fetch-model.sh s1-mini` into `Model/`), like the speech-model tests.
