# Dictator

Minimal, local-only dictation. Press a shortcut, talk, and the text is typed into whatever field
has focus. Speech is transcribed on your computer by the model you choose; audio never leaves it.

| App | Where | Stack | Install |
| --- | --- | --- | --- |
| **macOS** (Apple silicon) | [`apps/macos`](apps/macos/README.md) | Swift, SwiftUI, Core ML | `Dictator-<version>-macos-arm64.zip` |
| **Windows** (x64) | [`apps/windows`](apps/windows/README.md) | Rust, Tauri | `Dictator-<version>-windows-x64-setup.exe`, per user, no admin |

Both apps live in this repo so a feature lands in both at once. They share the behaviour:
- the same dictation state machine;
- the same tap/hold shortcut rules;
- the same smart joining, undo and cleanup;
- the same SHA-256 model manifest scheme.

The Windows app's `dictator-core` is a line-by-line port of the Swift DictationCore, with the same
tests. A behaviour change goes into both cores (see [ADR 0012](docs/adr/0012-monorepo-and-windows-app.md)).

Shared at the root:
- [CHANGELOG.md](CHANGELOG.md): both apps share version numbers and release notes.
- [LICENSE](LICENSE): MIT.
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
- [GLOSSARY.md](GLOSSARY.md)
- [`docs/adr`](docs/adr): decisions.

Website: [captains-chest.com/dictator](https://captains-chest.com/dictator) · privacy policy:
[captains-chest.com/dictator/privacy](https://captains-chest.com/dictator/privacy).

## CI / Release

**Quality gate** (`.github/workflows/quality-gate.yml`) runs on every push:
- `build-and-test` builds and tests the macOS app.
- `windows` runs `cargo fmt`, `clippy -D warnings` and `cargo test` for the Windows workspace on
  `windows-latest`.

**Release** (`.github/workflows/release.yml`): publishing a GitHub release builds both apps and attaches:
- the signed, notarized macOS zip and its `.sha256`;
- the Windows per-user installer and its `.sha256`;
- `latest.json`, the feed the Windows in-app updater reads.

`workflow_dispatch` is a dry run that uploads artifacts only.

## License

MIT © 2026 CaptainDuckay. Third-party components and the downloaded models are credited in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
