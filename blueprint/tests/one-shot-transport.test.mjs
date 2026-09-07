import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const entry = fileURLToPath(new URL("../scripts/blueprint-one-shot.mjs", import.meta.url));
test("unregistered one-shot graph queries initialize, resolve & refresh without watcher", { timeout: 120000 }, () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-one-shot-test-"));
  const home = mkdtempSync(join(tmpdir(), "blueprint-one-shot-home-"));
  const env = { ...process.env, HOME: home, USERPROFILE: home, BLUEPRINT_HOME: home };
  function request(method, input = {}, generation = null) {
    const wire = { protocolVersion: 1, requestId: "one-shot-test", repoId: "fixture", generation,
      method, deadlineMs: 30000, input: { repoRoot: root, ...input } };
    const run = spawnSync(process.execPath, [entry], { input: JSON.stringify(wire), encoding: "utf8",
      timeout: 35000, maxBuffer: 1024 * 1024, windowsHide: true, env });
    assert.equal(run.status, 0, run.stderr);
    const response = JSON.parse(run.stdout);
    if (!response.ok) response.diagnostics = run.stderr;
    assert.equal(response.requestId, wire.requestId);
    return response;
  }
  try {
    const initialized = spawnSync("git", ["init", "--quiet", root], { encoding: "utf8", windowsHide: true });
    assert.equal(initialized.status, 0, initialized.stderr);
    mkdirSync(join(root, "src"));
    const nudge = spawnSync(process.execPath, [fileURLToPath(new URL("../scripts/blueprint-watch.mjs", import.meta.url)), "nudge", root],
      { encoding: "utf8", windowsHide: true, timeout: 5000, env: { ...env, MEMBRANE_HUB_CHILD: "0" } });
    assert.equal(nudge.status, 2, nudge.stderr);
    assert.equal(JSON.parse(nudge.stdout).reason, "hub_inactive");
    assert.equal(existsSync(join(root, ".agent", "graph", "graph.db")), false);
    writeFileSync(join(root, "src", "retry.ts"), "export function reconnectAfterWake() { return 3; }\n");
    assert.equal(spawnSync("git", ["-C", root, "add", "src"], { windowsHide: true }).status, 0);
    assert.equal(spawnSync("git", ["-C", root, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "fixture"], { windowsHide: true }).status, 0);
    const found = request("search", { query: "reconnectAfterWake" });
    assert.equal(found.ok, true, JSON.stringify(found));
    const node = found.result.results.find(node => node.name === "reconnectAfterWake");
    assert.ok(node, JSON.stringify(found));
    assert.equal(request("resolve", { nodeId: node.id }, found.generation).ok, true);
    for (const method of ["expand", "impact"]) {
      const response = request(method, { anchor: node.id, depth: 1, budget: 1000 }, found.generation);
      assert.equal(response.ok, true, JSON.stringify(response));
    }
    for (const method of ["architecture", "documentTruth", "snapshot_list", "findings.get"]) {
      const response = request(method, {}, found.generation);
      assert.equal(response.ok, true, JSON.stringify(response));
    }
    const mismatch = request("resolve", { nodeId: node.id }, "wrong-generation");
    assert.equal(mismatch.ok, false);
    assert.equal(mismatch.error.code, "generation_mismatch");
    writeFileSync(join(root, "src", "retry.ts"), "export function reconnectAfterSleep() { return 2; }\n");
    const refreshed = request("refresh");
    assert.equal(refreshed.ok, true, JSON.stringify(refreshed));
    assert.notEqual(refreshed.generation, found.generation);
    const edited = request("search", { query: "reconnectAfterSleep" });
    assert.equal(edited.ok, true, JSON.stringify(edited));
    assert.notEqual(edited.generation, found.generation);
    assert.ok(edited.result.results.some(node => node.name === "reconnectAfterSleep"));
    const built = request("build");
    assert.equal(built.ok, true, JSON.stringify(built));
    const changes = request("changes", { sinceGeneration: built.generation });
    assert.equal(changes.ok, false, JSON.stringify(changes));
    assert.equal(changes.error.code, "snapshot_dirty");
    const status = request("status");
    assert.equal(status.ok, true, JSON.stringify(status));
    assert.equal(status.result.runtime.watcherRunning, false);
    assert.equal(status.result.runtime.enrolledRepoCount, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(home, { recursive: true, force: true });
  }
});
