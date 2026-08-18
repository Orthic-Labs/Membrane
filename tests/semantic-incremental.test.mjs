import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { claimEvidenceFingerprint, buildIncrementalPhase2Plan } from "../src/lib/incremental-phase2.mjs";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/cortex.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function makeRepo() {
  const repo = mkdtempSync(join(tmpdir(), "cortex-semantic-"));
  cpSync(FIXTURE, repo, { recursive: true });
  return repo;
}

function run(repo, args) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
}

test("one doc delta rewrites only that doc's claims and records the doc domain", () => {
  const repo = makeRepo();
  try {
    writeFileSync(join(repo, "docs/SECOND.md"), "# Secondary\n\nThe service is implemented in `src/service.ts`.\n");
    assert.equal(run(repo, ["build", "--out", ".agent"]).status, 0);
    const before = JSON.parse(readFileSync(join(repo, ".agent/claims.json"), "utf8"));
    const otherClaims = before.filter((claim) => claim.source !== "docs/SECOND.md").map((claim) => JSON.stringify(claim));
    writeFileSync(join(repo, "docs/SECOND.md"), "# Secondary\n\nThe service is implemented in `src/service.ts`.\nThe store is implemented in `src/store.ts`.\n");
    const delta = run(repo, ["delta", "docs/SECOND.md", "--out", ".agent"]);
    assert.equal(delta.status, 0, delta.stderr);
    const after = JSON.parse(readFileSync(join(repo, ".agent/claims.json"), "utf8"));
    assert.deepEqual(after.filter((claim) => claim.source !== "docs/SECOND.md").map((claim) => JSON.stringify(claim)), otherClaims);
    assert.equal(after.filter((claim) => claim.source === "docs/SECOND.md").length, 2);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      const owner = db.prepare("SELECT provider_id, freshness_domain FROM fact_owner WHERE source_path='docs/SECOND.md' AND provider_id='doctruth'").get();
      assert.equal(owner.provider_id, "doctruth");
      assert.equal(owner.freshness_domain, "doc");
      assert.ok(db.prepare("SELECT 1 FROM artifact_state WHERE artifact='claims.json'").get());
      assert.ok(db.prepare("SELECT 1 FROM artifact_state WHERE artifact='stale.json'").get());
      assert.ok(db.prepare("SELECT 1 FROM artifact_state WHERE artifact='queue.json'").get());
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("a code edit re-queues only verdicts whose evidence fingerprint changed", () => {
  const graph = (a, b) => ({
    manifest: { generationId: "gen:test" },
    nodes: [
      { kind: "file", path: "src/a.ts", evidence: [{ path: "src/a.ts", contentHash: a }] },
      { kind: "file", path: "src/b.ts", evidence: [{ path: "src/b.ts", contentHash: b }] },
      { kind: "file", path: "docs/a.md", evidence: [{ path: "docs/a.md", contentHash: "doc-a" }] },
      { kind: "file", path: "docs/b.md", evidence: [{ path: "docs/b.md", contentHash: "doc-b" }] },
    ],
  });
  const queue = {
    claims: [
      { id: "claim-a", source: "docs/a.md", line: 1, status: "implemented", text: "A is implemented.", candidateFiles: ["src/a.ts"] },
      { id: "claim-b", source: "docs/b.md", line: 1, status: "implemented", text: "B is implemented.", candidateFiles: ["src/b.ts"] },
    ],
  };
  const initial = graph("a1", "b1");
  const verdictEnvelope = {
    verdicts: queue.claims.map((claim) => ({ claimId: claim.id, verdict: "verified", inputFingerprint: claimEvidenceFingerprint(claim, initial) })),
  };
  const warm = buildIncrementalPhase2Plan({ graph: initial, queue, verdictEnvelope, understanding: null });
  assert.deepEqual(warm.verdicts.verify, []);
  const changed = buildIncrementalPhase2Plan({ graph: graph("a2", "b1"), queue, verdictEnvelope, understanding: null });
  assert.deepEqual(changed.verdicts.verify.map((claim) => claim.claimId), ["claim-a"]);
});

test("structural query proceeds while doc domain is pending", () => {
  const repo = makeRepo();
  try {
    assert.equal(run(repo, ["build", "--out", ".agent"]).status, 0);
    writeFileSync(join(repo, "docs/ARCHITECTURE.md"), `${readFileSync(join(repo, "docs/ARCHITECTURE.md"), "utf8")}\nThe order API is implemented in src/service.ts.\n`);
    assert.equal(run(repo, ["delta", "docs/ARCHITECTURE.md", "--out", ".agent"]).status, 0);
    const result = run(repo, ["graph", "search", "--query", "placeOrder", "--json"]);
    assert.equal(result.status, 0, result.stderr);
    const payload = JSON.parse(result.stdout);
    assert.equal(payload.freshnessReceipt.barrierResult, "caught_up");
    assert.deepEqual(payload.freshnessReceipt.domainsPending, ["doc"]);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});
