// D40: graph analytics and architecture/layer views — deterministic,
// generation-bound, candidates never overstated as facts.

import assert from "node:assert/strict";
import test from "node:test";

import { findSccs, findCycles, deadCodeCandidates, assignLayers, analyticsDigest } from "../graph/analytics/index.mjs";
import { buildArchitectureModel } from "../graph/architecture-model.mjs";

test("Tarjan SCC finds cycles deterministically", () => {
  const edges = [
    { source: "a", target: "b" },
    { source: "b", target: "a" },
    { source: "c", target: "d" },
  ];
  const sccs = findSccs(edges);
  const cycles = findCycles(edges);
  assert.ok(cycles.some((component) => component.includes("a") && component.includes("b")));
  // c→d is acyclic: c and d are singleton SCCs, not a cycle.
  assert.ok(!cycles.some((component) => component.includes("c") && component.includes("d")));
  assert.ok(sccs.some((component) => component.length === 1 && component[0] === "c"));
});

test("cycle detection includes self-loops", () => {
  const cycles = findCycles([{ source: "x", target: "x" }]);
  assert.ok(cycles.some((component) => component.length === 1 && component[0] === "x"));
});

test("dead-code candidates are candidates, not proven facts", () => {
  const nodes = [{ id: "a" }, { id: "b" }, { id: "c" }];
  const edges = [{ source: "a", target: "b" }];
  const candidates = deadCodeCandidates({ nodes, edges });
  // a (root) and c (unreferenced) both have no incoming edges; only b is referenced.
  assert.deepEqual(candidates.map((c) => c.id).sort(), ["a", "c"]);
  assert.equal(candidates[0].candidate, true);
});

test("layers are deterministic with stable tie-breaking", () => {
  const nodes = [{ id: "a" }, { id: "b" }, { id: "c" }];
  const edges = [{ source: "a", target: "b" }, { source: "b", target: "c" }];
  const layers = assignLayers({ nodes, edges });
  const byId = Object.fromEntries(layers.map((l) => [l.id, l.layer]));
  assert.equal(byId.a, 0);
  assert.equal(byId.b, 1);
  assert.equal(byId.c, 2);
});

test("analytics digest is reproducible and input-bound", () => {
  const digest1 = analyticsDigest({ algorithm: "tarjan", inputGenerationId: "g1", params: {} });
  const digest2 = analyticsDigest({ algorithm: "tarjan", inputGenerationId: "g1", params: {} });
  const digest3 = analyticsDigest({ algorithm: "tarjan", inputGenerationId: "g2", params: {} });
  assert.equal(digest1, digest2);
  assert.notEqual(digest1, digest3);
});

test("architecture model exposes layers, coupling, and hotspots", () => {
  const model = buildArchitectureModel({
    nodes: [{ id: "a" }, { id: "b" }, { id: "c" }],
    edges: [{ source: "a", target: "b" }, { source: "b", target: "c" }, { source: "a", target: "c" }],
    generationId: "g1",
  });
  assert.equal(model.schemaVersion, 1);
  assert.equal(model.generationId, "g1");
  assert.ok(model.layers.length > 0);
  assert.ok(model.hotspots.some((h) => h.id === "a"));
  assert.equal(model.hotspots[0].candidate, true);
});

test("hotspots are ordered by degree with stable tie-break", () => {
  const model = buildArchitectureModel({
    nodes: [{ id: "x" }, { id: "y" }],
    edges: [{ source: "x", target: "y" }, { source: "x", target: "y" }],
    generationId: "g",
  });
  assert.equal(model.hotspots[0].id, "x");
});
