import { FACT_PROVENANCE, isInferentialProvenance } from "./provenance.mjs";

// Canon ordering from INV-005 / BPC-004. Lower rank is stronger.
export const SEMANTIC_AUTHORITY_ORDER = Object.freeze([
  FACT_PROVENANCE.AUTHORITATIVE_SEMANTIC,
  FACT_PROVENANCE.LIVE_VERIFICATION,
  FACT_PROVENANCE.RULE_RESOLVED,
  FACT_PROVENANCE.STRUCTURAL_RESOLVED,
  FACT_PROVENANCE.FRAMEWORK_RESOLVED,
  FACT_PROVENANCE.HEURISTIC_BRIDGE,
  FACT_PROVENANCE.UNRESOLVED,
]);

export const RESOLUTION_SPECIFICITY_ORDER = Object.freeze([
  "EXACT_RESOLUTION",
  "SAME_FILE_LEXICAL",
  "CROSS_FILE_HEURISTIC",
  "UNRESOLVED",
]);

const AUTHORITY_RANK = new Map(SEMANTIC_AUTHORITY_ORDER.map((value, index) => [value, index]));
const SPECIFICITY_RANK = new Map(RESOLUTION_SPECIFICITY_ORDER.map((value, index) => [value, index]));

const COHERENT_SOURCE_STATES = new Set(["equal", "current", "clean", "fresh"]);
const STALE_SOURCE_STATES = new Set(["stale", "behind", "ahead", "diverged"]);

export function sourceCoherenceRank(candidate, targetSourceState = null) {
  if (candidate?.sourceCoherent === true) return 0;
  if (candidate?.sourceCoherent === false) return 1;

  const relation = sourceRelation(candidate);
  if (COHERENT_SOURCE_STATES.has(relation)) return 0;
  if (STALE_SOURCE_STATES.has(relation)) return 1;

  // If both sides expose an exact opaque source-state identity, equality is a
  // valid coherence proof; inequality is stale relative to that target.
  const candidateIdentity = candidate?.sourceStateId ?? candidate?.generationId ?? null;
  const targetIdentity = typeof targetSourceState === "string"
    ? targetSourceState
    : targetSourceState?.id ?? targetSourceState?.generationId ?? null;
  if (candidateIdentity !== null && targetIdentity !== null) {
    return String(candidateIdentity) === String(targetIdentity) ? 0 : 1;
  }

  // Unknown never means current (INV-017).
  return 2;
}

export function semanticAuthorityForFact(candidate) {
  if (AUTHORITY_RANK.has(candidate?.provenance)) return candidate.provenance;

  // Compatibility classification while older structural facts are migrated to
  // explicit provenance. This is deliberately categorical; numeric confidence
  // never decides the class.
  const provider = String(candidate?.provider?.id ?? candidate?.provider ?? candidate?.sourceProvider?.id ?? "").toLowerCase();
  if (candidate?.precisionTier === "COMPILER" || provider.includes("scip") || provider.includes("compiler")) {
    return candidate?.resolved === false ? FACT_PROVENANCE.UNRESOLVED : FACT_PROVENANCE.AUTHORITATIVE_SEMANTIC;
  }
  if (candidate?.confidenceTier === "UNRESOLVED" || candidate?.resolved === false) return FACT_PROVENANCE.UNRESOLVED;
  if (candidate?.confidenceTier === "CROSS_FILE_HEURISTIC") return FACT_PROVENANCE.HEURISTIC_BRIDGE;
  if (candidate?.confidenceTier === "SAME_FILE_LEXICAL") return FACT_PROVENANCE.STRUCTURAL_RESOLVED;
  if (candidate?.confidenceTier === "EXACT_RESOLUTION") return FACT_PROVENANCE.RULE_RESOLVED;
  return FACT_PROVENANCE.UNRESOLVED;
}

export function semanticAuthorityRankForFact(candidate) {
  return AUTHORITY_RANK.get(semanticAuthorityForFact(candidate)) ?? SEMANTIC_AUTHORITY_ORDER.length;
}

export function resolutionSpecificityRank(candidate) {
  return SPECIFICITY_RANK.get(candidate?.confidenceTier) ?? RESOLUTION_SPECIFICITY_ORDER.length;
}

function sourceRelation(candidate) {
  const raw = candidate?.sourceRelation
    ?? candidate?.sourceState?.relation
    ?? candidate?.sourceState
    ?? candidate?.freshness?.relation
    ?? candidate?.freshness;
  return typeof raw === "string" ? raw.toLowerCase() : "unknown";
}

