// D-S03/D-S04 — Hub owns process lifecycle; no OS service registration.
// This file retains install-related helpers but forbids OS-registration paths.
// The sole headless entry point is `cortex service run` (foreground).

import { mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const WATCH_SCRIPT = join(SCRIPT_DIR, "..", "scripts", "cortex-watch.mjs");

// D-S03: no OS service registration under any configuration.
export function installService({ root = process.cwd(), logDir = null, dryRun = false } = {}) {
  const logs = logDir ?? join(homedir(), ".cortex", "logs");
  if (!dryRun) mkdirSync(logs, { recursive: true });
  const watchScript = resolve(WATCH_SCRIPT);
  // serviceStart argv that the manifest (v4-U56) names — direct foreground target
  const serviceStart = [process.execPath, watchScript, "start"];
  const serviceStop = [process.execPath, watchScript, "stop"];
  const target = null; // no OS target — Hub spawns as child
  const body = `# Hub-owned lifecycle (D-S03): run \`cortex service run\` or let Hub spawn:\n# ${serviceStart.join(" ")}\n`;
  if (dryRun) return { platform: process.platform, target, body, serviceStart, serviceStop, forbidden: "OS registration forbidden per D-S03" };
  return { platform: process.platform, target, installed: false, serviceStart, serviceStop, note: "OS service registration forbidden per D-S03 — use cortex service run; Hub owns lifecycle" };
}

export function serviceTarget() {
  // D-S03: no target — kept for API compat, always null
  return null;
}

export function serviceControlPlan(action) {
  throw Object.assign(new Error(`OS service control forbidden per D-S03 — use cortex service run (requested ${action})`), { code: "os_registration_forbidden" });
}

export function controlService(action) {
  throw Object.assign(new Error(`OS service control forbidden per D-S03 — use cortex service run (requested ${action})`), { code: "os_registration_forbidden" });
}

export function foregroundRunArgs() {
  return [process.execPath, resolve(WATCH_SCRIPT), "start"];
}
