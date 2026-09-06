import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { canonicalCandidateSet, comparePaths, recallCircuitToCandidateSet } from "../src/graph/recall-circuit.mjs";

const sortedByCircuitComparator = (paths) => [...paths].sort(comparePaths);
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

// BPT-026: comparePaths (recall-circuit.mjs) is a non-compensatory
// lexicographic chain — semanticAuthorityRank precedes evidence tier, which
// precedes coverage, with no summing or weighting. The emitted
// `providerScore` / `scoreComponents` on CandidateV1 are presentational
// only; nothing may recover a compensatory ranking by summing them. This
// test constructs two paths where the non-compensatory comparator and a
// naive sum of `scoreComponents` DISAGREE on order, and proves the emitted
// candidate order follows the comparator, not the sum.
test("emitted candidate order follows the non-compensatory comparator, not a sum of scoreComponents", () => {
  const strongerAuthorityWeakerEverythingElse = {
    id: "path:a", state: "complete", seedId: "symbol:a", seedExactness: 1,
    // Best (lowest) semanticAuthorityRank, but worst edge tier and weak coverage.
    semanticAuthorityRank: 0, minimumEdgeTier: "UNRESOLVED", evidenceCoverage: 0.1,
    hopCount: 1, evidenceEnvelope: { id: "envelope:a" },
    nodes: [{ id: "symbol:a", name: "a", path: "a.ts", evidence: [{ path: "a.ts", startLine: 1, endLine: 1, contentHash: "a".repeat(32) }] }],
    edges: [], evidence: [],
  };
  const weakerAuthorityStrongerEverythingElse = {
    id: "path:b", state: "complete", seedId: "symbol:b", seedExactness: 1,
    // Worse semanticAuthorityRank, but best edge tier and full coverage.
    semanticAuthorityRank: 1, minimumEdgeTier: "EXACT_RESOLUTION", evidenceCoverage: 1,
    hopCount: 1, evidenceEnvelope: { id: "envelope:b" },
    nodes: [{ id: "symbol:b", name: "b", path: "b.ts", evidence: [{ path: "b.ts", startLine: 1, endLine: 1, contentHash: "b".repeat(32) }] }],
    edges: [], evidence: [],
  };
  // comparePaths ranks by semanticAuthorityRank BEFORE tier/coverage, so the
  // non-compensatory order is [a, b] even though b's edge tier and coverage
  // are both strictly better.
  // `recallCircuitToCandidateSet` does NOT sort — it preserves the order
  // `executeRecallCircuit` already established with `comparePaths`. Handing it
  // the paths already in comparator order and asserting that order comes back
  // would pass with the comparator deleted, so the two properties are proven
  // separately and the conversion is fed the WRONG order on purpose.
  const reversed = circuit([weakerAuthorityStrongerEverythingElse, strongerAuthorityWeakerEverythingElse]);
  assert.deepEqual(
    recallCircuitToCandidateSet(reversed).candidates.map((c) => c.id),
    ["symbol:b", "symbol:a"],
    "conversion must preserve the order it is given; ranking authority lives upstream in comparePaths",
  );

  // The ordering property itself, proven where it is actually decided.
  const ordered = sortedByCircuitComparator([
    weakerAuthorityStrongerEverythingElse,
    strongerAuthorityWeakerEverythingElse,
  ]);
  assert.deepEqual(ordered.map((path) => path.id), ["path:a", "path:b"],
    "semanticAuthorityRank must decide before edge tier and coverage, whatever order the paths arrive in");

  const raw = circuit(ordered);
  const result = recallCircuitToCandidateSet(raw);
  assert.deepEqual(result.candidates.map((c) => c.id), ["symbol:a", "symbol:b"]);

  const sums = result.candidates.map((c) => Object.values(c.scoreComponents).reduce((total, value) => total + value, 0));
  const [sumA, sumB] = sums;
  assert.ok(sumB > sumA,
    "fixture must actually make summed scoreComponents disagree with the comparator, or this test proves nothing");
  const sumOrder = [...result.candidates]
    .map((candidate, index) => ({ candidate, sum: sums[index] }))
    .sort((left, right) => right.sum - left.sum)
    .map((entry) => entry.candidate.id);
  assert.notDeepEqual(sumOrder, result.candidates.map((c) => c.id),
    "summing scoreComponents must NOT reproduce the emitted (comparator) order — proving scoreComponents cannot reintroduce compensatory ranking");
});
