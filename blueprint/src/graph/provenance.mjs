// Canonical provenance semantics for Blueprint facts.
//
// Provenance answers "what kind of evidence produced this fact?". Confidence
// answers a different question and is only meaningful for explicitly inferred
// facts. Deterministic/compiler/source-resolved facts therefore carry
// `confidence: null`; they are not 100%-probability guesses.

export const FACT_PROVENANCE = Object.freeze({
  AUTHORITATIVE_SEMANTIC: "AUTHORITATIVE_SEMANTIC",
  LIVE_VERIFICATION: "LIVE_VERIFICATION",
  RULE_RESOLVED: "RULE_RESOLVED",
  STRUCTURAL_RESOLVED: "STRUCTURAL_RESOLVED",
  FRAMEWORK_RESOLVED: "FRAMEWORK_RESOLVED",
  HEURISTIC_BRIDGE: "HEURISTIC_BRIDGE",
  UNRESOLVED: "UNRESOLVED",
});

const KNOWN = new Set(Object.values(FACT_PROVENANCE));
const INFERENTIAL = new Set([FACT_PROVENANCE.HEURISTIC_BRIDGE]);

export function isInferentialProvenance(provenance) {
  assertKnownProvenance(provenance);
  return INFERENTIAL.has(provenance);
}

export function confidenceForProvenance(provenance, confidence = null) {
  assertKnownProvenance(provenance);
  if (!INFERENTIAL.has(provenance)) return null;
  if (confidence === null || confidence === undefined) return null;
  const value = Number(confidence);
  if (!Number.isFinite(value) || value < 0 || value > 1) {
    throw new TypeError(`inferential confidence must be a finite number in [0,1], got ${String(confidence)}`);
  }
  return value;
}

export function withFactProvenance(fact, provenance, confidence = fact?.confidence ?? null) {
  assertKnownProvenance(provenance);
  return {
    ...fact,
    provenance,
    confidence: confidenceForProvenance(provenance, confidence),
  };
}

export function compilerSemanticFact(fact, { resolved = true } = {}) {
  return withFactProvenance(
    fact,
    resolved ? FACT_PROVENANCE.AUTHORITATIVE_SEMANTIC : FACT_PROVENANCE.UNRESOLVED,
    null,
  );
}

export function heuristicFact(fact, confidence) {
  return withFactProvenance(fact, FACT_PROVENANCE.HEURISTIC_BRIDGE, confidence);
}

export function assertKnownProvenance(provenance) {
  if (!KNOWN.has(provenance)) throw new TypeError(`unknown Blueprint provenance class: ${String(provenance)}`);
  return provenance;
}
