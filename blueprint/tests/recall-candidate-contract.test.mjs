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
// Every tier of the ordering, pinned individually. Asserting only the
// authority tier left five of the seven free to be reversed or deleted without
// any test noticing, which is most of the "non-compensatory by
// admissibility/evidence/seed/coverage/truth/analysis/hops/tie-break" claim
// going unproven.
test("each ordering tier decides in its declared position, and only when the tiers above it are equal", () => {
  const path = (id, overrides) => ({
    id, state: "complete", seedId: `symbol:${id}`, seedExactness: 0,
    semanticAuthorityRank: 0, minimumEdgeTier: "EXACT_RESOLUTION", evidenceCoverage: 1,
    hopCount: 1, evidenceEnvelope: { id: `envelope:${id}` },
    nodes: [], edges: [], evidence: [], ...overrides,
  });
  // Each case makes exactly one tier differ; everything above it is equal, so
  // the winner is decided by that tier alone.
  const cases = [
    ["state", { state: "partial" }],
    ["semanticAuthorityRank", { semanticAuthorityRank: 5 }],
    ["minimumEdgeTier", { minimumEdgeTier: "UNRESOLVED" }],
    ["seedExactness", { seedExactness: 9 }],
    ["evidenceCoverage", { evidenceCoverage: 0 }],
    ["hopCount", { hopCount: 9 }],
  ];
  for (const [tier, worse] of cases) {
    // The better path is given the LEXICALLY LATER id on purpose. If the tier
    // under test stops deciding, the chain falls through to the id tie-break
    // and produces the opposite order — so deleting the tier fails this
    // assertion instead of silently agreeing with it.
    const better = path("zzz");
    const loser = path("aaa", worse);
    assert.deepEqual(
      [loser, better].sort(comparePaths).map((entry) => entry.id),
      ["zzz", "aaa"],
      `${tier} must decide when every tier above it is equal`,
    );
  }
  // Tier PRECEDENCE, not just presence. Varying one tier at a time pins each
  // tier's existence and direction but leaves adjacent tiers free to be
  // swapped — and swapping them is exactly how a lower-priority signal starts
  // compensating for a higher-priority one, which is the failure BPT-026
  // forbids. Each case below makes TWO tiers disagree, so only the declared
  // precedence produces the asserted winner.
  const precedence = [
    ["state over semanticAuthorityRank", { state: "partial", semanticAuthorityRank: 0 }, { state: "complete", semanticAuthorityRank: 5 }],
    ["semanticAuthorityRank over minimumEdgeTier", { semanticAuthorityRank: 5, minimumEdgeTier: "EXACT_RESOLUTION" }, { semanticAuthorityRank: 0, minimumEdgeTier: "UNRESOLVED" }],
    ["minimumEdgeTier over seedExactness", { minimumEdgeTier: "UNRESOLVED", seedExactness: 0 }, { minimumEdgeTier: "EXACT_RESOLUTION", seedExactness: 9 }],
    ["seedExactness over evidenceCoverage", { seedExactness: 9, evidenceCoverage: 1 }, { seedExactness: 0, evidenceCoverage: 0 }],
    ["evidenceCoverage over hopCount", { evidenceCoverage: 0, hopCount: 1 }, { evidenceCoverage: 1, hopCount: 9 }],
  ];
  for (const [label, loserFields, winnerFields] of precedence) {
    // The winner again takes the lexically later id, so a collapsed chain
    // falls through to the tie-break and produces the opposite order.
    const winner = path("zzz", winnerFields);
    const loser = path("aaa", loserFields);
    assert.deepEqual(
      [loser, winner].sort(comparePaths).map((entry) => entry.id),
      ["zzz", "aaa"],
      `${label}: the higher tier must decide even when the lower one disagrees`,
    );
  }

  // `tierRank`'s mapping is part of the ordering, not an implementation
  // detail: every edge tier must rank strictly better than the next.
  const byTier = ["EXACT_RESOLUTION", "SAME_FILE_LEXICAL", "CROSS_FILE_HEURISTIC", "UNRESOLVED"];
  for (let i = 0; i + 1 < byTier.length; i += 1) {
    const better = path("zzz", { minimumEdgeTier: byTier[i] });
    const worse = path("aaa", { minimumEdgeTier: byTier[i + 1] });
    assert.deepEqual(
      [worse, better].sort(comparePaths).map((entry) => entry.id),
      ["zzz", "aaa"],
      `${byTier[i]} must rank strictly better than ${byTier[i + 1]}`,
    );
  }

  // The lexical tie-break is last and total: with every tier equal, ordering
  // is by id, so the result is deterministic rather than input-order dependent.
  assert.deepEqual(
    [path("zzz"), path("aaa")].sort(comparePaths).map((entry) => entry.id),
    ["aaa", "zzz"],
    "a full tie falls through to the deterministic id tie-break",
  );
});

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
