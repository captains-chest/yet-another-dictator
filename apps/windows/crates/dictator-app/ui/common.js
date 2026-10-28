// Thin wrappers over the Tauri globals (app.withGlobalTauri), shared by every window.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

export { invoke, listen };

/** Renders the app view now and whenever the backend reports a change. */
export async function bindView(render) {
  await listen("view-changed", (event) => render(event.payload));
  render(await invoke("view"));
}

export function formatBytes(bytes) {
  const megabytes = bytes / 1_000_000;
  return megabytes >= 1000 ? `${(megabytes / 1000).toFixed(1)} GB` : `${Math.round(megabytes)} MB`;
}

export function renderKeys(container, labels, { recording = false } = {}) {
  container.replaceChildren(
    ...(labels.length ? labels : ["None"]).map((label) => {
      const key = document.createElement("kbd");
      key.textContent = label;
      key.classList.toggle("recording", recording);
      return key;
    }),
  );
}

export function fillSelect(select, options, selected) {
  select.replaceChildren(
    ...options.map(({ value, label }) => {
      const option = document.createElement("option");
      option.value = value ?? "";
      option.textContent = label;
      option.selected = (value ?? "") === (selected ?? "");
      return option;
    }),
  );
}

/** Empty string from a select means "none" (system default, automatic language). */
export const optional = (value) => (value === "" ? null : value);
