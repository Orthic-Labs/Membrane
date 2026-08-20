import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { syncToCurrentSource } from "../src/graph/barrier.mjs";
import { issueScopeGrant, checkScopeGrant } from "../src/lib/receipt-store.mjs";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";
import { BlueprintRepositoryWorker } from "../watchman/repo-actor.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/blueprint.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function run(repo, args) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
}

function elapsed(fn) {
  const start = performance.now();
  const value = fn();
  return { value, ms: performance.now() - start };
}

async function elapsedAsync(fn) {
  const start = performance.now();
  const value = await fn();
  return { value, ms: performance.now() - start };
}

test("PR11 performance budgets stay within four-times CI slack", async () => {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-performance-"));
  try {
    cpSync(FIXTURE, repo, { recursive: true });
    assert.equal(run(repo, ["build", "--out", ".agent"]).status, 0);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    const barrierSamples = [];
    for (let sample = 0; sample < 5; sample += 1) {
      const barrier = await elapsedAsync(() => syncToCurrentSource(db, repo, { timeoutMs: 2000 }));
      assert.equal(barrier.value.barrierResult, "caught_up");
      barrierSamples.push(barrier.ms);
    }
    const barrierMs = Math.min(...barrierSamples);
    closeStore(db);

    const source = join(repo, "src/service.ts");
    writeFileSync(source, `${readFileSync(source, "utf8")}\nexport const performanceProbe = true;\n`);
    const delta = await elapsedAsync(() => new BlueprintRepositoryWorker({ root: repo }).ingest("src/service.ts"));
    assert.equal(delta.value.applied, true);
    const recall = elapsed(() => run(repo, ["recall", "--query", "placeOrder", "--json"]));
    assert.equal(recall.value.status, 0, recall.value.stderr);
    const issued = issueScopeGrant({ repoRoot: repo, generationId: "gen:test", taskId: "perf", paths: ["src/**"] });
    const grant = elapsed(() => checkScopeGrant({ repoRoot: repo, generationId: "gen:test", taskId: "perf", path: "src/service.ts" }));
    assert.equal(grant.value.allowed, true);
    const measured = { barrierMs, deltaMs: delta.ms, recallMs: recall.ms, grantMs: grant.ms, grantReceiptId: issued.grant.receiptId };
    assert.ok(measured.barrierMs <= 100, `healthy no-op barrier exceeded 4x budget: ${measured.barrierMs.toFixed(1)}ms`);
    assert.ok(measured.deltaMs <= 1000, `one-file delta exceeded 4x budget: ${measured.deltaMs.toFixed(1)}ms`);
    assert.ok(measured.recallMs <= 1400, `recall exceeded 4x budget: ${measured.recallMs.toFixed(1)}ms`);
    assert.ok(measured.grantMs <= 200, `grant check exceeded 4x budget: ${measured.grantMs.toFixed(1)}ms`);
    console.log(`performance-budget ${JSON.stringify(measured)}`);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});
