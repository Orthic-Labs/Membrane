import { createHash } from "node:crypto";

function sha(value) {
  return `sha256:${createHash("sha256").update(String(value ?? "")).digest("hex")}`;
}

export function normalizedAnchorText(value) {
  return String(value ?? "").replace(/\s+/g, " ").trim();
}

export function anchorFingerprint(value) {
  return sha(String(value ?? ""));
}

function candidateId(candidate, index) {
  return candidate?.id ?? candidate?.occurrenceId ?? candidate?.claimId ?? `candidate:${index}`;
}

function uniqueMatches(candidates, predicate) {
  return candidates.map((candidate, index) => ({ candidate, id: candidateId(candidate, index) })).filter(({ candidate }) => predicate(candidate));
}

function outcome(tier, matches, extra = {}) {
  if (matches.length === 1) return { state: "reanchored", tier, targetId: matches[0].id, target: matches[0].candidate, candidates: [matches[0].id], ...extra };
  if (matches.length > 1) return { state: "ambiguous", tier, targetId: null, target: null, candidates: matches.map((row) => row.id).sort(), ...extra };
  return null;
}

/**
 * Conservative relocation/re-anchoring. No fuzzy score is allowed to choose a
 * winner. The admissible chain is exact semantic entity -> exact fingerprint ->
 * unique normalized text. If no tier yields one winner the anchor is stale or
 * ambiguous, never silently moved.
 */
export function reanchorEvidence(previous, currentCandidates = []) {
  const candidates = Array.isArray(currentCandidates) ? currentCandidates : [];
  const portableId = previous?.portableId ?? previous?.entityPortableId ?? null;
  if (portableId) {
    const match = outcome("exact_entity", uniqueMatches(candidates, (candidate) => (candidate.portableId ?? candidate.entityPortableId) === portableId));
    if (match) return match;
  }

  const fingerprint = previous?.fingerprint ?? previous?.contentFingerprint ?? (previous?.text !== undefined ? anchorFingerprint(previous.text) : null);
  if (fingerprint) {
    const match = outcome("exact_fingerprint", uniqueMatches(candidates, (candidate) => {
      const candidateFingerprint = candidate.fingerprint ?? candidate.contentFingerprint ?? (candidate.text !== undefined ? anchorFingerprint(candidate.text) : null);
      return candidateFingerprint === fingerprint;
    }));
    if (match) return match;
  }

  const normalized = normalizedAnchorText(previous?.text);
  if (normalized) {
    const match = outcome("unique_normalized_text", uniqueMatches(candidates, (candidate) => normalizedAnchorText(candidate.text) === normalized));
    if (match) return match;
  }

  return {
    state: "stale",
    tier: "none",
    targetId: null,
    target: null,
    candidates: [],
    reason: "no_exact_reanchor",
  };
}

export function reconcileRenameAliases({ before = [], after = [], oldPath, newPath } = {}) {
  const prior = (before ?? []).filter((fact) => fact?.path === oldPath || fact?.evidence?.some((item) => item?.path === oldPath));
  const current = (after ?? []).filter((fact) => fact?.path === newPath || fact?.evidence?.some((item) => item?.path === newPath));
  const aliases = [];
  const unresolved = [];
  for (const fact of prior) {
    const result = reanchorEvidence(fact, current);
    if (result.state === "reanchored") aliases.push({ from: fact.id, to: result.targetId, tier: result.tier, oldPath, newPath });
    else unresolved.push({ from: fact.id, state: result.state, tier: result.tier, candidates: result.candidates, oldPath, newPath });
  }
  return { schemaVersion: 1, oldPath, newPath, aliases, unresolved };
}
