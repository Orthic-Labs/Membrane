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

import { execFileSync } from "node:child_process";
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

function gitLines(repoRoot, args) {
  try {
    return execFileSync("git", args, {
      cwd: repoRoot,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      timeout: 5_000,
      maxBuffer: 4 * 1024 * 1024,
    }).split(/\r?\n/).map((line) => line.trim().replaceAll("\\", "/")).filter(Boolean);
  } catch {
    return null;
  }
}

/**
 * Enumerate source paths whose sealed-generation evidence may be stale. This
 * is response-boundary suppression evidence, not a second freshness verdict.
 * A failed enumeration deliberately returns `complete:false`, requiring the
 * caller to suppress every source-backed row instead of guessing it is fresh.
 */
export function changedPathsSinceGeneration(repoRoot, generation, current) {
  if (!current?.available || !generation?.indexed_revision) {
    return Object.freeze({ complete: false, paths: Object.freeze([]), reason: "comparison_unavailable" });
  }
  const committed = gitLines(repoRoot, ["diff", "--no-renames", "--name-only", "--diff-filter=ACDMRTUXB", generation.indexed_revision, "--"]);
  const worktree = gitLines(repoRoot, ["diff", "--no-renames", "--name-only", "--diff-filter=ACDMRTUXB", "HEAD", "--"]);
  const untracked = gitLines(repoRoot, ["ls-files", "--others", "--exclude-standard"]);
  if (!committed || !worktree || !untracked) {
    return Object.freeze({ complete: false, paths: Object.freeze([]), reason: "comparison_failed" });
  }
  return Object.freeze({
    complete: true,
    paths: Object.freeze([...new Set([...committed, ...worktree, ...untracked])].sort()),
    reason: null,
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
export function buildFreshnessReceipt(db, repoRoot, { observe = gitSourceObservation, enumerateChanged = changedPathsSinceGeneration } = {}) {
  const envelope = readManifestEnvelope(db);
  const generation = generationFreshnessBasis(envelope);
  const current = observeCurrentVcsState(repoRoot, { observe });
  const freshness = evaluateFreshness(generation, current);
  const changedRaw = freshness === "changed_since_generation"
    ? enumerateChanged(repoRoot, generation, current)
    : Object.freeze({ complete: freshness === "fresh", paths: Object.freeze([]), reason: freshness === "fresh" ? null : freshness });
  const indexedPaths = new Set(db.prepare("SELECT path FROM generation_leaf WHERE kind='file'").all().map((row) => row.path));
  const changed = Object.freeze({ ...changedRaw, paths: Object.freeze(changedRaw.paths.filter((path) => indexedPaths.has(path))) });
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
    staleSources: changed,
    suppression: Object.freeze({
      required: freshness === "changed_since_generation",
      mode: freshness !== "changed_since_generation" ? "none" : changed.complete ? "changed_paths" : "whole_generation",
    }),
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
