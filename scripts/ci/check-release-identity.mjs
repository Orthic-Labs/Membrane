#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import addonConfig from "../../right-addon.config.mjs";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const sourcePaths = [
  "right-addon.config.mjs", "package.json", "pnpm-lock.yaml", "engine",
  "dist/install/assets/membrane-tab-icon.png", "LICENSE", "docs/legal/EULA.txt", "docs/legal/PRIVACY.md", "docs/legal/THIRD-PARTY-NOTICES.txt",
];
const dirty = execFileSync("git", ["-C", repoRoot, "status", "--porcelain", "--", ...sourcePaths], { encoding: "utf8" }).trim();
const roleBuilds = Object.values(addonConfig.targets).flatMap(({ build }) => build.args);
const releaseFiles = Object.values(addonConfig.targets).flatMap(({ files }) => files.flatMap(({ name, source }) => [name, source]));

assert.equal(addonConfig.schema, 1, "add-on config schema must be 1");
assert.equal(addonConfig.kind, "headless-addon", "release identity requires a headless add-on");
assert.equal(addonConfig.addon, "membrane", "release identity requires membrane add-on");
assert.equal(addonConfig.consumer.contract, "membrane-product-v1", "release identity must use the Membrane product contract");
assert.equal(addonConfig.consumer.productId, "membrane", "release identity requires Membrane product ID");
assert.deepEqual(addonConfig.consumer.serviceStop, ["SIGTERM"], "add-on consumer stop contract drifted");
assert.ok(roleBuilds.includes("membrane"), "add-on must build membrane command role");
assert.ok(roleBuilds.includes("cortex-service"), "add-on must build cortex-service service role");
assert.ok(!roleBuilds.some((value) => /crypt|orthic/i.test(value)), "release identity cannot build retired Crypt or Orthic roles");
assert.ok(!releaseFiles.some((value) => /crypt|orthic/i.test(value)), "release identity cannot package retired Crypt or Orthic assets");
assert.equal(dirty, "", `release identity requires clean add-on sources:\n${dirty}`);

console.log(`release identity OK: ${addonConfig.addon}@${addonConfig.version} command=membrane service=cortex-service`);
