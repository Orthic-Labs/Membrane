import assert from "node:assert/strict";
import test from "node:test";

import {
  FACT_PROVENANCE,
  compilerSemanticFact,
  confidenceForProvenance,
  heuristicFact,
  isInferentialProvenance,
  withFactProvenance,
} from "../src/graph/provenance.mjs";

test("authoritative and deterministic provenance never expose fake probability", () => {
  for (const provenance of [
    FACT_PROVENANCE.AUTHORITATIVE_SEMANTIC,
    FACT_PROVENANCE.LIVE_VERIFICATION,
    FACT_PROVENANCE.RULE_RESOLVED,
    FACT_PROVENANCE.STRUCTURAL_RESOLVED,
    FACT_PROVENANCE.FRAMEWORK_RESOLVED,
    FACT_PROVENANCE.UNRESOLVED,
  ]) {
    assert.equal(isInferentialProvenance(provenance), false);
    assert.equal(confidenceForProvenance(provenance, 1), null);
    const fact = withFactProvenance({ id: "fact", confidence: 0.99 }, provenance);
    assert.equal(fact.provenance, provenance);
    assert.equal(fact.confidence, null);
  }
});

test("heuristic bridge is the only provenance class that carries inferential confidence", () => {
  const fact = heuristicFact({ id: "edge:heuristic" }, 0.78);
  assert.equal(fact.provenance, "HEURISTIC_BRIDGE");
  assert.equal(fact.confidence, 0.78);
  assert.equal(isInferentialProvenance(fact.provenance), true);
  assert.throws(() => heuristicFact({ id: "bad" }, 1.1), /\[0,1\]/);
});

test("compiler semantic helper separates resolved authority from unresolved frontier", () => {
  const resolved = compilerSemanticFact({ id: "symbol:x", confidence: 1 });
  assert.equal(resolved.provenance, "AUTHORITATIVE_SEMANTIC");
  assert.equal(resolved.confidence, null);

  const unresolved = compilerSemanticFact({ id: "edge:x", confidence: 0 }, { resolved: false });
  assert.equal(unresolved.provenance, "UNRESOLVED");
  assert.equal(unresolved.confidence, null);
});
