// D08: init plan is pure and side-effect free; flags are exact; detection is
// deterministic.

import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, readdirSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildInitPlan } from "../src/lib/init/plan.mjs";
import { detectHosts } from "../src/lib/init/detect-hosts.mjs";

test("buildInitPlan is side-effect free", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-init-plan-"));
  try {
    const before = new Set(readdirSync(root));
    const plan = buildInitPlan({ root, host: "generic", scope: "project", mcp: "off", watch: "off", hooks: "none" });
    const after = new Set(readdirSync(root));
    assert.deepEqual([...after], [...before]);
    assert.equal(plan.schemaVersion, 1);
    assert.deepEqual(plan.hosts, ["generic"]);
    assert.ok(plan.actions.some((a) => a.id === "build-generation"));
    assert.ok(plan.uninstallCommand.includes("blueprint uninstall"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("explicit host wins over detection", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-init-host-"));
  try {
    writeFileSync(join(root, "CLAUDE.md"), "# claude\n");
    const plan = buildInitPlan({ root, host: "cursor", scope: "project" });
    assert.deepEqual(plan.hosts, ["cursor"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("auto host detects from existing config files", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-init-auto-"));
  try {
    mkdirSync(join(root, ".cursor", "rules"), { recursive: true });
    writeFileSync(join(root, ".cursor", "rules", "blueprint.mdc"), "# cursor\n");
    const hosts = detectHosts({ explicit: "auto", root });
    assert.ok(hosts.includes("cursor"), `expected cursor, got ${hosts}`);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("auto host falls back to generic with no configs", (t) => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-init-generic-"));
  try {
    const hosts = detectHosts({ explicit: "auto", root });
    // If a host CLI is installed on this machine, detection legitimately finds it.
    const knownProbe = ["claude", "codex", "cursor"].some((cmd) =>
      spawnSync("sh", ["-c", `command -v ${cmd} 2>/dev/null`], { encoding: "utf8" }).status === 0);
    if (knownProbe) {
      assert.ok(hosts.length > 0, "a host was detected");
      return;
    }
    assert.deepEqual(hosts, ["generic"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("mcp auto enables only for claude-code; explicit on/off respected", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-init-mcp-"));
  try {
    const generic = buildInitPlan({ root, host: "generic", scope: "project", mcp: "auto" });
    assert.equal(generic.mcpEnabled, false);
    const forced = buildInitPlan({ root, host: "generic", scope: "project", mcp: "on" });
    assert.equal(forced.mcpEnabled, true);
    const claude = buildInitPlan({ root, host: "claude-code", scope: "project", mcp: "auto" });
    assert.equal(claude.mcpEnabled, true);
    assert.ok(claude.actions.some((a) => a.id === "install-mcp"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("hooks level is carried in the plan", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-init-hooks-"));
  try {
    const plan = buildInitPlan({ root, host: "generic", scope: "project", hooks: "git" });
    const hooksAction = plan.actions.find((a) => a.kind === "hooks");
    assert.equal(hooksAction.level, "git");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("plan shape is fixed per S-07", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-init-shape-"));
  try {
    const plan = buildInitPlan({ root, host: "claude-code", scope: "project", mcp: "on", watch: "off", hooks: "all" });
    assert.equal(plan.schemaVersion, 1);
    assert.ok(Array.isArray(plan.hosts));
    assert.ok(Array.isArray(plan.actions));
    assert.equal(typeof plan.uninstallCommand, "string");
    for (const action of plan.actions) {
      assert.ok(action.id);
      assert.ok(action.kind);
      assert.equal(typeof action.reversible, "boolean");
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
