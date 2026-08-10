#!/usr/bin/env node
// stage-binaries.mjs — explicit pre-`tauri build` staging hook (U8).
// Reuses the same ORTHIC_PRODUCT_BINARIES_DIR mechanism that build-frontend.mjs
// uses for local dev, but as an isolated step the release lane can invoke before
// `tauri build` so packaging never reaches across repo boundaries (R-12/I-7).
//
// Usage: node scripts/stage-binaries.mjs [--check]
//   --check: fail if no staged binaries found (release lane), otherwise warn (dev).
import { chmodSync, cpSync, existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const targets = {
  "darwin-arm64": "aarch64-apple-darwin",
  "darwin-x64": "x86_64-apple-darwin",
  "win32-x64": "x86_64-pc-windows-msvc",
  "win32-arm64": "aarch64-pc-windows-msvc",
};
const target = process.env.TAURI_ENV_TARGET_TRIPLE || targets[`${process.platform}-${process.arch}`];
if (!target) throw new Error(`unsupported sidecar target: ${process.platform}-${process.arch}`);

const stagingRoot = process.env.ORTHIC_PRODUCT_BINARIES_DIR
  ? fileURLToPath(new URL(process.env.ORTHIC_PRODUCT_BINARIES_DIR, import.meta.url))
  : join(fileURLToPath(new URL("../", import.meta.url)), "..", "orthic-product-binaries", target);

const binaries = fileURLToPath(new URL("../src-tauri/binaries/", import.meta.url));
mkdirSync(binaries, { recursive: true });

const sidecars = ["crypt", "crypt-service", "membrane", "cortex", "cortex-service"];
let staged = 0;
for (const name of sidecars) {
  const suffix = target.includes("windows") ? ".exe" : "";
  const sources = [join(stagingRoot, `${name}${suffix}`), join(stagingRoot, `${name}-${target}${suffix}`)];
  const src = sources.find(existsSync);
  if (!src) continue;
  const dest = join(binaries, `${name}-${target}${suffix}`);
  cpSync(src, dest);
  if (process.platform !== "win32") chmodSync(dest, 0o755);
  console.log(`[orthic:stage] ${name} ← ${src}`);
  staged++;
}

const check = process.argv.includes("--check");
if (staged === 0) {
  const msg = `[orthic:stage] no staged sidecars in ${stagingRoot} for ${target}`;
  if (check) throw new Error(`${msg} — release requires binaries`);
  console.warn(`${msg} — dev build without binaries (release will require them)`);
} else {
  console.log(`[orthic:stage] staged ${staged} binaries to ${binaries}`);
}
