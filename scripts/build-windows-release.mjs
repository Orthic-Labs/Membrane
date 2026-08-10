// build-windows-release.mjs — never assembles/writes the release manifest.
// Bracket: writer pre-check -> tauri build -> release-assets check-built ->
// writer pre-check -> workspace right-release sign-windows -> installer rename.

import { spawnSync } from "node:child_process";
import { readFileSync, renameSync, rmSync } from "node:fs";

const version = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;
const directory = `${process.env.CARGO_TARGET_DIR || "src-tauri/target"}/release/bundle/nsis/`;
const source = `${directory}Membrane Hub_${version}_x64-setup.exe`;
const destination = `${directory}Membrane_${version}_x64-setup.exe`;

// Preauthority prepares unsigned staging; external RightKit signing writes paired signed/, then release-assets finalize verifies it.
run("node", ["scripts/write-release-manifest.mjs", "check", "--require-committed"]);
run("pnpm", ["exec", "tauri", "build", "--bundles", "nsis"]);
run("node", ["scripts/release-assets.mjs", "check-built", "--platform", "win"]);
run("node", ["scripts/write-release-manifest.mjs", "check", "--require-committed"]);
rmSync(destination, { force: true });
renameSync(source, destination);

function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with exit ${result.status}`);
}
