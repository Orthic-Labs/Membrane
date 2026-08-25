// Query-time freshness receipt (BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md
// §17.2.2). "Indexed 12 minutes ago" is not freshness: a receipt must
// distinguish what was indexed (`generation`) from what is on disk right now
// (`current`), observed directly, and report the relationship between them
// as one of `fresh | changed_since_generation | unknown | unavailable`.
//
// TWO SEPARATE AXES — do not collapse them:
//
//   freshness            Honest staleness. `changed_since_generation` is a
//                         TRUTHFUL SUCCESSFUL result: the index is internally
//                         coherent, the worktree has simply moved on since it
//                         was built. It is not a failure and not an error.
//
//   generation coherence Whether a consumer that pinned a specific
//                         generationId is actually being served that exact
//                         generation. A mismatch is incoherence, and it FAILS
//                         CLOSED unconditionally — including when freshness
//                         reports `fresh`. A pinned consumer must hard-fail on
//                         mismatch even against a fresh index; a consumer that
//                         receives `changed_since_generation` must not treat
//                         that, by itself, as a mismatch.
//
// `evaluateFreshness` and `assertGenerationCoherence` are independent
// functions for exactly this reason: an implementation that folds generation
// pinning into the freshness enum (or vice versa) is the conflation this
// module exists to prevent.

import { gitSourceObservation } from "./git-source-observation.mjs";
import { readManifestEnvelope } from "./store-sqlite.mjs";

export const FRESHNESS_RECEIPT_SCHEMA = "BlueprintFreshnessReceiptV1";

export const FRESHNESS_STATES = Object.freeze([
  "fresh",
  "changed_since_generation",
  "unknown",
  "unavailable",
]);

function typedError(code, message, details) {
  const error = new Error(message);
  error.code = code;
  if (details !== undefined) error.details = details;
  return error;
}

/**
 * What the sealed generation claims it was indexed at. Reads straight off the
 * envelope's `sourceObservation` (recorded by the SAME git observer this
 * module uses for `current` — see git-source-observation.mjs) so the two
 * sides are always comparable, never independently derived.
 */
export function generationFreshnessBasis(envelope) {
  const sourceObservation = envelope?.sourceObservation ?? null;
  return Object.freeze({
    indexed_revision: sourceObservation?.head ?? null,
    indexed_worktree_fingerprint: sourceObservation?.statusDigest ?? null,
  });
}

/**
 * What is on disk right now, observed directly — never inferred from how
 * long ago the generation was built. Returns `available: false` when git
 * itself is unavailable (no repo, git missing, timeout), which is the ONLY
 * path to freshness `unavailable`.
 */
export function observeCurrentVcsState(repoRoot, { observe = gitSourceObservation } = {}) {
  const observed = observe(repoRoot);
  if (!observed) {
    return Object.freeze({
      available: false,
      vcs_revision: null,
      dirty: null,
      worktree_fingerprint: null,
    });
  }
  return Object.freeze({
    available: true,
    vcs_revision: observed.head,
    dirty: observed.dirty,
    worktree_fingerprint: observed.statusDigest,
  });
}

/**
 * The freshness axis ONLY. Never throws, never considers a pinned generation
 * — see assertGenerationCoherence for that orthogonal check.
 */
export function evaluateFreshness(generation, current) {
  if (!current?.available) return "unavailable";
  if (!generation?.indexed_revision || !generation?.indexed_worktree_fingerprint) return "unknown";
  if (current.vcs_revision === generation.indexed_revision
    && current.worktree_fingerprint === generation.indexed_worktree_fingerprint) {
    return "fresh";
  }
  // Truthful staleness, not a failure: the index is coherent, the worktree
  // has moved on since it was captured.
  return "changed_since_generation";
}

/**
 * Build the full BlueprintFreshnessReceiptV1 for `db`'s sealed generation
 * against `repoRoot`'s current worktree state.
 */
export function buildFreshnessReceipt(db, repoRoot, { observe = gitSourceObservation } = {}) {
  const envelope = readManifestEnvelope(db);
  const generation = generationFreshnessBasis(envelope);
  const current = observeCurrentVcsState(repoRoot, { observe });
  const freshness = evaluateFreshness(generation, current);
  return Object.freeze({
    schema: FRESHNESS_RECEIPT_SCHEMA,
    generationId: envelope?.generationId ?? null,
    manifestDigest: envelope?.manifestDigest ?? null,
    generation,
    current: Object.freeze({
      vcs_revision: current.vcs_revision,
      dirty: current.dirty,
      worktree_fingerprint: current.worktree_fingerprint,
    }),
    freshness,
  });
}

/**
 * The generation-coherence axis ONLY — orthogonal to freshness, and it fails
 * closed regardless of what freshness reports. Call this whenever a caller
 * pinned a generationId; a mismatch means the caller is not looking at the
 * generation it thinks it is, which is unsafe at every freshness state,
 * including `fresh` (a fresh-but-different generation is still not the one
 * that was pinned).
 */
export function assertGenerationCoherence({ pinnedGenerationId, servedGenerationId }) {
  if (!pinnedGenerationId) return;
  if (pinnedGenerationId === servedGenerationId) return;
  throw typedError(
    "generation_mismatch",
    `Pinned generation ${pinnedGenerationId} does not match the served generation ${servedGenerationId}.`,
    { pinnedGenerationId, servedGenerationId },
  );
}
