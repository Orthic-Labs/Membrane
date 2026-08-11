#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export function platformFor(value = process.platform) {
  if (value === "darwin") return "mac";
  if (value === "win32") return "win";
  throw new Error(`unsupported add-on platform: ${value}`);
}

export function adoptionArgs({
  platform = platformFor(),
  lock = resolve(repoRoot, "product-addons/membrane.lock.json"),
  output = resolve(repoRoot, "src-tauri/addons"),
  source = process.env.ORTHIC_ADDON_SOURCE,
} = {}) {
  const args = ["exec", "right-release", "addon", "adopt", "--lock", lock, "--platform", platform, "--output", output];
  if (source) args.push("--source", resolve(source));
  return args;
}

export function stageMembrane(options = {}) {
  const lock = options.lock || resolve(repoRoot, "product-addons/membrane.lock.json");
  if (!existsSync(lock)) {
    throw new Error(`missing published Membrane add-on lock: ${lock}`);
  }
  const result = spawnSync(process.platform === "win32" ? "pnpm.cmd" : "pnpm", adoptionArgs({ ...options, lock }), {
    cwd: repoRoot,
    env: process.env,
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Membrane add-on adoption failed with exit ${result.status}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  stageMembrane();
}
