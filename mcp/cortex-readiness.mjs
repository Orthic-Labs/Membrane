// MBR-009 — Consume authoritative Cortex supervisor readiness.
//
// Membrane must NOT re-derive watcher liveness from a bare pid-alive probe the
// way a naive reader would. The Cortex watchman supervisor
// (cortex/watchman/supervisor.mjs) is the authoritative source of watcher
// readiness: it stamps every repo's `watch_state` with both `watcher_pid` AND
// `watcher_owner` (the supervisor's own per-object instanceId) when its actor
// claims a repo, and it clears both when the actor releases the repo. A status
// read therefore distinguishes four failure modes, not two:
//
//   - stale ACTOR  : no live watcher owns this repo right now. Either no pid is
//                    recorded (`watcher_never_started`), the recorded pid is
//                    dead (`watcher_process_dead`), or the recorded owner is a
//                    supervisor incarnation that has since been superseded
//                    (`watcher_owner_stale`). These are all "unwatched" from
//                    the consumer's standpoint — nothing is actively watching.
//   - stale DATA   : a live watcher DOES own the repo, but events are behind.
//                    `event_gap` non-zero → "degraded"; pending unapplied events
//                    → "stale".
//
// The defect this replaces: the catalog only ran `process.kill(pid, 0)`. After
// a supervisor restart the persisted pid is the predecessor's, which is dead,
// so the catalog reported "degraded" for repos the NEW supervisor was already
// (re)watching — a supervisor restart marked live repositories dead. Worse, the
// old code could not tell a recycled OS pid (now owned by an unrelated process)
// from a genuine live watcher, reporting "current" for a repo nobody watched.
//
// This module consumes the authoritative contract: it trusts the persisted
// owner stamp exactly as the supervisor itself does for an out-of-process
// reader (which has no `live.instanceId` of its own). A pid that is alive but
// carries a stale owner stamp is reported `watcher_owner_stale`, never
// "current" — and the freshness is `unwatched` (a stale actor), NOT
// `degraded` (stale data). That is the separation the acceptance demands.
//
// The module is self-contained and sibling-independent by construction (see
// MBR-015): it reads only the repo's own `.agent/graph/graph.db` `watch_state`
// table, the same store Cortex's supervisor reads. It never imports a sibling
// source tree.

import { existsSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

// The four freshness values the catalog surfaces. These match
// repository-catalog.mjs's WATCHER_STATES exactly and Cortex's FRESHNESS enum.
export const READINESS_STATES = Object.freeze(["current", "degraded", "stale", "unwatched"]);

const NO_GRAPH = Object.freeze({
  freshness: "unwatched",
  reason: "no_graph_built",
  alive: false,
  pid: null,
  owner: null,
  eventGap: 0,
  pendingEvents: 0,
  sourceClock: 0,
  appliedClock: 0,
});

/**
 * Is a pid alive right now? Returns false for a non-numeric or missing pid.
 * Uses signal 0 (existence probe); errors (ESRCH/EINVAL/EPERM) mean not alive.
 *
 * @param {number|null} pid
 * @returns {boolean}
 */
export function isPidAlive(pid) {
  const n = Number(pid ?? 0);
  if (!n || n <= 0) return false;
  try { process.kill(n, 0); return true; } catch { return false; }
}
/**
 * Read the persisted `watch_state` rows for a repo without ever mutating them.
 * Returns a plain object keyed by the state key, or `null` when the graph is
 * unbuilt or unreadable. Reads are isolated: one corrupt store never blanks the
 * caller's view of another repo (each call opens its own read-only handle and
 * swallows per-repo errors).
 */
function readWatchState(absoluteRoot) {
  const graphDbPath = join(absoluteRoot, ".agent", "graph", "graph.db");
  if (!existsSync(graphDbPath)) return null;
  let db;
  try {
    db = new DatabaseSync(graphDbPath, { open: true, readOnly: true });
    const rows = db.prepare("SELECT key, value FROM watch_state").all();
    if (!rows || rows.length === 0) return {};
    return Object.fromEntries(rows.map((row) => [row.key, row.value]));
  } catch {
    // graph.db unreadable or missing watch_state table → treat as no graph.
    return null;
  } finally {
    if (db) { try { db.close(); } catch {} }
  }
}

function countPendingEvents(absoluteRoot) {
  const graphDbPath = join(absoluteRoot, ".agent", "graph", "graph.db");
  if (!existsSync(graphDbPath)) return 0;
  let db;
  try {
    db = new DatabaseSync(graphDbPath, { open: true, readOnly: true });
    const row = db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=0").get();
    return Number(row?.n ?? 0);
  } catch {
    return 0;
  } finally {
    if (db) { try { db.close(); } catch {} }
  }
}
/**
 * Consume Cortex's authoritative supervisor readiness for one repo.
 *
 * Decision order (mirrors cortex/watchman/supervisor.mjs repoStatus for an
 * out-of-process reader, which has no `live.instanceId` of its own):
 *
 *   1. No graph.db / unreadable store        → unwatched, `no_graph_built`
 *   2. No persisted pid                      → unwatched, `watcher_never_started`
 *   3. pid present but dead                  → unwatched, `watcher_process_dead`
 *   4. pid alive but owner stamp is foreign  → unwatched, `watcher_owner_stale`
 *        (a supervisor restart leaves a stale predecessor stamp; this is a
 *         stale ACTOR, never "current". The caller may supply its own current
 *         supervisor instanceId via options.ownInstanceId to detect this; if
 *         the caller has no instance of its own it trusts the bare pid, exactly
 *         as the supervisor does for a bare status reader.)
 *   5. pid alive and owned (or trusted)      → actor is alive; then decide on
 *        data staleness: event_gap → degraded; pending events → stale; else
 *        current.
 *
 * @param {string} absoluteRoot absolute path to the repo root
 * @param {{ ownInstanceId?: string|null }} [options] the calling supervisor's
 *   own instanceId, if any. A bare catalog reader omits it and defers entirely
 *   to the persisted pid — matching Cortex's own bare-reader behavior.
 * @returns {{ freshness: "current"|"degraded"|"stale"|"unwatched", reason: string, alive: boolean, pid: number|null, owner: string|null, eventGap: number, pendingEvents: number, sourceClock: number, appliedClock: number }}
 */
export function readCortexReadiness(absoluteRoot, options = {}) {
  const state = readWatchState(absoluteRoot);
  if (!state) return { ...NO_GRAPH };

  const pid = Number(state.watcher_pid ?? 0) || null;
  const owner = state.watcher_owner ?? null;
  const eventGap = Number(state.event_gap ?? 0);
  const pendingEvents = countPendingEvents(absoluteRoot);
  const sourceClock = Number(state.source_clock ?? 0);
  const appliedClock = Number(state.applied_clock ?? 0);
  const lastError = state.last_error ?? null;
  const gapReason = state.event_gap_reason ?? null;

  // No pid recorded → the supervisor never claimed this repo.
  if (!pid) {
    return {
      freshness: "unwatched", reason: "watcher_never_started",
      alive: false, pid: null, owner, eventGap, pendingEvents, sourceClock, appliedClock,
    };
  }

  const pidAlive = isPidAlive(pid);
  // Stale ACTOR: the recorded watcher process is gone. This is the case that a
  // supervisor restart exposes — the predecessor's dead pid is still stamped.
  // We MUST NOT report "degraded" here: degraded means a live watcher is behind
  // on data, not that no watcher is alive at all.
  if (!pidAlive) {
    return {
      freshness: "unwatched", reason: "watcher_process_dead",
      alive: false, pid, owner, eventGap, pendingEvents, sourceClock, appliedClock,
    };
  }

  // The pid is alive. If the caller is itself a supervisor instance and the
  // persisted owner names a DIFFERENT instance, the stamp is from a superseded
  // predecessor — distrust it. (A bare reader with no ownInstanceId trusts the
  // bare pid, exactly as Cortex's supervisor does for a bare status reader.)
  const ownInstanceId = options.ownInstanceId ?? null;
  if (ownInstanceId && owner && owner !== ownInstanceId) {
    return {
      freshness: "unwatched", reason: "watcher_owner_stale",
      alive: false, pid, owner, eventGap, pendingEvents, sourceClock, appliedClock,
    };
  }

  // The actor is alive and owned. Now decide on DATA staleness only.
  if (eventGap) {
    return {
      freshness: "degraded",
      reason: gapReason ?? (lastError ? `event_gap: ${lastError}` : "event_gap_unreconciled"),
      alive: true, pid, owner, eventGap, pendingEvents, sourceClock, appliedClock,
    };
  }
  if (pendingEvents > 0) {
    return {
      freshness: "stale", reason: "events_pending_apply",
      alive: true, pid, owner, eventGap, pendingEvents, sourceClock, appliedClock,
    };
  }
  return {
    freshness: "current", reason: null,
    alive: true, pid, owner, eventGap, pendingEvents, sourceClock, appliedClock,
  };
}
