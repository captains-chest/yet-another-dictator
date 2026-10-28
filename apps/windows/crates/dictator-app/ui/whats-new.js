import { invoke } from "./common.js";

document.getElementById("notes").textContent = await invoke("whats_new");
