#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import hubReleaseConfig from "../../apps/membrane-hub/right-release.config.mjs";
import { RUNTIME_SPECS } from "../../apps/membrane-hub/scripts/runtime-inventory.mjs";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const sourcePaths = [
  "apps/membrane-hub/right-release.config.mjs",
  "apps/membrane-hub/scripts/runtime-inventory.mjs",
  "apps/membrane-hub/scripts/release-assets.mjs",
  "apps/membrane-hub/scripts/write-release-manifest.mjs",
];
const dirty = execFileSync("git", ["-C", repoRoot, "status", "--porcelain", "--", ...sourcePaths], { encoding: "utf8" }).trim();
const configText = readFileSync(new URL("../../apps/membrane-hub/right-release.config.mjs", import.meta.url), "utf8");
const releaseSources = [configText, JSON.stringify(RUNTIME_SPECS)];

assert.equal(hubReleaseConfig.schema, 1, "Hub release config schema must be 1");
assert.equal(hubReleaseConfig.app, "membrane-hub", "Membrane Hub must own release config");
assert.deepEqual(Object.keys(hubReleaseConfig.targets), ["mac"], "current release identity is Mac-only");
assert.ok(hubReleaseConfig.targets?.mac?.signed, "Hub Mac target must be signed");
assert.ok(hubReleaseConfig.targets.mac.artifacts.some((path) => /Membrane Hub_.*\.dmg$/.test(path)), "Hub Mac installer artifact missing");
assert.deepEqual(RUNTIME_SPECS.filter(({ delivery }) => delivery === "externalBin").map(({ component }) => component).sort(), ["cortex", "membrane"], "Hub sidecar identity drifted");
assert.ok(!releaseSources.some((value) => /cortex-service|crypt-service|orthic(?:[_-]manifest)?/i.test(value)), "Hub release identity contains retired runtime assets");
assert.ok(!releaseSources.some((value) => /windows|win32|\.exe\b/i.test(value)), "Hub release identity contains out-of-scope Windows assets");
assert.equal(dirty, "", `release identity requires clean Hub release sources:\n${dirty}`);

console.log("release identity OK: Membrane Hub release config + Mac runtime inventory");
