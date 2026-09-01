// G5 Lane C — Source adapter fan-out.
//
// Exposes `produceCandidates(task, scope) => ContextCandidate[]` and the
// richer `produceCandidatesWithTelemetry(task, scope) => { candidates, omissions, adapters }`
// for the integration owner and the planner service.
//
// Adapters run in a fixed, deterministic order:
//   1. task-anchor        — explicit user-named paths/symbols/ranges
//   2. dirty-files        — `git status` + per-file diffs vs HEAD
//   3. graph-resolve      — exact path/symbol resolution via Blueprint
//   4. git-metadata       — branch/commit/upstream/worktree summary
//   5. rules-documents    — AGENTS.md + .claude/rules + .codex/rules
//   6. live-overlay       — mtime/hash drift against the graph generation
//
// Each adapter is independently fault-tolerant: a thrown error is captured
// as `adapters[i].error` and the rest still run. The dispatch says
// "adapters produce candidates; they DO NOT perform final ranking" —
// this module is the last-mile fan-out and a tiny dedupe pass, nothing
// more. Ranking, budgeting, contradiction resolution, and receipt
// emission all live in the planner.

import * as taskAnchor from "./task-anchor.mjs";
import * as dirtyFiles from "./dirty-files.mjs";
import * as graphResolve from "./graph-resolve.mjs";
import * as gitMetadata from "./git-metadata.mjs";
import * as rulesDocuments from "./rules-documents.mjs";
import * as liveOverlay from "./live-overlay.mjs";

const ADAPTERS = [
  { id: "task-anchor", module: taskAnchor, layer: 1 },
  { id: "dirty-files", module: dirtyFiles, layer: 1 },
  { id: "graph-resolve", module: graphResolve, layer: 3 },
  { id: "git-metadata", module: gitMetadata, layer: 1 },
  { id: "rules-documents", module: rulesDocuments, layer: 2 },
  { id: "live-overlay", module: liveOverlay, layer: 1 },
];

const PROVIDER_TAG = "membrane-sources";

function runAdapter(entry, task, scope) {
  try {
    const result = entry.module.produce(task, scope) ?? { candidates: [], omissions: [] };
    return {
      id: entry.id,
      layer: entry.layer,
      ok: true,
      candidateCount: Array.isArray(result.candidates) ? result.candidates.length : 0,
      omissionCount: Array.isArray(result.omissions) ? result.omissions.length : 0,
      extras: Object.fromEntries(
        Object.entries(result).filter(([key]) => !["candidates", "omissions"].includes(key)),
      ),
      candidates: Array.isArray(result.candidates) ? result.candidates : [],
      omissions: Array.isArray(result.omissions) ? result.omissions : [],
    };
  } catch (error) {
    return {
      id: entry.id,
      layer: entry.layer,
      ok: false,
      error: error?.message ?? String(error),
      candidates: [],
      omissions: [{ id: `${PROVIDER_TAG}:${entry.id}:fatal`, layer: entry.layer, reason: "adapter_threw" }],
    };
  }
}

function stableKey(candidate) {
  return [
    candidate.sourceKind,
    candidate.sourceRef,
    candidate.sourceHash,
    candidate.id,
  ].join("|");
}

/**
 * Produce typed ContextCandidate v1 records from every Lane C adapter.
 *
 * @param {string} task - the planner-supplied task string
 * @param {{
 *   repoRoot: string,
 *   generation?: object,
 *   generationMeta?: { indexedAt?: string|null, recordedHashes?: Map<string,string>, portable?: boolean },
 *   freshGeneration?: () => object,
 *   ruleByteCap?: number,
 *   diffByteCap?: number,
 *   bodyByteCap?: number,
 *   maxSymbolHits?: number,
 *   maxRules?: number,
 *   maxOverlay?: number,
 *   rulesInclude?: string[],
 *   rulesExclude?: string[],
 * }} scope
 */
export function produceCandidatesWithTelemetry(task, scope) {
  const safeScope = scope ?? {};
  const telemetry = [];
  const collected = [];
  const omissions = [];
  for (const entry of ADAPTERS) {
    const result = runAdapter(entry, task, safeScope);
    telemetry.push(result);
    for (const candidate of result.candidates) collected.push(candidate);
    for (const omission of result.omissions) omissions.push(omission);
  }
  // Dedupe by (sourceKind, sourceRef, sourceHash, id). The first occurrence
  // wins; the integration owner may relax this with `scope.dedupeStrategy`.
  const seen = new Set();
  const deduped = [];
  for (const candidate of collected) {
    const key = stableKey(candidate);
    if (seen.has(key)) continue;
    seen.add(key);
    deduped.push(candidate);
  }
  return {
    candidates: deduped,
    omissions,
    adapters: telemetry,
    provider: PROVIDER_TAG,
  };
}

/**
 * Convenience: just the candidates, in adapter order, deduped.
 *
 * @param {string} task
 * @param {object} scope
 * @returns {object[]} ContextCandidate v1 records
 */
export function produceCandidates(task, scope) {
  return produceCandidatesWithTelemetry(task, scope).candidates;
}

export const adapterInfo = ADAPTERS.map((entry) => ({
  id: entry.id,
  layer: entry.layer,
  module: entry.module.adapterInfo ?? null,
}));

export { taskAnchor, dirtyFiles, graphResolve, gitMetadata, rulesDocuments, liveOverlay };
export const PROVIDER = PROVIDER_TAG;