// D-S03/D-S04 — Hub owns process lifecycle; no OS service registration.
// This file retains install-related helpers but forbids OS-registration paths.
// The sole headless entry point is `cortex service run` (foreground).

import { mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { writeProductManifest } from "../lib/init/manifest.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const CLI_SCRIPT = join(SCRIPT_DIR, "..", "..", "scripts", "cortex.mjs");

// D-S03: no OS service registration under any configuration.
export function installService({ root = process.cwd(), logDir = null, dryRun = false } = {}) {
  const logs = logDir ?? join(homedir(), ".cortex", "logs");
  if (!dryRun) mkdirSync(logs, { recursive: true });
  const cliScript = resolve(CLI_SCRIPT);
  const serviceStart = [process.execPath, cliScript, "service", "run", "--root", resolve(root)];
  const serviceStop = [process.execPath, cliScript, "service", "stop"];
  const target = null; // no OS target — Hub spawns as child
  const body = `# Hub-owned lifecycle (D-S03): run \`cortex service run\` or let Hub spawn:\n# ${serviceStart.join(" ")}\n`;
  if (dryRun) return { platform: process.platform, target, body, serviceStart, serviceStop, forbidden: "OS registration forbidden per D-S03" };
  const manifest = writeProductManifest({ installRoot: resolve(root) });
  return { platform: process.platform, target, installed: true, manifest: manifest.path, serviceStart, serviceStop, note: "Product manifest installed; OS service registration forbidden per D-S03 — Hub owns lifecycle" };
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

export function foregroundRunArgs(root = process.cwd()) {
  return [process.execPath, resolve(CLI_SCRIPT), "service", "run", "--root", resolve(root)];
}
