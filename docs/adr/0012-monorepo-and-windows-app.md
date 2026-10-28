# 12. Monorepo with a Rust/Tauri Windows app

- Status: Accepted
- Date: 2026-10-28

## Context

Dictator needs a Windows version, and the two apps should not drift apart as features land. The
macOS app is Swift with Core ML models, and neither runs on Windows. Windows users at our
organisation can't be expected to have admin rights, so the app must install per user.

## Decision

**One repository.** The macOS app moves to `apps/macos` unchanged and the Windows app lives in
`apps/windows`. Shared at the root: CHANGELOG (one version line for both), docs/ADRs, glossary,
licences and CI. A feature PR changes both apps, or says why not.

**The Windows app is Rust, as far as makes sense.**
- `dictator-core` ports the Swift DictationCore line by line, with its tests:
  - the controller is a pure state machine (`handle(event) -> effects`);
  - the shell executes the effects.
- Engines use Rust bindings to native runtimes:
  - sherpa-onnx for Parakeet;
  - whisper.cpp for Whisper;
  - llama.cpp for S1-mini.
- Win32 is reached through the `windows` crate.
- The UI is Tauri 2: Rust with a small static HTML/JS front-end in WebView2. It brings a per-user
  NSIS installer, a signed updater, autostart and a single-instance guard. JavaScript only renders
  the view and sends commands.

**Windows-native models.** The same model families as macOS, in builds that run on Windows CPUs:
- Parakeet TDT 0.6B v2 as int8 ONNX (sherpa-onnx);
- Whisper Large v3 Turbo as ggml Q5_0;
- S1-mini as the same GGUF.

They are pinned by SHA-256 in `apps/windows/model-manifest.txt`, which ships in the install, as in
ADR 0004 and 0007. Phonon-2 is left out until an official ONNX export exists.

**Per-user install by default.**
- NSIS `installMode: currentUser` installs to `%LOCALAPPDATA%\Programs` with no UAC prompt.
- Data, models and settings live in `%LOCALAPPDATA%\Dictator`.
- Autostart uses `HKCU\…\Run`.
- Updates run the same per-user installer in passive mode.

**Native linking.**
- llama.cpp is linked as DLLs, because its ggml clashes with whisper.cpp's.
- Everything uses the static C runtime that sherpa-onnx's prebuilt libraries need.
- The sherpa-onnx archive is fetched and SHA-256-verified before building.

## Consequences

- A behaviour change to the dictation logic must be made in Swift DictationCore and in
  `dictator-core`. The ported tests keep the two honest.
- Windows CI (`windows` job) runs fmt, clippy and tests; the release job builds the installer and
  `latest.json`. Nothing Windows-specific can be verified on the macOS developers' machines beyond
  cross-compiling `dictator-windows`.
- Until Authenticode signing is set up, SmartScreen warns on first run. Updates are still
  verified with the Tauri updater key.
- Inference is CPU-only by default; Whisper's Vulkan backend is a feature flag for later.
