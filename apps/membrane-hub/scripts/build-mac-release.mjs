// build-mac-release.mjs — RightRelease phase hooks; never writes release manifest.
// prepare: writer pre-check -> sidecars -> source receipt -> signed Tauri app/DMG -> check-built.
// package: finalize -> check-packaged -> writer pre-check.
// `finalize` must run here: it is the only thing that writes this platform's
// assets.json, and check-packaged (the very next step) hard-requires that
// receipt to already exist ("missing finalized receipt"). Unlike `prepare`
// (a snapshot of the pre-build sidecars, deliberately taken before this
// script starts so check-built can prove the rebuild was reproducible),
// finalize captures the packaged, signed .app that only exists once `tauri
// build` above has completed — there is no external point before or after
// this synchronous script where a human/CI step could inject it instead.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolveManagedCargoTarget } from "./lib/target-root.mjs";

const version = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;
const target = "aarch64-apple-darwin";
// A managed build owns the target root, so src-tauri/target is not a valid
// assumption; `cargo metadata` against the src-tauri manifest is the sole
// source of truth for where the build actually lands (see lib/target-root.mjs).
const manifestPath = fileURLToPath(new URL("../src-tauri/Cargo.toml", import.meta.url));
const bundleRoot = resolveManagedCargoTarget(manifestPath);
const dmg = `${bundleRoot}/${target}/release/bundle/dmg/Membrane Hub_${version}_aarch64.dmg`;
const env = {
  ...process.env,
  APPLE_SIGNING_IDENTITY: process.env.APPLE_SIGNING_IDENTITY || "Developer ID Application: Adrian D'souza (6KLGD3LLKF)",
  TAURI_ENV_TARGET_TRIPLE: target,
};

const phase = process.argv[2];
if (phase === "prepare") {
  run("node", ["scripts/write-release-manifest.mjs", "check", "--require-committed"], env);
  run("pnpm", ["run", "build"], env);
  run("node", ["scripts/release-assets.mjs", "prepare", "--platform", "mac"], env);
  run("node", ["scripts/stage-runtime.mjs"], env);
  run("node", ["scripts/sign-macos-runtime.mjs"], env);
  run("pnpm", ["exec", "tauri", "build", "--target", target, "--bundles", "app,dmg"], { ...env, MEMBRANE_SIDECARS_READY: "1" });
  run("node", ["scripts/release-assets.mjs", "check-built", "--platform", "mac"], env);
  if (!existsSync(dmg)) throw new Error(`missing signed DMG: ${dmg}`);
} else if (phase === "package") {
  if (!existsSync(dmg)) throw new Error(`missing RightKit-signed DMG: ${dmg}`);
  run("node", ["scripts/release-assets.mjs", "finalize", "--platform", "mac"], env);
  run("node", ["scripts/release-assets.mjs", "check-packaged", "--platform", "mac"], env);
  run("node", ["scripts/write-release-manifest.mjs", "check", "--require-committed"], env);
} else {
  throw new Error("usage: node scripts/build-mac-release.mjs <prepare|package>");
}

function run(command, args, commandEnv) {
  const result = spawnSync(command, args, { stdio: "inherit", env: commandEnv });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with exit ${result.status}`);
}
