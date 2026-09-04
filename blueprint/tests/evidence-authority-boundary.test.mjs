import assert from "node:assert/strict";
import test from "node:test";
import { evaluateEvidence, semanticAuthorityForFact, semanticAuthorityRankForFact, sourceCoherenceRank } from "../src/graph/evidence-authority.mjs";
import { confidenceForProvenance } from "../src/graph/provenance.mjs";

const fact = (extra = {}) => ({
  id: "candidate", kind: "REFERENCES", target: "symbol:current", resolved: true,
  provenance: "STRUCTURAL_RESOLVED", confidenceTier: "EXACT_RESOLUTION",
  confidence: null, sourceRelation: "current", ...extra,
});
const evaluate = (candidates, extra = {}) => evaluateEvidence({ candidates, requestedRelation: "REFERENCES", ...extra });

test("optimistic current flags cannot override a mismatched source identity", () => {
  const stale = fact({ id: "scip", provenance: "AUTHORITATIVE_SEMANTIC", sourceStateId: "old", sourceCoherent: true });
  const current = fact({ id: "ast", sourceStateId: "new" });
  assert.equal(sourceCoherenceRank(stale, "new"), 1);
  const result = evaluate([stale, current], { targetSourceState: "new" });
  assert.equal(result.admitted.id, "ast");
  assert.equal(evaluate([stale], { targetSourceState: "new" }).reason, "no_source_coherent_evidence");
});

test("explicit stale observations dominate current flags and matching identity", () => {
  for (const relation of ["stale", "behind", "ahead", "diverged"]) {
    assert.equal(sourceCoherenceRank(fact({ sourceCoherent: true, sourceRelation: relation, sourceStateId: "same" }), "same"), 1);
  }
  assert.equal(sourceCoherenceRank(fact({ sourceCoherent: false, sourceStateId: "same" }), "same"), 1);
});

test("unknown or object identities never become current through string coercion", () => {
  assert.equal(sourceCoherenceRank(fact(), "new"), 2);
  assert.equal(sourceCoherenceRank(fact({ sourceStateId: {} }), {}), 2);
  assert.equal(sourceCoherenceRank(fact({ sourceStateId: "" })), 2);
  assert.equal(sourceCoherenceRank(fact({ sourceStateId: 42 }), "42"), 1);
  assert.equal(sourceCoherenceRank(fact({ sourceStateId: 42 }), 42), 0);
});

test("unresolved markers and unknown provenance cannot acquire compiler authority", () => {
  assert.equal(semanticAuthorityForFact(fact({ provenance: "AUTHORITATIVE_SEMANTIC", resolved: false })), "UNRESOLVED");
  assert.equal(semanticAuthorityForFact(fact({ provenance: "AUTHORITATIVE_SEMANTIC", confidenceTier: "UNRESOLVED" })), "UNRESOLVED");
  assert.equal(semanticAuthorityForFact(fact({ provenance: "unknown-class", provider: "scip-python", precisionTier: "COMPILER" })), "UNRESOLVED");
  assert.equal(semanticAuthorityForFact({ provider: "docs-about-compiler" }), "UNRESOLVED");
  assert.equal(evaluate([fact({ resolved: false, provenance: "AUTHORITATIVE_SEMANTIC" })]).reason, "resolution_unresolved");
});

test("required relationship and real target cannot be replaced by an edge id", () => {
  const missingKind = fact(); delete missingKind.kind;
  assert.equal(evaluate([missingKind]).reason, "no_admissible_evidence");
  const missingTarget = fact(); delete missingTarget.target;
  assert.equal(evaluate([missingTarget]).reason, "resolution_target_missing");
  assert.equal(evaluate([fact({ target: null, targetId: "symbol:other" })]).reason, "resolution_target_missing");
});

test("inference confidence rejects coercible nonnumbers rather than inventing probabilities", () => {
  for (const value of ["0.9", "", true, false, [], [0.8], NaN, Infinity, -0.1, 1.1]) {
    assert.throws(() => confidenceForProvenance("HEURISTIC_BRIDGE", value), TypeError);
    const result = evaluate([fact({ provenance: "HEURISTIC_BRIDGE", confidence: value })]);
    assert.equal(result.reason, "invalid_inferential_confidence");
  }
});

test("null inferential confidence remains unknown, not numeric zero", () => {
  const inferred = fact({ provenance: "HEURISTIC_BRIDGE", confidence: null });
  assert.equal(evaluate([inferred]).vector.inferentialConfidence, null);
  assert.equal(evaluate([inferred]).admitted.confidence, null);
  assert.equal(confidenceForProvenance("HEURISTIC_BRIDGE", 0), 0);
});

test("admission normalizes tagged legacy confidence without mutating input", () => {
  const legacy = fact({ provenance: "AUTHORITATIVE_SEMANTIC", confidence: 1 });
  const result = evaluate([legacy]);
  assert.equal(result.admitted.confidence, null);
  assert.equal(result.equivalentEvidence[0].confidence, null);
  assert.equal(legacy.confidence, 1);
});

test("LSP verification cannot originate or outrank a canonical graph fact", () => {
  const verification = fact({ id: "lsp", provenance: "LIVE_VERIFICATION" });
  assert.equal(evaluate([verification]).reason, "verification_without_canonical_evidence");
  assert.ok(semanticAuthorityRankForFact(verification) > semanticAuthorityRankForFact(fact()));
  const result = evaluate([verification, fact()]);
  assert.equal(result.admitted.id, "candidate");
  assert.equal(result.verifications.length, 1);
  assert.equal(result.verifications[0].provenance, "LIVE_VERIFICATION");
});

test("coherent LSP disagreement returns resolution_conflict without rewriting source truth", () => {
  const canonical = fact();
  const verification = fact({ id: "lsp", target: "symbol:other", provenance: "LIVE_VERIFICATION" });
  const result = evaluate([canonical, verification]);
  assert.equal(result.state, "unresolved_frontier");
  assert.equal(result.reason, "resolution_conflict");
  assert.equal(result.admitted, null);
  assert.equal(canonical.target, "symbol:current");
  const staleCheck = evaluate([canonical, { ...verification, sourceRelation: "stale" }]);
  assert.equal(staleCheck.admitted.target, "symbol:current");
  assert.equal(staleCheck.verifications.length, 1, "stale check remains visible but cannot rewrite truth");
});
