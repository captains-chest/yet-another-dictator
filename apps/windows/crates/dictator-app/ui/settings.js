import { bindView, invoke, formatBytes, renderKeys, fillSelect, optional } from "./common.js";

const $ = (id) => document.getElementById(id);

function modelRow(model, view) {
  const row = document.createElement("div");
  row.className = "row";
  const label = document.createElement("span");
  label.className = "label";
  label.textContent = model.name;
  if (model.recommended) {
    const badge = document.createElement("span");
    badge.className = "badge";
    badge.textContent = "Recommended";
    label.append(badge);
  }
  const detail = document.createElement("span");
  detail.className = "muted";
  detail.textContent = ` · ${formatBytes(model.bytes)} · ${model.note}`;
  label.append(detail);
  if (model.error) {
    const error = document.createElement("div");
    error.className = "error";
    error.textContent = model.error;
    label.append(error);
  }
  row.append(label);

  if (model.progress !== null) {
    const progress = document.createElement("progress");
    progress.value = model.progress;
    row.append(progress);
  } else if (model.id === view.selectedModel && model.installed) {
    const inUse = document.createElement("span");
    inUse.className = "muted";
    inUse.textContent = "In use";
    row.append(inUse);
  } else {
    const use = document.createElement("button");
    use.textContent = model.installed ? "Use" : model.error ? "Retry" : "Download";
    use.onclick = () => invoke("select_model", { id: model.id });
    row.append(use);
  }
  if (model.installed && model.id !== view.selectedModel) {
    const remove = document.createElement("button");
    remove.className = "danger";
    remove.textContent = "Delete";
    remove.onclick = () => invoke("delete_model", { id: model.id });
    row.append(remove);
  }
  return row;
}

function render(view) {
  $("version").textContent = `Version ${view.version}`;

  const recordingDictate = view.recordingShortcut === "dictate";
  const recordingUndo = view.recordingShortcut === "undo";
  renderKeys($("shortcut"), recordingDictate ? view.recordingHeld : view.shortcut, { recording: recordingDictate });
  renderKeys($("undo-shortcut"), recordingUndo ? view.recordingHeld : view.undoShortcut ?? [], { recording: recordingUndo });
  $("record-shortcut").textContent = recordingDictate ? "Press keys…" : "Change…";
  $("record-undo").textContent = recordingUndo ? "Press keys…" : "Set…";
  $("clear-undo").disabled = !view.undoShortcut;
  $("shortcut-error").textContent = view.shortcutError ?? "";

  $("models").replaceChildren(...view.models.map((model) => modelRow(model, view)));
  fillSelect($("language"), view.languages.map(({ code, name }) => ({ value: code, label: name })), view.language);
  $("language").disabled = view.languages.length < 2;
  $("language-note").textContent = view.languageNote ?? "";

  const cleanup = view.cleanup;
  $("cleanup-enabled").checked = cleanup.enabled;
  $("cleanup-progress").hidden = cleanup.progress === null;
  $("cleanup-progress").value = cleanup.progress ?? 0;
  fillSelect($("cleanup-styling"), cleanup.stylings.map(({ id, name }) => ({ value: id, label: name })), cleanup.styling);
  $("cleanup-styling").disabled = !cleanup.enabled;
  $("cleanup-description").textContent =
    "Removes filler words like \u201Cum\u201D, applies spoken corrections (\u201CFriday, no wait, Thursday\u201D), and formats numbers, dates, times and email addresses. English only. " +
    `Off by default; turning it on downloads the model (${formatBytes(cleanup.bytes)}, from Hugging Face, verified by SHA-256). It runs on this PC. ` +
    "If cleanup is still loading, fails, or takes too long, the raw transcript is typed instead. Undo removes exactly what was typed.";
  $("cleanup-error").textContent = cleanup.error ?? "";
  $("cleanup-delete").hidden = !cleanup.installed || cleanup.enabled;

  const microphones = [{ value: null, label: "System default" }, ...view.microphones.map(({ id, name }) => ({ value: id, label: name }))];
  if (view.microphone && !view.microphones.some(({ id }) => id === view.microphone)) {
    microphones.push({ value: view.microphone, label: `${view.microphoneName ?? "Chosen microphone"} (not connected)` });
  }
  fillSelect($("microphone"), microphones, view.microphone);
  $("microphone-note").textContent = view.microphoneConnected
    ? "Used only for dictation. Calls and other apps keep the input set in Windows Sound settings."
    : "Not connected. Dictation uses the system default input until it is.";
  $("keep-ready").checked = view.keepMicrophoneReady;

  $("open-at-login").checked = view.openAtLogin;
  $("check-updates").checked = view.checkForUpdates;
}

$("record-shortcut").onclick = () => invoke("record_shortcut", { kind: "dictate" });
$("record-undo").onclick = () => invoke("record_shortcut", { kind: "undo" });
$("clear-undo").onclick = () => invoke("clear_undo_shortcut");
$("language").onchange = (event) => invoke("set_language", { code: optional(event.target.value) });
$("cleanup-enabled").onchange = (event) => invoke("set_cleanup_enabled", { enabled: event.target.checked });
$("cleanup-styling").onchange = (event) => invoke("set_cleanup_styling", { styling: event.target.value });
$("cleanup-delete").onclick = () => invoke("delete_cleanup_model");
$("microphone").onchange = (event) => invoke("set_microphone", { id: optional(event.target.value) });
$("keep-ready").onchange = (event) => invoke("set_keep_microphone_ready", { on: event.target.checked });
$("open-at-login").onchange = (event) => invoke("set_open_at_login", { on: event.target.checked });
$("check-updates").onchange = (event) => invoke("set_check_for_updates", { on: event.target.checked });

bindView(render);
