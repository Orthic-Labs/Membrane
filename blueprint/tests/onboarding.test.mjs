import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { applyInitPlan, uninstallInit } from "../src/lib/init/apply.mjs";
import { buildInitPlan } from "../src/lib/init/plan.mjs";
import { readWatchConfig, writeWatchConfig } from "../watchman/supervisor.mjs";

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "cortex-onboarding-repo-"));
  const home = mkdtempSync(join(tmpdir(), "cortex-onboarding-home-"));
  mkdirSync(join(root, ".git"));
  writeFileSync(join(root, "CORTEX-AGENT.md"), "# Existing\n");
  return { root, home, watch: join(home, "watch.json") };
}

test("onboarding enrolls only after explicit watch selection and uninstalls reversibly", () => {
  const { root, home, watch } = fixture();
  const trust = join(import.meta.dirname, "..", "src", "lib", "update", "trusted-update-keys.json");
  const trustBefore = createHash("sha256").update(readFileSync(trust)).digest("hex");
  try {
    const off = buildInitPlan({ root, host: "generic", mcp: "off", watch: "off" });
    assert.equal(off.actions.some((action) => action.id === "enroll-watch"), false);
    const plan = buildInitPlan({ root, host: "generic", mcp: "on", watch: "on" });
    const applied = applyInitPlan({ root, plan, build: false, watchConfigPath: watch });
    assert.equal(applied.ok, true, applied.error);
    assert.deepEqual(readWatchConfig(watch).repos.map((repo) => repo.root), [realpathSync(root)]);
    const removed = uninstallInit({ root, watchConfigPath: watch });
    assert.equal(removed.ok, true, removed.error);
    assert.deepEqual(readWatchConfig(watch).repos, []);
    assert.equal(readFileSync(join(root, "CORTEX-AGENT.md"), "utf8"), "# Existing\n");
    assert.equal(createHash("sha256").update(readFileSync(trust)).digest("hex"), trustBefore);
  } finally { rmSync(root, { recursive: true, force: true }); rmSync(home, { recursive: true, force: true }); }
});

test("uninstall preserves enrollment that existed before onboarding", () => {
  const { root, home, watch } = fixture();
  try {
    writeWatchConfig({ version: 1, repos: [{ root, enabled: true }] }, watch);
    const plan = buildInitPlan({ root, host: "generic", mcp: "off", watch: "on" });
    assert.equal(applyInitPlan({ root, plan, build: false, watchConfigPath: watch }).ok, true);
    assert.equal(uninstallInit({ root, watchConfigPath: watch }).ok, true);
    assert.deepEqual(readWatchConfig(watch).repos.map((repo) => repo.root), [root]);
  } finally { rmSync(root, { recursive: true, force: true }); rmSync(home, { recursive: true, force: true }); }
});
