# 9. ModelLibrary owns the model handover

- Status: Accepted
- Date: 2026-10-18
- Amends: [ADR 0007](0007-model-library.md) (activation and `inUse`)

## Context

ADR 0007 made `ModelLibrary` own the model lifecycle, but the handover to the engine was
immediate and unprepared: `engine.use(makeTranscriber(...))` swapped in a transcriber that had
never been loaded, set `inUse`, and called `onActivated` (`DictationController.loadModel()`) on
every switch. Three bugs followed:

1. Switching models while recording called `loadModel()`, whose first act is
   `state = .loadingModel` — wiping the in-progress recording.
2. A model that failed to load (bad or incomplete Core ML files) had already replaced the working
   one, so dictation broke until the user switched back by hand. The failure only surfaced later,
   inside the controller, as a generic unavailable state.
3. `inUse` was set before the load, so it claimed a model was serving that had never loaded, and
   the `guard inUse != model` early return made `retry()` a no-op in cases where a retry was the
   only way forward.

## Decision

Design B: the library loads before it commits, and the commit is atomic.

- `ActiveTranscriber` is now a `final class … Sendable` over a `Mutex<Slot>`
  (`.empty(reason:)` / `.serving(engine)`). `use(_:)` is synchronous and serves at once;
  `unavailable(_:)` records a reason only while empty. `prepare`/`transcribe` forward when
  serving, and throw the recorded reason (or `NoModel`) while empty.
- `ModelLibrary.activate` downloads as before, builds `next = makeTranscriber(model, folder)` and
  `try await next.prepare()` *before* committing. On commit — with no `await` between the
  still-selected guard and the swap — it sets `engine.use(next)`, `inUse = model`,
  `loadFailure = nil`, and calls `onActivated()` only if nothing was serving before. A switch
  while serving never touches the controller, so a recording is untouched.
- A load failure while still selected sets `loadFailure`. If a model is serving, it keeps
  serving and the controller is untouched; `statusText` reads "Couldn't load <model>: <reason>
  Still using <other>." If nothing is serving, the library calls `engine.unavailable(message)`
  and then `onActivated()`, so the controller reports the real reason. `canRetry` is true for a
  failed download or a load failure, and `retry()` re-runs activation for both.
- `inUse` mirrors the engine: it is set only on a successful commit. Choosing a different model
  clears `loadFailure`; choosing back to the serving model while a new one loads drops the new
  one at its commit guard.

### Rejected: A. A switch ticket in the controller

Design A gave `DictationController` a `switchTo` ticket/token API so the controller itself could
ignore stale activations and avoid leaving `.recording`. That puts model policy in the controller,
widens `Transcribing` for every platform that shares DictationCore, and still leaves the engine
swapped before the load was proven. Loading before committing fixes all three bugs at the seam
that already owns model choice, with no DictationCore change.

## Consequences

- Dictation is no longer interrupted by switching models, including mid-recording.
- A failed load keeps the previous model serving and offers Retry in the menu and Settings.
- The first-load failure message is the real load reason, not "choose a model".
- Two model engines can briefly be loaded at once during a switch; the old one is released when
  the library drops it after the commit.
- Device check still needed: dictate while switching models in Settings.
