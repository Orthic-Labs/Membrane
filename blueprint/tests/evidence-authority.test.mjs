import assert from "node:assert/strict";
import test from "node:test";

import {
  evaluateEvidence,
  semanticAuthorityForFact,
  semanticAuthorityRankForFact,
  sourceCoherenceRank,
} from "../src/graph/evidence-authority.mjs";

test("freshness precedes authority: current structural fact beats stale compiler fact", () => {
  const result = evaluateEvidence({
    requestedRelation: "CALLS",
    candidates: [
      {
        id: "compiler-stale",
        kind: "CALLS",
        target: "symbol:old",
        provenance: "AUTHORITATIVE_SEMANTIC",
        confidenceTier: "EXACT_RESOLUTION",
        sourceRelation: "behind",
        confidence: null,
      },
      {
        id: "structural-current",
        kind: "CALLS",
        target: "symbol:current",
        provenance: "STRUCTURAL_RESOLVED",
        confidenceTier: "SAME_FILE_LEXICAL",
        sourceRelation: "equal",
        confidence: null,
      },
    ],
  });
  assert.equal(result.state, "admitted");
  assert.equal(result.admitted.id, "structural-current");
  assert.equal(result.vector.coherence, 0);
});

test("authority precedes inferential confidence: compiler fact beats high-confidence heuristic", () => {
  const result = evaluateEvidence({
    candidates: [
      {
        id: "heuristic",
        target: "symbol:guess",
        provenance: "HEURISTIC_BRIDGE",
        confidenceTier: "CROSS_FILE_HEURISTIC",
        sourceRelation: "equal",
        confidence: 0.999,
      },
      {
        id: "compiler",
        target: "symbol:exact",
        provenance: "AUTHORITATIVE_SEMANTIC",
        confidenceTier: "EXACT_RESOLUTION",
        sourceRelation: "equal",
        confidence: null,
      },
    ],
  });
  assert.equal(result.state, "admitted");
  assert.equal(result.admitted.id, "compiler");
});

test("equal categorical authority with conflicting targets returns a frontier, not a guessed winner", () => {
  const result = evaluateEvidence({
    candidates: [
      { id: "a", target: "symbol:a", provenance: "RULE_RESOLVED", confidenceTier: "EXACT_RESOLUTION", sourceRelation: "equal" },
      { id: "b", target: "symbol:b", provenance: "RULE_RESOLVED", confidenceTier: "EXACT_RESOLUTION", sourceRelation: "equal" },
    ],
  });
  assert.equal(result.state, "unresolved_frontier");
  assert.equal(result.reason, "authority_tie_conflict");
  assert.deepEqual(result.targets, ["symbol:a", "symbol:b"]);
});

test("inferential confidence is a final tie-break only within heuristic evidence", () => {
  const result = evaluateEvidence({
    candidates: [
      { id: "weak", target: "symbol:weak", provenance: "HEURISTIC_BRIDGE", confidenceTier: "CROSS_FILE_HEURISTIC", sourceRelation: "equal", confidence: 0.61 },
      { id: "strong", target: "symbol:strong", provenance: "HEURISTIC_BRIDGE", confidenceTier: "CROSS_FILE_HEURISTIC", sourceRelation: "equal", confidence: 0.84 },
    ],
  });
  assert.equal(result.state, "admitted");
  assert.equal(result.admitted.id, "strong");
});

test("unknown never collapses to current", () => {
  const result = evaluateEvidence({
    candidates: [
      { id: "unknown", target: "symbol:x", provenance: "AUTHORITATIVE_SEMANTIC", confidenceTier: "EXACT_RESOLUTION", sourceRelation: "unknown" },
    ],
  });
  assert.equal(sourceCoherenceRank({ sourceRelation: "unknown" }), 2);
  assert.equal(result.state, "unresolved_frontier");
  assert.equal(result.reason, "no_source_coherent_evidence");
});

test("inadmissible and wrong-relation candidates cannot participate", () => {
  const result = evaluateEvidence({
    requestedRelation: "CALLS",
    candidates: [
      { id: "wrong", kind: "IMPORTS", target: "file:x", provenance: "AUTHORITATIVE_SEMANTIC", confidenceTier: "EXACT_RESOLUTION", sourceRelation: "equal" },
      { id: "blocked", kind: "CALLS", target: "symbol:x", provenance: "AUTHORITATIVE_SEMANTIC", confidenceTier: "EXACT_RESOLUTION", sourceRelation: "equal", scopeAllowed: false },
    ],
  });
  assert.equal(result.state, "unresolved_frontier");
  assert.equal(result.reason, "no_admissible_evidence");
});

test("legacy facts are mapped categorically without consulting scalar confidence", () => {
  assert.equal(semanticAuthorityForFact({ provider: "scip-python", precisionTier: "COMPILER", confidence: 0.01 }), "AUTHORITATIVE_SEMANTIC");
  assert.equal(semanticAuthorityForFact({ confidenceTier: "CROSS_FILE_HEURISTIC", confidence: 1 }), "HEURISTIC_BRIDGE");
  assert.ok(
    semanticAuthorityRankForFact({ provider: "scip-python", precisionTier: "COMPILER", confidence: 0.01 })
      < semanticAuthorityRankForFact({ confidenceTier: "CROSS_FILE_HEURISTIC", confidence: 1 }),
  );
});

test("same target from equivalent evidence is admitted and retained as equivalent support", () => {
  const result = evaluateEvidence({
    candidates: [
      { id: "a", target: "symbol:x", provenance: "RULE_RESOLVED", confidenceTier: "EXACT_RESOLUTION", sourceRelation: "equal" },
      { id: "b", target: "symbol:x", provenance: "RULE_RESOLVED", confidenceTier: "EXACT_RESOLUTION", sourceRelation: "equal" },
    ],
  });
  assert.equal(result.state, "admitted");
  assert.equal(result.admitted.target, "symbol:x");
  assert.equal(result.equivalentEvidence.length, 2);
});
