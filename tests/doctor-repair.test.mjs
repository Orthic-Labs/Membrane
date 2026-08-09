// D11: doctor repair plans — ordered, non-destructive, previewable, and
// confirmable.

import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

import { collectDoctorDiagnostics } from "../lib/operations/doctor.mjs";
import { buildRepairPlan } from "../lib/operations/repair.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/cortex.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

// Minimal map/stale fixture (same shape the build emits) so
// collectDoctorDiagnostics reaches the mcp_config_launchable lane.
function writeFixture(root, mcpServers) {
  mkdirSync(join(root, ".agent"), { recursive: true });
  writeFileSync(join(root, ".agent", "map.json"), JSON.stringify({ nodes: [], edges: [], stats: { docs: 0, claims: 0, codeRefs: 0 } }));
  writeFileSync(join(root, ".agent", "stale.json"), JSON.stringify({ missingReferences: [] }));
  if (mcpServers) writeFileSync(join(root, ".mcp.json"), JSON.stringify({ mcpServers }));
}

test("repair plan is ordered and non-destructive", () => {
  const plan = buildRepairPlan({
    root: "/repo",
    graphState: "stale",
    reasons: [
      { code: "stale_graph", severity: "blocker" },
      { code: "event_gap", severity: "blocker" },
    ],
  });
  assert.equal(plan.schemaVersion, 1);
  assert.ok(plan.actions.length >= 2);
  // Rebuild precedes reconciliation.
  assert.equal(plan.actions[0].id, "rebuild-graph");
  assert.equal(plan.actions[0].reversible, false);
  assert.ok(plan.actions.some((a) => a.id === "reconcile-watcher"));
});

test("repair plan no-ops when nothing is broken", () => {
  const plan = buildRepairPlan({ root: "/repo", graphState: "ready", reasons: [] });
  assert.equal(plan.actions.length, 1);
  assert.equal(plan.actions[0].id, "no-op");
});

test("cortex doctor --repair-plan --json emits a schema-valid plan", () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-doctor-repair-"));
  try {
    cpSync(FIXTURE, repo, { recursive: true });
    const result = spawnSync(process.execPath, [CLI, "doctor", "--repair-plan", "--json"], { cwd: repo, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    const plan = JSON.parse(result.stdout);
    assert.equal(plan.schemaVersion, 1);
    assert.ok(Array.isArray(plan.actions));
    for (const action of plan.actions) {
      assert.ok(action.id);
      assert.equal(typeof action.reversible, "boolean");
    }
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("cortex doctor --apply-repair without --yes requires confirmation", () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-doctor-confirm-"));
  try {
    cpSync(FIXTURE, repo, { recursive: true });
    const result = spawnSync(process.execPath, [CLI, "doctor", "--repair-plan", "--apply-repair", "--json"], { cwd: repo, encoding: "utf8" });
    assert.equal(result.status, 3);
    assert.match(result.stderr, /confirmation_required/);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("cortex doctor --repair-plan --apply-repair --yes applies the plan", () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-doctor-apply-"));
  try {
    cpSync(FIXTURE, repo, { recursive: true });
    const result = spawnSync(process.execPath, [CLI, "doctor", "--repair-plan", "--apply-repair", "--yes", "--json"], { cwd: repo, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const payload = JSON.parse(result.stdout);
    assert.ok(Array.isArray(payload.applied));
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("collectDoctorDiagnostics stays synchronous for existing callers", () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-doctor-sync-"));
  try {
    writeFixture(repo, { cortex: { command: process.execPath, args: [join(ROOT, "scripts/cortex-mcp.mjs"), "--root", repo] } });
    const diagnostics = collectDoctorDiagnostics(repo);
    assert.equal(typeof diagnostics.then, "undefined", "collectDoctorDiagnostics must not return a Promise");
    assert.equal(diagnostics.schemaVersion, 1);
    assert.ok(Array.isArray(diagnostics.reasons));
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("mcp_config_launchable passes for a launchable cortex MCP config", () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-doctor-mcp-good-"));
  try {
    const args = [join(ROOT, "scripts/cortex-mcp.mjs"), "--root", repo];
    writeFixture(repo, { cortex: { command: process.execPath, args } });
    const diagnostics = collectDoctorDiagnostics(repo);
    const reason = diagnostics.reasons.find((r) => r.code === "mcp_config_launchable");
    assert.ok(reason, "mcp_config_launchable reason missing for a launchable config");
    assert.equal(reason.severity, "info");
    assert.equal(reason.status, "pass");
    assert.equal(reason.command, process.execPath);
    assert.deepEqual(reason.args, args);
    assert.match(reason.message, /stayed alive/);
    assert.equal(diagnostics.errors.length, 0);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("mcp_config_launchable fails with exit-code evidence for a sabotaged config", () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-doctor-mcp-bad-"));
  try {
    const args = [join(ROOT, "scripts/cortex-mcp.mjs")];
    writeFixture(repo, { cortex: { command: process.execPath, args } });
    const diagnostics = collectDoctorDiagnostics(repo);
    const reason = diagnostics.reasons.find((r) => r.code === "mcp_config_launchable");
    assert.ok(reason, "mcp_config_launchable reason missing for a sabotaged config");
    assert.equal(reason.severity, "blocker");
    assert.equal(reason.exitCode, 1);
    assert.ok(reason.message.includes(reason.command), "evidence must contain the exact command");
    assert.ok(reason.message.includes(JSON.stringify(reason.args)), "evidence must contain the JSON args");
    assert.match(reason.message, /exited early/);
    assert.ok(diagnostics.errors.some((e) => e.includes("not launchable")), "fail must surface in doctor errors");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("mcp_config_launchable is absent when .mcp.json is absent", () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-doctor-mcp-no-file-"));
  try {
    writeFixture(repo, null);
    const diagnostics = collectDoctorDiagnostics(repo);
    assert.ok(!diagnostics.reasons.some((r) => r.code === "mcp_config_launchable"));
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("mcp_config_launchable is absent when .mcp.json has no cortex entry", () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-doctor-mcp-no-entry-"));
  try {
    writeFixture(repo, { other: { command: process.execPath, args: [] } });
    const diagnostics = collectDoctorDiagnostics(repo);
    assert.ok(!diagnostics.reasons.some((r) => r.code === "mcp_config_launchable"));
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("mcp_config_launchable degrades to a typed warning when spawn fails", () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-doctor-mcp-spawn-"));
  try {
    writeFixture(repo, { cortex: { command: "definitely-not-a-real-executable-xyz", args: [] } });
    let diagnostics;
    assert.doesNotThrow(() => { diagnostics = collectDoctorDiagnostics(repo); });
    const reason = diagnostics.reasons.find((r) => r.code === "mcp_config_launchable");
    assert.ok(reason, "mcp_config_launchable reason missing for a spawn failure");
    assert.equal(reason.severity, "warning");
    assert.ok(diagnostics.warnings.some((w) => w.includes("spawn-checked")), "spawn failure must surface as a typed warning");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
