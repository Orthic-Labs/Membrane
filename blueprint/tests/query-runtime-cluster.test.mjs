import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildDisposableArchitectureProjection } from "../src/graph/architecture-model.mjs";
import { decomposeChangeRisk } from "../src/graph/analytics/index.mjs";
import { executeRecallCircuit, recallCircuitToCandidateSet } from "../src/graph/recall-circuit.mjs";
import { resolveSeeds } from "../src/graph/seed-resolver.mjs";
import { buildGraphGeneration, scanSourcesPublic } from "../src/graph/static-provider.mjs";
import { changesSinceReference, createSnapshot } from "../src/graph/snapshots.mjs";
import { closeStore, openStore, openStoreReadOnly, readManifestEnvelope } from "../src/graph/store-sqlite.mjs";
import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";
import { routeFederatedQuery } from "../src/lib/federation/index.mjs";
import { evaluateConvergenceOracle, reconcile } from "../watchman/reconcile.mjs";

const CLI = join(import.meta.dirname, "..", "scripts", "blueprint.mjs");

function repo(files, { git = false } = {}) {
  const root = mkdtempSync(join(tmpdir(), "blueprint-query-cluster-"));
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(join(root, path, ".."), { recursive: true });
    writeFileSync(join(root, path), content);
  }
  if (git) {
    const run = (args) => execFileSync("git", args, { cwd: root, stdio: "ignore" });
    run(["init", "--quiet"]);
    run(["config", "user.email", "test@example.invalid"]);
    run(["config", "user.name", "Test"]);
    run(["add", "."]);
    run(["commit", "--quiet", "-m", "fixture"]);
  }
  if (git) execFileSync(process.execPath, [CLI, "build", "--out", ".agent"], { cwd: root, stdio: "ignore" });
  else buildGraphGeneration(root, { outDir: ".agent", persist: true });
  return root;
}

