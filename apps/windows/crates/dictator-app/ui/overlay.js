import { listen } from "./common.js";

const bars = [...document.querySelectorAll(".meter span")];
// Middle bars move most, like a voice meter.
const weights = [0.5, 0.8, 1, 0.8, 0.5];
const maxBarPx = 18;
const minBarPx = 4;

await listen("overlay", ({ payload }) => {
  document.body.classList.toggle("transcribing", payload === "transcribing");
  document.getElementById("status").textContent = payload === "transcribing" ? "Transcribing" : "Listening";
  if (payload === "transcribing") bars.forEach((bar) => (bar.style.height = ""));
});

await listen("level", ({ payload: level }) => {
  if (document.body.classList.contains("transcribing")) return;
  bars.forEach((bar, index) => {
    bar.style.height = `${minBarPx + (maxBarPx - minBarPx) * Math.min(1, level * weights[index] * 1.5)}px`;
  });
});
