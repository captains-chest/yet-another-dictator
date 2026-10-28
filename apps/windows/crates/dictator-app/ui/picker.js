import { bindView, invoke, formatBytes } from "./common.js";

const $ = (id) => document.getElementById(id);
let chosen = null;
let latest = null;

function render(view) {
  latest = view;
  chosen ??= view.models.find((model) => model.recommended)?.id ?? view.models[0]?.id;
  $("models").replaceChildren(
    ...view.models.map((model) => {
      const label = document.createElement("label");
      label.className = "model";
      const radio = document.createElement("input");
      radio.type = "radio";
      radio.name = "model";
      radio.checked = model.id === chosen;
      radio.onchange = () => {
        chosen = model.id;
        render(latest);
      };
      const text = document.createElement("div");
      const name = document.createElement("strong");
      name.textContent = model.name;
      text.append(name);
      if (model.recommended) {
        const badge = document.createElement("span");
        badge.className = "badge";
        badge.textContent = "Recommended";
        text.append(badge);
      }
      const detail = document.createElement("p");
      detail.className = "muted";
      detail.textContent = `${formatBytes(model.bytes)} download · ${model.note} · ${model.source}`;
      text.append(detail);
      label.append(radio, text);
      return label;
    }),
  );
  const model = view.models.find(({ id }) => id === chosen);
  $("download").textContent = model ? `Download ${model.name} (${formatBytes(model.bytes)})` : "Download";
}

$("download").onclick = () => invoke("choose_first_model", { id: chosen });
bindView(render);
