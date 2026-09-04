import assert from "node:assert/strict";
import test from "node:test";

import { assertPublicationCandidate, evaluatePublicationCandidate } from "../src/graph/publication-policy.mjs";

function generation({ id = "gen-1", nodes = [], edges = [], complete = true, counts = true, ...rest } = {}) {
  return {
    schemaVersion: 1,
    manifest: {
      generationId: id,
      complete,
      ...(counts ? { counts: { nodes: nodes.length, edges: edges.length } } : {}),
    },
    nodes,
    edges,
    ...rest,
  };
}

const file = (path) => ({ id: `file:${path}`, kind: "file", path, evidence: [{ path }] });
const symbol = (path, name) => ({ id: `symbol:${path}::${name}`, kind: "symbol", path, evidence: [{ path }] });
const edge = (id, source, target, path) => ({ id, kind: "CALLS", source, target, evidence: [{ path }] });

test("complete, internally consistent generation is publishable", () => {
  const candidate = generation({ nodes: [file("a.ts")], edges: [] });
  const decision = evaluatePublicationCandidate(candidate);
  assert.equal(decision.action, "allow");
  assert.equal(decision.reasonCode, "complete_generation");
  assert.deepEqual(decision.problems, []);
});

test("partial, truncated, and count-mismatched generations fail closed", () => {
  const incomplete = generation({ complete: false, nodes: [file("a.ts")] });
  assert.equal(evaluatePublicationCandidate(incomplete).reasonCode, "manifest_not_complete");

  const truncated = generation({ nodes: [file("a.ts")], truncated: true });
  assert.ok(evaluatePublicationCandidate(truncated).problems.includes("generation_truncated"));

  const countMismatch = generation({ nodes: [file("a.ts")] });
  countMismatch.manifest.counts.nodes = 99;
  assert.ok(evaluatePublicationCandidate(countMismatch).problems.includes("manifest_node_count_mismatch"));
});

test("incremental shrink guard allows facts owned by changed paths to disappear", () => {
  const prior = generation({
    id: "prior",
    nodes: [file("a.ts"), file("b.ts"), symbol("a.ts", "old")],
    edges: [edge("edge:old", "symbol:a.ts::old", "file:b.ts", "a.ts")],
  });
  const candidate = generation({ id: "next", nodes: [file("a.ts"), file("b.ts")], edges: [] });
  const decision = evaluatePublicationCandidate(candidate, { priorGeneration: prior, changedPaths: ["a.ts"] });
  assert.equal(decision.action, "allow");
  assert.deepEqual(decision.unexpectedShrink, []);
});

test("incremental shrink guard blocks unrelated fact disappearance", () => {
  const prior = generation({
    id: "prior",
    nodes: [file("a.ts"), file("b.ts"), symbol("b.ts", "keep")],
    edges: [edge("edge:keep", "symbol:b.ts::keep", "file:a.ts", "b.ts")],
  });
  const candidate = generation({ id: "next", nodes: [file("a.ts"), file("b.ts")], edges: [] });
  const decision = evaluatePublicationCandidate(candidate, { priorGeneration: prior, changedPaths: ["a.ts"] });
  assert.equal(decision.action, "block");
  assert.ok(decision.problems.includes("unexpected_unrelated_fact_shrink"));
  assert.deepEqual(new Set(decision.unexpectedShrink.map((item) => item.factId)), new Set(["symbol:b.ts::keep", "edge:keep"]));
  assert.throws(() => assertPublicationCandidate(candidate, { priorGeneration: prior, changedPaths: ["a.ts"] }), (error) => error.code === "publication_incomplete");
});

test("doc-truth truncation cannot be promoted as a complete generation", () => {
  const candidate = generation({ nodes: [file("README.md")], docTruth: { truncated: true } });
  const decision = evaluatePublicationCandidate(candidate);
  assert.equal(decision.action, "block");
  assert.ok(decision.problems.includes("doc_truth_truncated"));
});
