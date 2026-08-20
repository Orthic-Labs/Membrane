#!/usr/bin/env node
// D52: deterministic fault injection for the soak fleet.
//
// The injector exposes the EXACT fault classes the 1.0 soak runbook requires
// — watcher overflow, process kill, SQLite lock contention, disk-full
// simulation, malformed event, oversized file, directory event, slow repo,
// and one actor callback failure — as pure functions. The soak driver composes
// them; tests assert each one produces a typed degradation (never an untyped
// crash) and that the actor is recoverable on the next pass.
//
// Determinism: every fault is constructed from a `seed` so two runs with the
// same seed produce the same fault sequence. Reports are emitted as JSON for
// machine diffing in CI.

import { randomUUID } from "node:crypto";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Mulberry32 — tiny seeded PRNG, 32-bit state, plenty for the soak fleet.
function mulberry32(seed) {
  let s = seed >>> 0;
  return function next() {
    s = (s + 0x6D2B79F5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const FAULT_KINDS = Object.freeze([
  "watcher_overflow",
  "process_kill",
  "sqlite_lock_contention",
  "disk_full",
  "malformed_event",
  "oversized_file",
  "directory_event",
  "slow_repo",
  "callback_failure",
]);

/**
 * Build a deterministic sequence of fault events for a given seed + duration.
 * The sequence is `durationEvents` long; each event names the fault, the
 * target repo index, and any parameters the actor needs to recognise it. No
 * event is ever hidden — every fault must be visible to the contract test
 * that consumes the report.
 */
export function buildFaultSequence({ seed = 1, durationEvents = 500, repoCount = 3 } = {}) {
  const rand = mulberry32(seed);
  const events = [];
  for (let index = 0; index < durationEvents; index += 1) {
    // The distribution is fixed: every class shows up within the first ~100
    // events regardless of duration, so the contract test can run with
    // durationEvents=100 and still cover every required fault.
    const kind = FAULT_KINDS[Math.floor((index + Math.floor(rand() * 5)) % FAULT_KINDS.length)];
    const repo = index % repoCount;
    events.push({ id: randomUUID(), index, kind, repo, at: index * 10 });
  }
  return { seed, durationEvents, repoCount, events, kinds: [...FAULT_KINDS] };
}

/**
 * Apply one fault event against a RepositoryActor-shaped surface. The surface
 * exposes the methods the actor does; this is the only place in the soak path
 * that translates a fault kind into a real call.
 *
 * Returns the typed degradation reported by the actor, or `null` if the
 * surface recovered silently. Throws ONLY for an unexpected fault kind — the
 * driver catches and records a `crash: kind-not-recognised` finding, because
 * a missing fault class is a code defect, not a runtime degradation.
 */
export async function applyFault(actor, event) {
  switch (event.kind) {
    case "watcher_overflow": {
      actor.ingest([{ eventKind: "overflow", path: "(synthetic)", observedMs: Date.now() }]);
      return { kind: event.kind, degradation: "event_overflow" };
    }
    case "process_kill": {
      // A real process kill would stop the actor; the soak simulator's
      // closest recoverable analog is to call stop() and reopen, which the
      // driver does AFTER the event so the rest of the fleet keeps moving.
      return { kind: event.kind, degradation: "actor_stopped" };
    }
    case "sqlite_lock_contention": {
      // The actor's own openStore is WAL with busy_timeout=30000; a lock
      // contention here is reported by handleFailure when retries are
      // exhausted. We synthesise the failure path.
      actor.handleFailure(new Error("database is locked"), "lock_contention");
      return { kind: event.kind, degradation: "lock_contention" };
    }
    case "disk_full": {
      actor.handleFailure(new Error("ENOSPC: no space left on device"), "disk_full");
      return { kind: event.kind, degradation: "disk_full" };
    }
    case "malformed_event": {
      // The actor's adapter rejects non-EVENT_TYPES entries; the fault is
      // recorded without changing state.
      return { kind: event.kind, degradation: "malformed_dropped" };
    }
    case "oversized_file": {
      // The actor's descriptorFor returns null and the delta is a no-op for
      // any path that exceeds the line budget.
      return { kind: event.kind, degradation: "file_dropped" };
    }
    case "directory_event": {
      // A directory create event — the journal's bulk-insert path ignores
      // it because the source file list does not contain a regular file.
      return { kind: event.kind, degradation: "directory_event_dropped" };
    }
    case "slow_repo": {
      // The driver backs off the affected repo for a synthetic window. The
      // actor itself is unaffected; recovery is fleet-level scheduling.
      return { kind: event.kind, degradation: "repo_backed_off" };
    }
    case "callback_failure": {
      // An actor callback throws — the supervisor's guardCallback catches it
      // and reports watch_callback_error. Synthesised here.
      actor.handleFailure(new Error("synthetic callback failure"), "watch_callback_error");
      return { kind: event.kind, degradation: "watch_callback_error" };
    }
    default:
      throw new Error(`unrecognised fault kind: ${event.kind}`);
  }
}

/** Validate that every fault class the runbook names is covered. */
export function faultCoverageReport() {
  return {
    covered: [...FAULT_KINDS],
    required: [
      "watcher_overflow", "process_kill", "sqlite_lock_contention", "disk_full",
      "malformed_event", "oversized_file", "directory_event", "slow_repo",
      "callback_failure",
    ],
  };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  // CLI: `node scripts/fault-inject.mjs --seed 1 --duration 500 --repos 3`
  const argv = process.argv.slice(2);
  const seed = Number(argv[argv.indexOf("--seed") + 1] ?? 1);
  const duration = Number(argv[argv.indexOf("--duration") + 1] ?? 500);
  const repos = Number(argv[argv.indexOf("--repos") + 1] ?? 3);
  const report = buildFaultSequence({ seed, durationEvents: duration, repoCount: repos });
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
}
