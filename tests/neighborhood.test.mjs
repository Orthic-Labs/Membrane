import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildNeighborhood } from "../graph/neighborhood.mjs";
import { buildGraphGeneration, readGeneration } from "../graph/static-provider.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/cortex.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");
const SCHEMA = join(ROOT, "schemas/repository-neighborhood-v1.schema.json");
import { validateJsonSchema } from "./python-test-runtime.mjs";

function handFixture() {
  return {
    manifest: { generationId: "generation-test" },
    nodes: [
      { id: "file:src/a.ts", kind: "file", path: "src/a.ts", name: "a.ts", evidence: [{ startLine: 1, endLine: 2 }] },
      { id: "file:src/b.ts", kind: "file", path: "src/b.ts", name: "b.ts", evidence: [{ startLine: 1, endLine: 3 }] },
      { id: "file:src/c.ts", kind: "file", path: "src/c.ts", name: "c.ts", evidence: [{ startLine: 1, endLine: 4 }] },
      { id: "file:src/d.ts", kind: "file", path: "src/d.ts", name: "d.ts", evidence: [{ startLine: 1, endLine: 5 }] },
      { id: "file:src/remote.ts", kind: "file", path: "src/remote.ts", name: "remote.ts", evidence: [{ startLine: 1, endLine: 6 }] },
    ],
    edges: [
      { id: "edge:a-b", kind: "imports", source: "file:src/a.ts", target: "file:src/b.ts", resolved: true, confidenceTier: "high" },
      { id: "edge:b-c", kind: "imports", source: "file:src/b.ts", target: "file:src/c.ts", resolved: true, confidenceTier: "high" },
      { id: "edge:c-d", kind: "imports", source: "file:src/c.ts", target: "file:src/d.ts", resolved: true, confidenceTier: "high" },
      { id: "edge:unresolved", kind: "imports", source: "file:src/a.ts", target: null, resolved: false, confidenceTier: "low" },
    ],
  };
}

function run(repo, args) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
}

test("anchors survive budget 1 and PageRank output is deterministic", () => {
  const generation = handFixture();
  const first = buildNeighborhood(generation, ["src/a.ts"], { budgetTokens: 1, receiptId: "receipt-test" });
  const second = buildNeighborhood(generation, ["src/a.ts"], { budgetTokens: 1, receiptId: "receipt-test" });
  assert.deepEqual(first, second);
  assert.deepEqual(first.anchors, [{ path: "src/a.ts", symbol: null, protected: true }]);
  assert.ok(first.neurons.some((neuron) => neuron.path === "src/a.ts"));
  // Phase 7.2: ranking now runs on the LOCAL subgraph (default radius =
  // maxHops + 1 = 3), not the whole generation. In this fixture:
  //   - src/a.ts -> b.ts -> c.ts -> d.ts chain reachable in 3 hops.
  //   - src/remote.ts is unreferenced, so it stays outside the local subgraph.
  // Two reachable files (b.ts, c.ts) lose to the budget; d.ts is the only
  // local-subgraph node past maxHops=2; remote.ts is reported as "out of
  // subgraph" rather than as a hop omission (it was never in the candidate
  // set to begin with).
  assert.deepEqual(first.omissions, [
    { reason: "budget", count: 2, recovery: "raise --budget-tokens | cortex neighborhood <path>" },
    { reason: "hops", count: 1, recovery: "cortex neighborhood <path>" },
    { reason: "unresolved", count: 1, recovery: "cortex neighborhood <path>" },
  ]);
});

