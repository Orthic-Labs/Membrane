import { FACT_PROVENANCE, isInferentialProvenance, withFactProvenance } from "./provenance.mjs";

// Canon ordering from INV-005 / BPC-004. Lower rank is stronger.
// LIVE_VERIFICATION is a cross-check receipt, not a canonical producer (§5.2).
export const SEMANTIC_AUTHORITY_ORDER = Object.freeze([
  FACT_PROVENANCE.AUTHORITATIVE_SEMANTIC,
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

const KNOWN_PROVENANCE = new Set(Object.values(FACT_PROVENANCE));
const LEGACY_COMPILER_PROVIDERS = new Set(["blueprint-scip", "scip-python"]);
const AUTHORITY_RANK = new Map(SEMANTIC_AUTHORITY_ORDER.map((value, index) => [value, index]));
const SPECIFICITY_RANK = new Map(RESOLUTION_SPECIFICITY_ORDER.map((value, index) => [value, index]));

const COHERENT_SOURCE_STATES = new Set(["equal", "current", "clean", "fresh"]);
const STALE_SOURCE_STATES = new Set(["stale", "behind", "ahead", "diverged"]);

// These values come from the caller's source observation. This checks their
// consistency; it does not itself observe files or prove workspace freshness.
function usableIdentity(value) {
  return (typeof value === "string" && value.length > 0)
    || (typeof value === "number" && Number.isSafeInteger(value) && value >= 0);
}

export function sourceCoherenceRank(candidate, targetSourceState = null) {
  const relation = sourceRelation(candidate);
  // Negative observations dominate optimistic flags, including contradictory
  // metadata. Unknown identities are never coerced to equal strings.
  if (candidate?.sourceCoherent === false || STALE_SOURCE_STATES.has(relation)) return 1;
  const identity = candidate?.sourceStateId ?? candidate?.generationId ?? null;
  if (identity !== null && !usableIdentity(identity)) return 2;
  if (targetSourceState !== null) {
    const target = typeof targetSourceState === "string" || typeof targetSourceState === "number"
      ? targetSourceState
      : targetSourceState?.id ?? targetSourceState?.generationId ?? null;
    if (!usableIdentity(identity) || !usableIdentity(target)) return 2;
    return identity === target ? 0 : 1;
  }
  if (candidate?.sourceCoherent === true || COHERENT_SOURCE_STATES.has(relation)) return 0;
  return 2;
}

export function semanticAuthorityForFact(candidate) {
  if (candidate?.resolved === false || candidate?.confidenceTier === "UNRESOLVED") return FACT_PROVENANCE.UNRESOLVED;
  if (candidate?.provenance !== undefined && candidate?.provenance !== null) {
    // An unknown explicit class is malformed, not permission to fall back to
    // a stronger legacy provider classification.
    return KNOWN_PROVENANCE.has(candidate.provenance) ? candidate.provenance : FACT_PROVENANCE.UNRESOLVED;
  }
  // Compatibility for untagged V1 facts. Numeric confidence never selects a
  // class, and a producer's name merely containing "compiler" proves nothing.
  if (candidate?.confidenceTier === "CROSS_FILE_HEURISTIC") return FACT_PROVENANCE.HEURISTIC_BRIDGE;
  const provider = candidate?.provider?.id ?? candidate?.provider ?? candidate?.sourceProvider?.id ?? "";
  if (candidate?.precisionTier === "COMPILER" || LEGACY_COMPILER_PROVIDERS.has(provider)) return FACT_PROVENANCE.AUTHORITATIVE_SEMANTIC;
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
  return typeof kind === "string" && kind === requestedRelation;
}

function admissible(candidate, requestedRelation) {
  return candidate !== null
    && typeof candidate === "object"
    && !Array.isArray(candidate)
    && candidate.admissible !== false
    && candidate.scopeAllowed !== false
    && requestedRelationMatches(candidate, requestedRelation);
}

function inferentialConfidence(candidate) {
  const provenance = semanticAuthorityForFact(candidate);
  if (!isInferentialProvenance(provenance)) return null;
  const value = candidate?.confidence;
  return typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1 ? value : null;
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

function candidateTarget(candidate, requestedRelation = null) {
  for (const key of ["target", "targetId", "entityId"]) {
    if (Object.hasOwn(candidate, key)) return candidate[key];
  }
  // A node can identify itself; an edge cannot use its own id as a missing
  // target. In particular target:null is not targetId/id fallback permission.
  return requestedRelation || Object.hasOwn(candidate, "source") ? null : candidate?.id ?? null;
}

function normalizedCandidate(candidate, provenance = semanticAuthorityForFact(candidate)) {
  const clean = { ...candidate };
  delete clean.__authorityVector;
  return Object.freeze(withFactProvenance(clean, provenance));
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
  const admissibleCandidates = candidates
    .filter((candidate) => admissible(candidate, requestedRelation))
    .map((candidate) => ({ ...candidate, __authorityVector: vector(candidate, targetSourceState) }));

  if (!admissibleCandidates.length) return frontier("no_admissible_evidence", []);
  const invalidConfidence = admissibleCandidates.filter((candidate) =>
    isInferentialProvenance(semanticAuthorityForFact(candidate))
    && candidate.confidence !== null && candidate.confidence !== undefined
    && candidate.__authorityVector.inferentialConfidence === null);
  if (invalidConfidence.length) return frontier("invalid_inferential_confidence", invalidConfidence);

  const verifications = admissibleCandidates.filter((candidate) => candidate.provenance === FACT_PROVENANCE.LIVE_VERIFICATION);
  const admittedCandidates = admissibleCandidates.filter((candidate) => candidate.provenance !== FACT_PROVENANCE.LIVE_VERIFICATION);
  if (!admittedCandidates.length) return frontier("verification_without_canonical_evidence", verifications);

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

  const targets = new Set(finalists.map((candidate) => candidateTarget(candidate, requestedRelation)));
  if ([...targets].some((target) => typeof target !== "string" || target.length === 0)) {
    return frontier("resolution_target_missing", finalists, { bestVector });
  }
  if (targets.size > 1) {
    return frontier("authority_tie_conflict", finalists, {
      bestVector,
      targets: Object.freeze([...targets].map(String).sort()),
    });
  }

  const target = candidateTarget(best, requestedRelation);
  const conflicts = verifications.filter((candidate) => candidate.__authorityVector.coherence === 0
    && candidateTarget(candidate, requestedRelation) !== null
    && candidateTarget(candidate, requestedRelation) !== undefined
    && candidateTarget(candidate, requestedRelation) !== target);
  if (conflicts.length) return frontier("resolution_conflict", [best, ...conflicts], { bestVector });
  return Object.freeze({
    state: "admitted",
    reason: "categorical_precedence",
    admitted: normalizedCandidate(best),
    vector: bestVector,
    equivalentEvidence: Object.freeze(finalists.map((candidate) => normalizedCandidate(candidate))),
    verifications: Object.freeze(verifications.map((candidate) => normalizedCandidate(candidate, FACT_PROVENANCE.LIVE_VERIFICATION))),
  });
}
