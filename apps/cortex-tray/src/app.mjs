import { invoke } from "../vendor/@tauri-apps/api/core.js";
import { trayView } from "./state.mjs";

const byId = (id) => document.getElementById(id);
let lifecycle = "running";

function render(snapshot) {
  const view = trayView(snapshot, lifecycle);
  document.body.dataset.state = view.state;
  byId("state").textContent = view.state;
  byId("summary").textContent = view.summary;
  byId("reason").textContent = view.reason ?? "";
  byId("repository").textContent = view.repository ?? "—";
  byId("repositories").textContent = view.repositories?.length ? view.repositories.join(" · ") : "None enrolled";
  byId("freshness").textContent = view.freshness ?? "—";
  byId("watcher").textContent = view.watcher ?? "—";
}

async function refresh() {
  try { render(await invoke("snapshot")); }
  catch (error) { render({ error: { message: String(error) } }); }
}

async function service(action) {
  lifecycle = action === "start" ? "starting" : "stopping";
  render(null);
  try { await invoke("service_action", { action }); }
  finally { lifecycle = "running"; await refresh(); }
}

byId("explore").onclick = () => invoke("open_explorer");
byId("start").onclick = () => service("start");
byId("stop").onclick = () => service("stop");
byId("startup").onchange = (event) => invoke("set_startup", { enabled: event.target.checked });
byId("hide").onclick = () => invoke("hide_tray");
byId("quit").onclick = () => invoke("quit_app");
byId("startup").checked = await invoke("startup_setting").catch(() => false);
await refresh();
setInterval(refresh, 5000);
