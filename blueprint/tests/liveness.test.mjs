import assert from "node:assert/strict";
import test from "node:test";
import { buildEntryPointRegistry } from "../src/graph/entry-points.mjs";
import { buildLivenessProjection, LIVENESS_STATES } from "../src/graph/liveness.mjs";

const ev = (path) => [{ path, startLine: 1, endLine: 1, contentHash: `${path}-hash` }];
const generation = {
  manifest: { generationId: "g-live", complete: true },
  nodes: [
    { id: "entry", kind: "symbol", path: "src/main.ts", labels: ["Function", "EntryPoint"], evidence: ev("src/main.ts") },
    { id: "live", kind: "symbol", path: "src/live.ts", labels: ["Function"], evidence: ev("src/live.ts") },
    { id: "orphan", kind: "symbol", path: "src/orphan.ts", labels: ["Function"], evidence: ev("src/orphan.ts") },
    { id: "candidate", kind: "symbol", path: "src/candidate.ts", labels: ["Function"], evidence: ev("src/candidate.ts") },
    { id: "leaf", kind: "symbol", path: "src/leaf.ts", labels: ["Function"], evidence: ev("src/leaf.ts") },
  ],
  edges: [
    { id: "e1", source: "entry", target: "live", resolved: true, confidenceTier: "EXACT_RESOLUTION", evidence: ev("src/main.ts") },
    { id: "e2", source: "candidate", target: "leaf", resolved: true, confidenceTier: "EXACT_RESOLUTION", evidence: ev("src/candidate.ts") },
  ],
};

test("entry registry separates explicit evidence from zero-inbound structural candidates", () => {
  const rows = buildEntryPointRegistry(generation);
  assert.equal(rows.find((row) => row.id === "entry").authority, "explicit");
  assert.equal(rows.find((row) => row.id === "candidate").authority, "structural_candidate");
});

test("liveness emits only LIVE UNREACHED UNKNOWN and never calls zero-inbound dead", () => {
  const result = buildLivenessProjection(generation);
  assert.deepEqual(LIVENESS_STATES, ["LIVE", "UNREACHED", "UNKNOWN"]);
  assert.equal(result.results.find((row) => row.nodeId === "entry").state, "LIVE");
  assert.equal(result.results.find((row) => row.nodeId === "live").state, "LIVE");
  assert.equal(result.results.find((row) => row.nodeId === "candidate").state, "UNREACHED");
  assert.equal(result.results.find((row) => row.nodeId === "orphan").state, "UNREACHED");
  assert.ok(result.results.every((row) => !["DEAD", "UNUSED"].includes(row.state)));
  assert.deepEqual(result.results.find((row) => row.nodeId === "live").reachabilityPath, ["entry", "live"]);
});

test("liveness fails to UNKNOWN when source or entrypoint basis is not trustworthy", () => {
  const stale = buildLivenessProjection(generation, { sourceState: "stale" });
  assert.ok(stale.results.every((row) => row.state === "UNKNOWN"));
  const noExplicit = { ...generation, nodes: generation.nodes.map((node) => ({ ...node, labels: (node.labels ?? []).filter((label) => label !== "EntryPoint") })) };
  assert.ok(buildLivenessProjection(noExplicit).results.every((row) => row.state === "UNKNOWN"));
  const incomplete = { ...generation, manifest: { ...generation.manifest, complete: false } };
  assert.ok(buildLivenessProjection(incomplete).results.every((row) => row.state === "UNKNOWN"));
});
