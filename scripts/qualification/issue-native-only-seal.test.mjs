import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(new URL("./issue-native-only-seal.mjs", import.meta.url), "utf8");

test("native-only seal issuer is receipt-bound & fail-closed", () => {
  for (const term of [
    "membrane.native-only-seal.v1",
    "membrane.release-evidence.v1",
    "membrane.windows-installed-qualification.v1",
    "signed-version-liveness-durable-state-v1",
    "full-native-upgrade-uninstall-v1",
    "productionInterpreterRows",
    "boundedExternalInterpreterRows",
    "blueprint-bundled-runtime-blueprint/scripts",
    "blueprint-bundled-runtime-blueprint/src",
    "blueprint-bundled-runtime-blueprint/watchman",
    "blueprint-bundled-launchers-blueprint/release",
    "nativeOnlyProcessTree",
    "platform_trust",
    "authenticode",
    "Object.fromEntries(inputs",
    "renameSync",
    "lstatSync",
  ]) assert.ok(source.includes(term), term);
  assert.match(source, /releaseHash !== qualificationHash/);
  assert.match(source, /previous installer is not older than current installer/);
  assert.match(source, /seal already exists/);
  assert.doesNotMatch(source, /spawnSync|execFileSync|cargo|pnpm|tauri/);
});