test("response boundary suppresses only changed source evidence", async () => {
  const root = repo({ "src/value.js": "export const oldValue = 1;\n", "src/stable.js": "export const stableValue = 1;\n" }, { git: true });
  try {
    const service = createBlueprintApplicationService({ allowEmbeddedRoot: true, freshnessOwnership: "resident" });
    assert.ok((await service.search({ repoRoot: root, query: "oldValue" })).results.length > 0);
    writeFileSync(join(root, "src/value.js"), "export const replacementValue = 2;\n");
    const stale = await service.search({ repoRoot: root, query: "oldValue", allowStale: true });
    assert.equal(stale.freshnessReceipt.freshness, "changed_since_generation");
    assert.deepEqual(stale.freshnessReceipt.staleSources.paths, ["src/value.js"]);
    assert.equal(stale.results.length, 0);
    assert.ok(stale.omissions.some((item) => item.reason === "stale_source_suppressed"));
    assert.ok((await service.search({ repoRoot: root, query: "stableValue", allowStale: true })).results.length > 0);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("convergence oracle compares live source leaves with sealed incremental state", () => {
  const root = repo({ "src/a.js": "export const a = 1;\n" });
  try {
    const db = openStore(join(root, ".agent/graph/graph.db"));
    try {
      assert.equal(evaluateConvergenceOracle(db, scanSourcesPublic(root).files, { eventGapOverride: false }).converged, true);
      writeFileSync(join(root, "src/a.js"), "export const a = 2;\n");
      const mismatch = evaluateConvergenceOracle(db, scanSourcesPublic(root).files, { eventGapOverride: false });
      assert.equal(mismatch.converged, false);
      assert.deepEqual(mismatch.mismatches.changed, ["src/a.js"]);
      writeFileSync(join(root, "src/a.js"), "export const a = 1;\n");
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('domains_pending','compiler_python') ON CONFLICT(key) DO UPDATE SET value=excluded.value").run();
      const domainPending = evaluateConvergenceOracle(db, scanSourcesPublic(root).files, { eventGapOverride: false });
      assert.equal(domainPending.converged, false);
      assert.deepEqual(domainPending.domainsPending, ["compiler_python"]);
    } finally { closeStore(db); }
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("snapshot, generation & treeish projections contain semantic node and edge deltas", async () => {
  const root = repo({
    ".gitignore": ".agent/\n",
    "src/a.js": "export const a = 1;\n",
    "src/b.js": "export const b = 1;\n",
  }, { git: true });
  try {
    execFileSync("git", ["add", "docs/architecture.md", "docs/product.md"], { cwd: root, stdio: "ignore" });
    execFileSync("git", ["commit", "--quiet", "-m", "generated projections"], { cwd: root, stdio: "ignore" });
    execFileSync(process.execPath, [CLI, "build", "--out", ".agent"], { cwd: root, stdio: "ignore" });
    let db = openStore(join(root, ".agent/graph/graph.db"));
    let baseGeneration;
    try {
      baseGeneration = readManifestEnvelope(db).generationId;
      createSnapshot(db, "semantic-base", root);
    } finally { closeStore(db); }
    const baseTreeish = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim();
    writeFileSync(join(root, "src/b.js"), "import { a } from './a.js';\nexport const b = a;\n");
    execFileSync("git", ["add", "src/b.js"], { cwd: root, stdio: "ignore" });
    execFileSync("git", ["commit", "--quiet", "-m", "semantic change"], { cwd: root, stdio: "ignore" });
    db = openStore(join(root, ".agent/graph/graph.db"));
    try {
      await reconcile(db, root, { outDir: ".agent" });
      for (const projection of [
        changesSinceReference(db, root, { snapshot: "semantic-base" }),
        changesSinceReference(db, root, { generation: baseGeneration }),
        changesSinceReference(db, root, { treeish: { base: baseTreeish, head: "HEAD" } }),
      ]) {
        assert.equal(projection.authority, "history_reference_only");
        assert.ok(projection.semanticDelta.nodes.changed.length > 0);
        assert.ok(projection.semanticDelta.edges.added.length > 0);
        assert.ok(projection.semanticDelta.nodes.changed.every((change) => change.before.evidence.length > 0 && change.after.evidence.length > 0));
        assert.ok(projection.semanticDelta.edges.added.every((change) => change.after.evidence.length > 0));
      }
    } finally { closeStore(db); }
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("ambiguous seed resolution abstains with explicit candidates while evidence paths stay atomic", () => {
  const root = repo({ "src/a.js": "export function work(){ return 1; }\n", "src/b.js": "export function work(){ return 2; }\n" });
  try {
    const db = openStoreReadOnly(join(root, ".agent/graph/graph.db"));
    try {
      const generationId = readManifestEnvelope(db).generationId;
      const ambiguous = resolveSeeds(db, "work", { generationId });
      assert.equal(ambiguous.state, "ambiguous");
      assert.equal(ambiguous.seeds.length, 0);
      assert.equal(ambiguous.candidateCount, 2);
      assert.ok(ambiguous.candidates.every((candidate) => Array.isArray(candidate.evidence)));
      const circuit = executeRecallCircuit(db, "trace", { generationId, anchors: ["src/a.js"], policy: "dependency.forward" });
      const richCandidates = recallCircuitToCandidateSet(circuit);
      const candidates = recallCircuitToCandidateSet(circuit, { canonical: true });
      assert.ok(circuit.paths.every((path) => path.evidenceEnvelope.kind === "AtomicEvidencePath"));
      assert.ok(richCandidates.candidates.every((candidate) => candidate.evidencePathId && candidate.evidenceEnvelope));
      assert.ok(candidates.candidates.every((candidate) => !("evidencePathId" in candidate) && !("evidenceEnvelope" in candidate)));
    } finally { closeStore(db); }
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("diff impact resolves file seeds & returns inspectable risk with low-authority co-change", async () => {
  const root = repo({ "src/a.js": "export const a = 1;\n", "src/b.js": "import { a } from './a.js'; export const b = a;\n" });
  try {
    const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
    const result = await service.impact({
      repoRoot: root,
      diff: "--- a/src/a.js\n+++ b/src/a.js\n@@ -1 +1 @@\n",
      cochangeScore: 1,
    });
    assert.equal(result.seedEnvelope.families.diff, true);
    assert.deepEqual(result.seedEnvelope.changedPaths, ["src/a.js"]);
    assert.equal(result.risk.kind, "ChangeRiskDecomposition");
    assert.equal(result.risk.factors.find((factor) => factor.id === "cochange").authority, "historical_low");
    assert.ok(result.slices.every((slice) => slice.generationId === result.generationId));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("architecture projection is disposable, cited, generation-bound & planner-neutral", () => {
  const evidence = [{ path: "src/a.js", startLine: 1, endLine: 2, contentHash: "abc" }];
  const projection = buildDisposableArchitectureProjection({
    generationId: "g1",
    nodes: [{ id: "a", path: "src/a.js", evidence }, { id: "b", path: "lib/b.js", evidence: [{ ...evidence[0], path: "lib/b.js" }] }],
    edges: [{ id: "e", source: "a", target: "b", kind: "imports", evidence }],
  });
  assert.equal(projection.authority, "disposable_cited_view");
  assert.equal(projection.plannerAuthority, "none");
  assert.ok(projection.components.every((component) => component.citations.length > 0));
  assert.ok(projection.flows.every((flow) => flow.citations.length > 0));
});

test("federation routes explicit repositories independently without raw node-space merge", async () => {
  const response = await routeFederatedQuery({
    repositories: [{ repoId: "a" }, { repoId: "b" }],
    allowedRepoIds: ["a", "b"],
    operation: "search",
    querySlice: async (repository) => repository.repoId === "a"
      ? { generationId: "ga", results: [{ id: "same" }], freshnessReceipt: { receiptId: "ra" } }
      : Promise.reject(Object.assign(new Error("offline"), { code: "offline" })),
  });
  assert.equal(response.selection, "unranked_repository_slices");
  assert.equal(response.slices.length, 2);
  assert.deepEqual(response.results[0], { repoId: "a", generationId: "ga", results: [response.slices[0].results[0]] });
  assert.equal(response.slices[1].omissions[0].reason, "repository_query_failed");
});

test("co-change cannot dominate structural risk", () => {
  const low = decomposeChangeRisk({ cochangeScore: 1 });
  assert.ok(low.score <= 0.15);
  assert.equal(low.band, "low");
});
