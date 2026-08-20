// Native prepackage input: Blueprint's canonical release stager owns its
// Node/dependency closure; inventory only hashes its generated result.
import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { writeRuntimeInventory } from "./runtime-inventory.mjs";

const hub = fileURLToPath(new URL("../", import.meta.url));
const runtime = join(hub, "src-tauri", "runtime");
const blueprint = resolve(hub, "../../blueprint/scripts/release/stage-runtime.mjs");

export function stageHubRuntime({ hubDir = hub, runtimeDir = runtime } = {}) {
  const blueprintOut = join(runtimeDir, "blueprint");
  rmSync(blueprintOut, { recursive: true, force: true }); mkdirSync(runtimeDir, { recursive: true });
  const result = spawnSync(process.execPath, [blueprint, "--out", blueprintOut], { cwd: resolve(hubDir, "../../blueprint"), stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Blueprint runtime stage failed with exit ${result.status}`);
  return writeRuntimeInventory({ hubDir, runtimeDir });
}

if (fileURLToPath(import.meta.url) === resolve(process.argv[1] || "")) stageHubRuntime();
