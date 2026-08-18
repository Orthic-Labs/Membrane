import assert from "node:assert/strict";
import { cpSync, mkdtempSync, realpathSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildGraphGeneration, createContextCandidateSet, readGeneration } from "../src/graph/static-provider.mjs";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";
import { writeWatchConfig } from "../watchman/supervisor.mjs";

const ROOT = join(import.meta.dirname, "..");
const WATCH = join(ROOT, "scripts/cortex-watch.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function makeRepo(label) {
  const repo = mkdtempSync(join(tmpdir(), `cortex-federation-${label}-`));
  cpSync(FIXTURE, repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  return repo;
}

test("barrier-all returns independent receipts and candidate identities", () => {
  const repoA = makeRepo("a");
  const repoB = makeRepo("b");
  const home = mkdtempSync(join(tmpdir(), "cortex-federation-home-"));
  try {
    const db = openStore(join(repoA, ".agent/graph/graph.db"));
    try { db.prepare("INSERT INTO watch_state(key,value) VALUES ('event_gap','1') ON CONFLICT(key) DO UPDATE SET value='1'").run(); }
    finally { closeStore(db); }
    writeWatchConfig({ repos: [{ root: repoA, enabled: true }, { root: repoB, enabled: true }] }, join(home, ".cortex", "watch.json"));
    const result = spawnSync(process.execPath, [WATCH, "barrier-all", "--json"], {
      env: { ...process.env, HOME: home, USERPROFILE: home },
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    const payload = JSON.parse(result.stdout);
    const canonicalA = realpathSync(repoA);
    const canonicalB = realpathSync(repoB);
    assert.equal(payload.receipts.length, 2);
    assert.deepEqual(payload.receipts.map((entry) => entry.repoRoot).sort(), [canonicalA, canonicalB].sort());
    assert.ok(payload.receipts.every((entry) => entry.receipt.receiptId));
    assert.ok(payload.receipts.every((entry) => entry.receipt.barrierResult === "caught_up"));
    const candidateA = createContextCandidateSet(readGeneration(repoA, ".agent"), { task: "a" });
    const candidateB = createContextCandidateSet(readGeneration(repoB, ".agent"), { task: "b" });
    assert.notEqual(candidateA.repoId, candidateB.repoId);
    assert.equal(candidateA.repoRoot, canonicalA);
    assert.equal(candidateB.repoRoot, canonicalB);
    assert.equal(candidateA.receiptId, null);
    const cliCandidate = spawnSync(process.execPath, [join(ROOT, "scripts/cortex.mjs"), "candidates", "--repo-id", "explicit-repo", "--query", "placeOrder", "--json"], { cwd: repoA, encoding: "utf8" });
    assert.equal(cliCandidate.status, 0, cliCandidate.stderr);
    assert.equal(JSON.parse(cliCandidate.stdout).repoId, "explicit-repo");
  } finally {
    rmSync(repoA, { recursive: true, force: true });
    rmSync(repoB, { recursive: true, force: true });
    rmSync(home, { recursive: true, force: true });
  }
});
