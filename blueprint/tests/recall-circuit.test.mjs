import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { executeRecallCircuit, recallCircuitToCandidateSet } from "../src/graph/recall-circuit.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { closeStore, openStoreReadOnly, readManifestEnvelope } from "../src/graph/store-sqlite.mjs";
import { traversalPolicyFamilies } from "../src/graph/traversal-policy.mjs";

test("RecallCircuit resolves exact anchors, returns evidence paths, and is deterministic", () => {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-recall-circuit-"));
  try {
    mkdirSync(join(repo, "src"));
    writeFileSync(join(repo, "src", "entry.js"), 'import { work } from "./worker.js";\nexport function main(){ return work(); }\n');
    writeFileSync(join(repo, "src", "worker.js"), "export function work(){ return 1; }\n");
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const db = openStoreReadOnly(join(repo, ".agent", "graph", "graph.db"));
    try {
      const generationId = readManifestEnvelope(db).generationId;
      const first = executeRecallCircuit(db, "trace dependencies", { generationId, anchors: ["src/entry.js"], policy: "dependency.forward" });
      const second = executeRecallCircuit(db, "trace dependencies", { generationId, anchors: ["src/entry.js"], policy: "dependency.forward" });
      assert.equal(first.id, second.id);
      assert.equal(first.state, "complete");
      assert.ok(first.paths.length > 0);
      assert.ok(first.paths.every((path) => path.nodes.length === path.edges.length + 1));
      assert.ok(first.paths.every((path) => path.evidence.length > 0));
      assert.ok(first.paths.every((path) => Number.isInteger(path.semanticAuthorityRank)));
      assert.ok(first.paths.every((path) => !("meanEdgeConfidence" in path)));
      const candidates = recallCircuitToCandidateSet(first, { repoRoot: repo });
      assert.equal(candidates.recallCircuit.id, first.id);
      assert.ok(candidates.candidates.every((candidate) => candidate.evidencePathId));
      assert.ok(candidates.candidates.every((candidate) => candidate.scoreComponents.semanticAuthority > 0));
    } finally {
      closeStore(db);
    }
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("RecallCircuit abstains without inventing semantic matches", () => {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-recall-abstain-"));
  try {
    writeFileSync(join(repo, "known.js"), "export const known = 1;\n");
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const db = openStoreReadOnly(join(repo, ".agent", "graph", "graph.db"));
    try {
      const generationId = readManifestEnvelope(db).generationId;
      const circuit = executeRecallCircuit(db, "vocabularynotpresent", { generationId });
      assert.equal(circuit.state, "abstained");
      assert.deepEqual(circuit.paths, []);
      assert.deepEqual(circuit.omissions, [{ reason: "no_relevant_seed" }]);
    } finally {
      closeStore(db);
    }
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("all required traversal policy families exist", () => {
  assert.deepEqual(traversalPolicyFamilies(), [
    "dependency.forward",
    "impact.reverse",
    "callgraph.forward",
    "test.coverage",
    "config.consumers",
    "architecture.boundary",
    "explore.both",
  ]);
});
