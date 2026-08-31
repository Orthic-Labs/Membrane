#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import hubReleaseConfig from "../../apps/membrane-hub/right-release.config.mjs";
import { RUNTIME_SPECS } from "../../apps/membrane-hub/scripts/runtime-inventory.mjs";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const sourcePaths = [
  "apps/membrane-hub/right-release.config.mjs",
  "apps/membrane-hub/scripts/runtime-inventory.mjs",
  "apps/membrane-hub/scripts/release-assets.mjs",
  "apps/membrane-hub/scripts/write-release-manifest.mjs",
  "apps/membrane-hub/scripts/write-windows-release-evidence.mjs",
];
const dirty = execFileSync("git", ["-C", repoRoot, "status", "--porcelain", "--", ...sourcePaths], { encoding: "utf8" }).trim();
const releaseSources = [JSON.stringify(hubReleaseConfig.targets), JSON.stringify(hubReleaseConfig.buildInputs), JSON.stringify(RUNTIME_SPECS)];

assert.equal(hubReleaseConfig.schema, 1, "Hub release config schema must be 1");
assert.equal(hubReleaseConfig.app, "membrane-hub", "Membrane Hub must own release config");
assert.deepEqual(Object.keys(hubReleaseConfig.targets), ["mac", "win"], "current release identity must cover macOS & Windows");
assert.ok(hubReleaseConfig.targets?.mac?.signed, "Hub macOS target must be signed");
assert.equal(hubReleaseConfig.targets.mac.cargoTarget, "aarch64-apple-darwin", "Hub macOS target triple drifted");
assert.ok(hubReleaseConfig.targets.mac.artifacts.some((path) => /Membrane Hub_.*_aarch64\.dmg$/.test(path)), "Hub macOS installer artifact missing");
assert.ok(hubReleaseConfig.targets?.win?.signed, "Hub Windows target must be signed");
assert.ok(hubReleaseConfig.targets.win.artifacts.some((path) => /Membrane[_ ]Hub_.*_x64-setup\.exe$/.test(path)), "Hub Windows installer artifact missing");
assert.deepEqual(RUNTIME_SPECS.filter(({ delivery }) => delivery === "externalBin").map(({ component }) => component).sort(), ["cortex", "membrane", "membrane-daemon", "membrane-tray"], "Hub sidecar identity drifted");
assert.ok(!releaseSources.some((value) => /cortex-service|crypt-service|orthic(?:[_-]manifest)?/i.test(value)), "Hub release identity contains retired runtime assets");
assert.equal(dirty, "", `release identity requires clean Hub release sources:\n${dirty}`);

console.log("release identity OK: Membrane Hub release config + macOS/Windows runtime inventory");