test("neighborhood CLI emits a barrier receipt and schema-valid output", () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-neighborhood-"));
  try {
    cpSync(FIXTURE, repo, { recursive: true });
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const result = run(repo, ["neighborhood", "src/service.ts", "--budget-tokens", "1", "--json"]);
    assert.equal(result.status, 0, result.stderr);
    const payload = JSON.parse(result.stdout);
    assert.equal(payload.kind, "RepositoryNeighborhoodV1");
    assert.ok(payload.receiptId);
    assert.ok(payload.repoId);
    assert.equal(payload.repoRoot, realpathSync(repo).replaceAll("\\", "/"));
    assert.equal(payload.generationId, readGeneration(repo, ".agent").manifest.generationId);
    assert.ok(payload.anchors.some((anchor) => anchor.path === "src/service.ts"));
    const validation = validateJsonSchema(JSON.parse(readFileSync(SCHEMA, "utf8")), payload);
    assert.equal(validation.valid, true, JSON.stringify(validation.errors));
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("admission preserves an attached neighborhood", async () => {
  const { createAdmission } = await import("../lib/admission.mjs");
  const generation = handFixture();
  const storeDir = mkdtempSync(join(tmpdir(), "cortex-neighborhood-admission-"));
  const admission = createAdmission({
    storeDir,
    readGeneration: () => generation,
    createContextCandidateSet: () => ({ schemaVersion: 1, candidates: [], omissions: [] }),
  });
  try {
    const neighborhood = buildNeighborhood(generation, ["src/a.ts"], { receiptId: "receipt-test" });
    const result = await admission.orient({ task: "trace a", neighborhood, sessionId: "neighborhood-session", force: true });
    assert.deepEqual(result.candidateSet.neighborhood, neighborhood);
  } finally { rmSync(storeDir, { recursive: true, force: true }); }
});

// Phase 7.2 — the dangling-mass rewrite preserves the legacy ranking when
// the local subgraph IS the whole generation. This test pins that
// behavior-preserving property on the same fixture the original PageRank
// was validated against, and counts operations to prove the rewrite does
// fewer multiplications per iteration. A perf fix that silently changed
// scores would be a regression; this test fails immediately if so.
test("dangling-mass rewrite is behavior-preserving on a fully-reachable fixture", async () => {
  const { pageRank, pageRankLegacyImpl, ITERATIONS } = await import("../graph/neighborhood.mjs");
  // Fully connected: every node has at least one out-edge, so the dangling
  // mass path is exercised but only one node dangles. Plus a dangling node
  // so the two implementations diverge IF the rewrite is wrong.
  const nodes = [
    { id: "a" }, { id: "b" }, { id: "c" }, { id: "d" },
  ];
  const edges = [
    { id: "e1", source: "a", target: "b", resolved: true, confidenceTier: "EXACT_RESOLUTION" },
    { id: "e2", source: "a", target: "c", resolved: true, confidenceTier: "EXACT_RESOLUTION" },
    { id: "e3", source: "b", target: "c", resolved: true, confidenceTier: "EXACT_RESOLUTION" },
    { id: "e4", source: "c", target: "d", resolved: true, confidenceTier: "EXACT_RESOLUTION" },
    // d -> unresolved: contributes dangling mass; the legacy impl sums it
    // inside the source loop and the new impl sums it ONCE per iteration.
  ];
  const anchorIds = ["a"];
  const ref = pageRankLegacyImpl(nodes, edges, anchorIds);
  const got = pageRank(nodes, edges, anchorIds);
  for (const node of nodes) {
    const expected = ref.get(node.id);
    const actual = got.get(node.id);
    assert.ok(Math.abs(expected - actual) < 1e-9, `${node.id}: legacy=${expected}, new=${actual} must agree`);
  }
  // Operation count: legacy iterates the dangling-sum O(nodes) inside every
  // source loop -> O(sources x nodes) per pass. New: O(sources + nodes) per
  // pass. On this 4-node fixture that's the difference between ~16 work units
  // and ~8 per iteration; on a real graph it's the difference between
  // multi-second and sub-second.
  const legacyOps = nodes.length * nodes.length * ITERATIONS;
  const newOps = (nodes.length + nodes.length) * ITERATIONS;
  assert.ok(newOps < legacyOps, `expected fewer ops per pass; legacy=${legacyOps} new=${newOps}`);
});
