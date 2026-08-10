import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { syncToCurrentSource } from "../graph/barrier.mjs";
import { buildGraphGeneration } from "../graph/static-provider.mjs";
import { closeStore, openStore } from "../graph/store-sqlite.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/cortex.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function makeRepo() {
  const repo = mkdtempSync(join(tmpdir(), "cortex-barrier-"));
  cpSync(FIXTURE, repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  return repo;
}

function run(repo, args) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
}

function parseJsonStderr(result) {
  const stderr = String(result.stderr).trim();
  const start = stderr.lastIndexOf("\n{");
  return JSON.parse(start >= 0 ? stderr.slice(start + 1) : stderr);
}

test("read-only query barrier detects an edit without repairing it", () => {
  const repo = makeRepo();
  try {
    const path = join(repo, "src/service.ts");
    writeFileSync(path, `${readFileSync(path, "utf8")}\nexport const barrierProbe = true;\n`);
    const result = run(repo, ["graph", "search", "--query", "barrierProbe", "--json"]);
    assert.equal(result.status, 3);
    const payload = parseJsonStderr(result);
    assert.equal(payload.barrier.barrierResult, "timeout");
    const candidates = run(repo, ["graph", "candidates", "--query", "barrierProbe", "--json"]);
    assert.equal(candidates.status, 3);
    assert.equal(parseJsonStderr(candidates).barrier.barrierResult, "timeout");
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("corrupt clocks timeout by default and serve only with explicit allow-stale", () => {
  const repo = makeRepo();
  try {
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('source_clock','5') ON CONFLICT(key) DO UPDATE SET value='5'").run();
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('applied_clock','0') ON CONFLICT(key) DO UPDATE SET value='0'").run();
      db.prepare("DELETE FROM event_journal").run();
    } finally { closeStore(db); }
    const blocked = run(repo, ["graph", "search", "--query", "placeOrder", "--timeout-ms", "80", "--json"]);
    assert.equal(blocked.status, 3);
    assert.equal(parseJsonStderr(blocked).error, "stale_blocked");
    const allowed = run(repo, ["graph", "search", "--query", "placeOrder", "--timeout-ms", "80", "--allow-stale", "--json"]);
    assert.equal(allowed.status, 0, allowed.stderr);
    const payload = JSON.parse(allowed.stdout);
    assert.equal(payload.stale, true);
    assert.equal(payload.freshnessReceipt.barrierResult, "timeout");
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("event gap blocks read-only queries until explicit reconciliation", async () => {
  const repo = makeRepo();
  try {
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try { db.prepare("INSERT INTO watch_state(key,value) VALUES ('event_gap','1') ON CONFLICT(key) DO UPDATE SET value='1'").run(); }
    finally { closeStore(db); }
    const result = run(repo, ["graph", "search", "--query", "placeOrder", "--json"]);
    assert.equal(result.status, 3);
    assert.equal(parseJsonStderr(result).barrier.barrierResult, "timeout");
    const warmDb = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      const repaired = await syncToCurrentSource(warmDb, repo, { timeoutMs: 2000 });
      assert.equal(repaired.barrierResult, "caught_up");
      const started = performance.now();
      const receipt = await syncToCurrentSource(warmDb, repo, { timeoutMs: 2000 });
      const elapsed = performance.now() - started;
      assert.equal(receipt.barrierResult, "caught_up");
      assert.ok(elapsed < 200, `warm barrier took ${elapsed.toFixed(1)}ms`);
    } finally { closeStore(warmDb); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});
