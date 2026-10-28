# Dictator for Windows

Part of the [Dictator monorepo](../../README.md). The feature set matches the macOS app, with the
same dictation logic ported to Rust.

## Using it

- Hold **Ctrl+Alt+D** to talk and release to finish, or tap it to start and tap again to stop.
- The text is typed where the cursor is.
- Change the shortcut in Settings (tray icon → Settings…).
- **Undo Last Dictation** in the tray menu, or an optional shortcut, removes exactly what was typed.
- Settings also covers:
  - the speech model;
  - language (Whisper only);
  - optional cleanup with S1-mini by Superwhisper;
  - microphone and keeping it ready;
  - open at login;
  - the daily update check.

## Install

Download `Dictator-<version>-windows-x64-setup.exe` from the
[latest release](https://github.com/captains-chest/yet-another-dictator/releases) and run it.

- **It installs for the current user only.** It goes into `%LOCALAPPDATA%\Programs\Dictator`, with
  no administrator rights and no UAC prompt.
- Settings and models live in `%LOCALAPPDATA%\Dictator`.
- Open at login uses the per-user `HKCU\…\Run` key.
- Nothing is written machine-wide, and every Windows user has their own copy and models.
- Updates install from the tray menu (**Install Update**). The installer is signed with the
  Tauri updater key, and the signature is checked against the public key built into the app.

Apps running as administrator can't be typed into by a normal-user app (Windows UIPI).
Dictator says so instead of failing silently.

## Models

You choose on first launch. Nothing downloads until then, and only the chosen model downloads.

| Model | Size | Runtime | Languages |
| --- | --- | --- | --- |
| NVIDIA Parakeet TDT 0.6B v2, int8 ONNX (recommended) | 661 MB | sherpa-onnx / ONNX Runtime, CPU | English |
| Whisper Large v3 Turbo, ggml Q5_0 | 574 MB | whisper.cpp, CPU (Vulkan optional) | Automatic or English |
| S1-mini by Superwhisper, Q4_K_M (optional cleanup) | 484 MB | llama.cpp, CPU | English |

These are the Windows-native builds of the macOS models. The macOS app uses Core ML conversions,
which don't run on Windows. Phonon-2 is not offered: only an unofficial ONNX export exists.

Every file is pinned in [`model-manifest.txt`](model-manifest.txt):
- an immutable Hugging Face commit;
- its SHA-256;
- its size.

The manifest ships inside the install. Downloads resume, and are verified before use.

## Build and test

You need Rust (the version is pinned in `rust-toolchain.toml`).

On Windows you also need:
- the MSVC build tools;
- CMake.

Then:

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p dictator-app                    # the app (Windows; on macOS it runs without typing)
```

The model end-to-end tests in `dictator-engines` need two things, and are skipped otherwise:
- **Models:** `scripts/fetch-model.ps1 <model-id>`, or `fetch-model.sh`.
- **Synthesised speech:** macOS `say`.

To build the installer:

```powershell
$env:SHERPA_ONNX_ARCHIVE_DIR = pwsh crates/dictator-engines/scripts/fetch-sherpa-onnx.ps1 win-x64
cargo install tauri-cli --version =2.12.4 --locked
cd crates/dictator-app
cargo build --release --locked
cargo tauri build --bundles nsis   # see release.yml for the llama.cpp DLL resources
```

How the native code is linked:
- Everything links the **static C runtime** (`.cargo/config.toml`), because sherpa-onnx's prebuilt
  libraries require it.
- llama.cpp is linked **dynamically**: its ggml would clash with whisper.cpp's. Its DLLs (`llama*.dll`,
  `ggml*.dll`) ship next to `Dictator.exe`. See [`dictator-engines`](crates/dictator-engines/README.md).

## Architecture

```
crates/
  dictator-core      Pure logic, ported from the Swift DictationCore:
                     - the DictationController state machine (events in, effects out);
                     - shortcut chords, smart joining, undo history, text chunking;
                     - the capture buffer, version, What's New and update logic;
                     - the ports (traits).
  dictator-cleanup   CleaningTranscriber and S1-mini's prompt format (ADR 0011).
  dictator-models    The manifest, the resumable verified installer, and the model catalog.
  dictator-engines   The only crate that touches model runtimes: sherpa-onnx, whisper-rs and llama-cpp-2.
  dictator-audio     The cpal microphone, resampled to 16 kHz mono, with pre-roll.
  dictator-windows   The Win32 adapters, with pure helpers tested anywhere:
                     - low-level keyboard and mouse hooks;
                     - the HotkeyListener;
                     - SendInput typing;
                     - UI Automation reading the text before the cursor.
  dictator-app       The Tauri shell: tray, settings, picker, overlay, What's New, updater.
                     - dictation.rs runs the controller's effects on one thread.
                     - app.rs owns settings and models.
```

Security notes that differ from macOS:
- **Hotkey:** a low-level keyboard hook (`WH_KEYBOARD_LL`) sees every key. Each one is compared in
  memory against the shortcut and dropped; nothing is stored or logged. The shortcut's keys are
  withheld from other apps. Our own typed keys are tagged and ignored.
- **Typing:** synthetic Unicode `SendInput` events, never the clipboard. Output is a single line,
  so dictation can never press Enter and submit a form.
- **Context:** UI Automation reads up to 64 characters before the cursor, only to space and
  capitalise the dictation. Password fields are skipped.
- **Audio** stays in memory, capped at 10 minutes. Transcripts are never logged or written to disk.

## Before the first release

1. Generate the updater key pair:
   ```sh
   cargo tauri signer generate -w ~/.tauri/dictator.key
   ```
2. Put the public key in `crates/dictator-app/tauri.conf.json` under `plugins.updater.pubkey`.
   The release job fails while the placeholder is there.
3. Store the private key and its password as the repository secrets `TAURI_SIGNING_PRIVATE_KEY`
   and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
4. Make the quality gate's `windows` check required on `main`.
5. Authenticode signing is not set up yet, so SmartScreen warns on first run until it is.