function requestedRelationMatches(candidate, requestedRelation) {
  if (!requestedRelation) return true;
  const kind = candidate?.relation ?? candidate?.kind ?? null;
  return kind === null || String(kind) === String(requestedRelation);
}

function admissible(candidate, requestedRelation) {
  return candidate !== null
    && typeof candidate === "object"
    && candidate.admissible !== false
    && candidate.scopeAllowed !== false
    && requestedRelationMatches(candidate, requestedRelation);
}

function inferentialConfidence(candidate) {
  const provenance = semanticAuthorityForFact(candidate);
  if (!isInferentialProvenance(provenance)) return null;
  const value = Number(candidate?.confidence);
  return Number.isFinite(value) && value >= 0 && value <= 1 ? value : null;
}

function vector(candidate, targetSourceState) {
  return Object.freeze({
    coherence: sourceCoherenceRank(candidate, targetSourceState),
    authority: semanticAuthorityRankForFact(candidate),
    specificity: resolutionSpecificityRank(candidate),
    inferentialConfidence: inferentialConfidence(candidate),
  });
}

function compareVectors(left, right) {
  return left.coherence - right.coherence
    || left.authority - right.authority
    || left.specificity - right.specificity
    // Confidence is legal only inside the heuristic class and only after the
    // categorical dimensions above have tied.
    || (right.inferentialConfidence ?? -1) - (left.inferentialConfidence ?? -1);
}

function sameCategoricalVector(left, right) {
  return left.coherence === right.coherence
    && left.authority === right.authority
    && left.specificity === right.specificity;
}

function candidateTarget(candidate) {
  return candidate?.target ?? candidate?.targetId ?? candidate?.entityId ?? candidate?.id ?? null;
}

function frontier(reason, candidates, details = {}) {
  return Object.freeze({
    state: "unresolved_frontier",
    reason,
    admitted: null,
    candidates: Object.freeze(candidates.map((candidate) => Object.freeze({
      candidate,
      vector: candidate.__authorityVector,
    }))),
    ...details,
  });
}

/**
 * Evaluate competing evidence for one requested relationship.
 *
 * Governing order is non-compensatory:
 *   admissibility -> source coherence -> authority -> specificity ->
 *   inferential confidence (heuristic evidence only).
 *
 * A stale compiler fact therefore cannot beat a current structural fact, and
 * scalar confidence can never outvote a stronger categorical source.
 */
export function evaluateEvidence({ targetSourceState = null, candidates = [], requestedRelation = null } = {}) {
  const admittedCandidates = candidates
    .filter((candidate) => admissible(candidate, requestedRelation))
    .map((candidate) => ({ ...candidate, __authorityVector: vector(candidate, targetSourceState) }));

  if (!admittedCandidates.length) return frontier("no_admissible_evidence", []);

  admittedCandidates.sort((left, right) => compareVectors(left.__authorityVector, right.__authorityVector)
    || String(left.id ?? "").localeCompare(String(right.id ?? "")));

  const best = admittedCandidates[0];
  const bestVector = best.__authorityVector;

  if (bestVector.coherence !== 0) {
    return frontier("no_source_coherent_evidence", admittedCandidates, { bestVector });
  }
  if (semanticAuthorityForFact(best) === FACT_PROVENANCE.UNRESOLVED) {
    return frontier("resolution_unresolved", admittedCandidates, { bestVector });
  }

  const categoricalPeers = admittedCandidates.filter((candidate) => sameCategoricalVector(candidate.__authorityVector, bestVector));
  let finalists = categoricalPeers;
  if (isInferentialProvenance(semanticAuthorityForFact(best))) {
    const bestConfidence = bestVector.inferentialConfidence;
    finalists = categoricalPeers.filter((candidate) => candidate.__authorityVector.inferentialConfidence === bestConfidence);
  }

  const targets = new Set(finalists.map(candidateTarget));
  if (targets.size > 1) {
    return frontier("authority_tie_conflict", finalists, {
      bestVector,
      targets: Object.freeze([...targets].map(String).sort()),
    });
  }

  const clean = { ...best };
  delete clean.__authorityVector;
  return Object.freeze({
    state: "admitted",
    reason: "categorical_precedence",
    admitted: Object.freeze(clean),
    vector: bestVector,
    equivalentEvidence: Object.freeze(finalists.map((candidate) => {
      const value = { ...candidate };
      delete value.__authorityVector;
      return Object.freeze(value);
    })),
  });
}
