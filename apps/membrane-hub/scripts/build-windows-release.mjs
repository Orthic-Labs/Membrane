// build-windows-release.mjs — never assembles/writes the release manifest.
// Bracket: writer pre-check -> tauri build -> release-assets check-built ->
// copy signed sidecars into staging -> release-assets finalize -> writer
// pre-check -> installer rename.
//
// finalize must run here, mirroring build-mac-release.mjs: it is the only
// thing that writes this platform's assets.json, and there is no other
// synchronous point where a human/CI step could inject it. Unlike mac
// (`tauri build` codesigns the packaged .app in place, so finalize reads the
// bundle directly), Windows sidecar signing happens externally, in place at
// src-tauri/binaries/, before this script runs; finalize instead reads a
// deterministic `signed/` staging copy (release-assets.mjs finalizeWin), so
// this script's only remaining job is snapshotting the post-signing sidecars
// there before calling it. release-assets.mjs's check-packaged has no win
// support (mac-only — see its parseArgs), so unlike the mac bracket this one
// stops at finalize; it does not invent a win check-packaged step.

import { execFileSync, spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, renameSync, rmSync } from "node:fs";
import { basename, join } from "node:path";
import { bundleRoot, parentWorkspaceRoot, repoRootFromCwd, sidecarSourcePath, signedDir, SIDECAR_NAMES, targetTriple } from "./release-assets.mjs";

const version = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;
const directory = `${process.env.CARGO_TARGET_DIR || "src-tauri/target"}/release/bundle/nsis/`;
const source = `${directory}Membrane Hub_${version}_x64-setup.exe`;
const destination = `${directory}Membrane_${version}_x64-setup.exe`;

run("node", ["scripts/write-release-manifest.mjs", "check", "--require-committed"]);
run("pnpm", ["exec", "tauri", "build", "--bundles", "nsis"]);
run("node", ["scripts/release-assets.mjs", "check-built", "--platform", "win"]);
copySignedSidecarsIntoStaging();
run("node", ["scripts/release-assets.mjs", "finalize", "--platform", "win"]);
run("node", ["scripts/write-release-manifest.mjs", "check", "--require-committed"]);
rmSync(destination, { force: true });
renameSync(source, destination);

function copySignedSidecarsIntoStaging() {
  const hubDir = process.cwd();
  const repoRoot = repoRootFromCwd(hubDir);
  const parent = parentWorkspaceRoot(repoRoot);
  const commit = execFileSync("git", ["-C", repoRoot, "rev-parse", "HEAD"], { encoding: "utf8" }).trim();
  const target = targetTriple("win");
  const staging = signedDir(bundleRoot(parent, commit, "win"));
  mkdirSync(staging, { recursive: true });
  for (const name of SIDECAR_NAMES) {
    const from = sidecarSourcePath(hubDir, target, name);
    copyFileSync(from, join(staging, basename(from)));
  }
}

function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with exit ${result.status}`);
}
