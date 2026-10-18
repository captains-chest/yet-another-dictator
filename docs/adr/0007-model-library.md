# 7. One ModelLibrary owns the speech-model lifecycle

- Status: Accepted
- Date: 2026-10-16
- Builds on: [ADR 0006](0006-choose-the-speech-model.md) (choose the speech model; download nothing until chosen)
- Amended (2026-10-18): [ADR 0009](0009-model-handover.md) changes activation — the new model is loaded before the handover commits, `inUse` is set only on that commit, and switches no longer call `onActivated`.

## Context

After ADR 0006 the model lifecycle was spread over four places: `ModelChoice` (the stored choice),
`ModelStartup` (what to do at launch), `ModelDownload` (a main-actor wrapper around one
`ModelProvisioner`) and `AppModel` (`startModel`, `chooseModel`, `downloadAndLoadModel`,
`deleteModel`). Each piece was shallow and the rules that matter lived in the gaps between them,
where no test could reach. That hid three bugs:

1. `ModelDownload.select` built a new `ModelProvisioner` every time the model changed. Choosing
   A, then B, then A again while A was downloading started a second download of A that wrote the
   same `.part` files as the first.
2. Settings offered Delete for any installed model that was not selected, including the model the
   engine was still transcribing with while the new choice downloaded.
3. Nothing recorded which model the engine was actually using, so the UI could not tell
   "selected" from "loaded".

## Decision

One `@MainActor @Observable final class ModelLibrary` (`Sources/Dictator/ModelLibrary.swift`) owns
the whole lifecycle: the persisted choice (`speechModel` in UserDefaults; an already-installed
model is adopted when unset), the model in use, downloading, activation and deleting.

- One `ModelProvisioner` per model for the library's lifetime, kept in a dictionary, so choosing a
  model again joins its running download.
- `choose(_:)` persists the choice, downloads if missing, and activates only if the model is still
  selected when the download ends: the latest choice wins. Activation is `engine.use`, then the
  controller's `loadModel`, then `inUse` is set.
- `canDelete` is true only for a model that is installed, not selected, not in use and not
  downloading. `delete(_:)` does nothing otherwise.
- `status` and progress reflect only the selected model.
- `start()` with no choice fetches nothing and only lets the controller show "choose a model".

`AppModel` only wires it up: it builds the library with the real transport, engine and transcriber
factory, shows the picker if `needsChoice`, and calls `start()`. Settings, the menu and the picker
read the library directly. `ModelProvisioner`, the UserDefaults key and the disk layout are
unchanged. Abandoned downloads are not cancelled; they finish and stay on disk.

### Rejected: B. A `ModelSwitcher` coordinating shallow protocols

Keeping `ModelChoice` and `ModelDownload` behind small protocols (`ModelChoosing`,
`ModelDownloading`) with a `ModelSwitcher` coordinating them would have made each piece fakeable,
but the bugs above are in the coordination itself. Splitting it across protocols keeps the
interfaces as large as the implementation and the rules still spread out; tests would mostly check
that fakes were called in order.

## Consequences

- The lifecycle is tested through one interface in `DictatorTests` with a fake transport (able to
  hold a fetch), a fake transcriber and the real `ActiveTranscriber`, offline.
- The three bugs above are fixed and covered by tests.
- A downloaded-then-abandoned model takes disk space until the user deletes it in Settings.
