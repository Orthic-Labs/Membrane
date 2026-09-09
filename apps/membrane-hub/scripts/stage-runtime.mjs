// Installed runtime input: native sidecars/contracts only. Stable-current
// installer owns executable identity; Hub owns resident service lifetime.
import { mkdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { writeRuntimeInventory } from "./runtime-inventory.mjs";

const hub = fileURLToPath(new URL("../", import.meta.url));
const runtime = join(hub, "src-tauri", "runtime");

export function stageHubRuntime({ hubDir = hub, runtimeDir = runtime } = {}) {
  rmSync(runtimeDir, { recursive: true, force: true }); mkdirSync(runtimeDir, { recursive: true });
  return writeRuntimeInventory({ hubDir, runtimeDir });
}

if (fileURLToPath(import.meta.url) === resolve(process.argv[1] || "")) {
  const action = process.argv[2] || "hub";
  if (action === "hub") stageHubRuntime(); else throw new Error("usage: stage-runtime.mjs [hub]");
}
