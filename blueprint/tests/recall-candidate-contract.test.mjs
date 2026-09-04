import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { canonicalCandidateSet, recallCircuitToCandidateSet } from "../src/graph/recall-circuit.mjs";
import { validateJsonSchema } from "./python-test-runtime.mjs";

const schema = JSON.parse(readFileSync(new URL("../schemas/context-candidate-set.v1.schema.json", import.meta.url), "utf8"));
function circuit(paths = []) {
  return { id: "recall:test", task: "inspect work", generationId: "generation:test", state: paths.length ? "complete" : "abstained",
    bounds: { maxPaths: 10 }, paths, omissions: paths.length ? [] : [{ reason: "no_relevant_seed", lane: "seed" }] };
}

test("empty Recall projection preserves typed omission and validates as strict V1", () => {
  const raw = circuit();
  const before = JSON.stringify(raw);
  const result = recallCircuitToCandidateSet(raw, { canonical: true });
  const validation = validateJsonSchema(schema, result);
  assert.equal(validation.valid, true, JSON.stringify(validation.errors));
  assert.deepEqual(result.omissions, [{ id: "recall:test:omission:0", reason: "no_relevant_seed" }]);
  assert.equal(JSON.stringify(raw), before);
});

test("V1 strips only internal fields while rich Recall output retains path evidence", () => {
  const raw = circuit([{ id: "path:test", state: "complete", seedId: "symbol:x", seedExactness: 1,
    minimumEdgeTier: "EXACT_RESOLUTION", evidenceCoverage: 1, semanticAuthorityRank: 0,
    evidenceEnvelope: { id: "envelope:test", policy: "data_only" },
    nodes: [{ id: "symbol:x", name: "work", path: "x.ts", evidence: [{ path: "x.ts", startLine: 1, endLine: 1, contentHash: "1".repeat(32) }] }],
    edges: [], evidence: [] }]);
  const rich = recallCircuitToCandidateSet(raw);
  const before = JSON.stringify(rich);
  const canonical = canonicalCandidateSet(rich);
  const validation = validateJsonSchema(schema, canonical);
  assert.equal(validation.valid, true, JSON.stringify(validation.errors));
  for (const key of ["recallCircuitId", "evidencePathId", "evidenceEnvelope"]) {
    assert.ok(key in rich.candidates[0]);
    assert.ok(!(key in canonical.candidates[0]));
  }
  assert.deepEqual(raw.paths[0].evidenceEnvelope, rich.candidates[0].evidenceEnvelope);
  assert.equal(canonical.candidates[0].instructionPolicy, "data_only");
  assert.equal(JSON.stringify(rich), before);
});
