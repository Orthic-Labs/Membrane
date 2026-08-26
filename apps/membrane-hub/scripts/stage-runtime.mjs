// Installed runtime input: native sidecars/contracts plus independently owned
// Blueprint runtime. One installer makes Blueprint available; Hub only owns
// resident service/watcher lifetime.
import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { addInstalledBlueprintInventory, writeRuntimeInventory } from "./runtime-inventory.mjs";

const hub = fileURLToPath(new URL("../", import.meta.url));
const runtime = join(hub, "src-tauri", "runtime");
const blueprintStager = fileURLToPath(new URL("../../../blueprint/scripts/release/stage-runtime.mjs", import.meta.url));

export function stageHubRuntime({ hubDir = hub, runtimeDir = runtime } = {}) {
  rmSync(runtimeDir, { recursive: true, force: true }); mkdirSync(runtimeDir, { recursive: true });
  writeRuntimeInventory({ hubDir, runtimeDir });
  const result = spawnSync(process.execPath, [blueprintStager, "--out", join(runtimeDir, "blueprint")], { cwd: fileURLToPath(new URL("../../../blueprint", import.meta.url)), stdio: "inherit", windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Blueprint runtime staging failed with exit ${result.status}`);
  return addInstalledBlueprintInventory({ runtimeDir });
}

if (fileURLToPath(import.meta.url) === resolve(process.argv[1] || "")) {
  const action = process.argv[2] || "hub";
  if (action === "hub") stageHubRuntime(); else throw new Error("usage: stage-runtime.mjs [hub]");
}
