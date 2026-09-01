// BlueprintStoreLeaseV1 — store write-ownership contract (BLUEPRINT_CANONICAL_
// SOURCE_OF_TRUTH.md §17.2.1). Only Blueprint code may open the Blueprint
// store; this module is the ONE place that arbitrates who may hold the
// writer role on a given graph.db path.
//
// Exclusion mechanism (in priority order):
//   1. PRIMARY — an OS-level lock. Acquiring a lease opens a dedicated sidecar
//      SQLite file (`<dbPath>.lease.lock`) and holds a live `BEGIN EXCLUSIVE`
//      transaction on it for the lifetime of the lease. SQLite's EXCLUSIVE
//      lock is implemented with real OS advisory file locks (fcntl on
//      POSIX, LockFileEx region locks on Windows), which the OS releases the
//      instant the holding process's file descriptor closes — including an
//      ungraceful crash or SIGKILL, with zero cooperation required from the
//      dead process. This is what lets a crashed owner's lock disappear
//      without any recovery ritual: a contending acquire simply succeeds.
//   2. DIAGNOSIS ONLY — a JSON sidecar (`<dbPath>.lease.json`) carrying the
//      BlueprintStoreLeaseV1 fields below. It is written best-effort after the
//      OS lock is won and is NEVER consulted to decide whether the store is
//      actually held — only a successful `BEGIN EXCLUSIVE` proves that. After
//      a crash this file is stale by construction (it still names the dead
//      owner) and MUST be treated as informational only; it is repaired
//      in place the next time anyone successfully acquires the lease.
//
// BlueprintStoreLeaseV1 shape:
//   owner_kind             "hub" | "one_shot"
//   owner_instance_id      caller-supplied identity for this owner instance
//   pid                    process.pid of the acquiring process
//   process_start_identity disambiguates PID reuse (see below)
//   acquired_at            ISO-8601 timestamp of first acquisition
//   heartbeat_at           ISO-8601 timestamp of the most recent heartbeat
//   lease_epoch            monotonically increasing across acquisitions of
//                          this dbPath (best-effort; diagnostic only)
//
// PID alone is insufficient because PIDs are reused by the OS across process
// lifetimes; `process_start_identity` disambiguates by recording (an
// approximation of) when the owning process itself started, so a stale
// metadata row naming a since-recycled pid cannot be mistaken for the
// process that currently holds that pid.
//
// A one-shot writer that contends with an active owner NEVER becomes a
// second writer: it fails typed `resident_owner_active`. Routing a one-shot
// request through an active Hub owner instead of failing is a Hub-hosting
// concern and is intentionally not implemented here (see agent scope notes).

import { DatabaseSync } from "node:sqlite";
import { randomUUID } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from "node:fs";
import { dirname } from "node:path";

export const STORE_LEASE_SCHEMA = "BlueprintStoreLeaseV1";
export const STORE_LEASE_OWNER_KINDS = Object.freeze(["hub", "one_shot"]);

function lockPathFor(dbPath) {
  return `${dbPath}.lease.lock`;
}

function metadataPathFor(dbPath) {
  return `${dbPath}.lease.json`;
}

function typedError(code, message, details) {
  const error = new Error(message);
  error.code = code;
  if (details !== undefined) error.details = details;
  return error;
}

// Approximate epoch-ms start time of THIS process, derived from
// `process.uptime()`. Node exposes no direct process-start timestamp, but
// this is stable for the lifetime of one process instance and — unlike the
// bare pid — is not reused when the OS recycles the pid for an unrelated
// later process, which is all the diagnostic field needs to guarantee.
export function currentProcessStartIdentity() {
  return Math.round(Date.now() - process.uptime() * 1000);
}

function isSqliteBusyError(error) {
  if (error?.code === "ERR_SQLITE_ERROR") return true;
  return /database is locked|SQLITE_BUSY/i.test(String(error?.message ?? ""));
}

/** Best-effort read of the diagnostic metadata sidecar. Never throws. */
export function readStoreLeaseMetadata(dbPath) {
  const path = metadataPathFor(dbPath);
  if (!existsSync(path)) return null;
  try {
    const parsed = JSON.parse(readFileSync(path, "utf8"));
    if (parsed?.schema !== STORE_LEASE_SCHEMA) return null;
    return parsed;
  } catch {
    // A torn/corrupt metadata write is diagnosis-only fallout, never a
    // reason to block acquisition — the OS lock remains the source of truth.
    return null;
  }
}

function writeStoreLeaseMetadata(dbPath, lease) {
  const path = metadataPathFor(dbPath);
  const tmp = `${path}.tmp-${process.pid}-${randomUUID()}`;
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(tmp, `${JSON.stringify(lease, null, 2)}\n`);
  renameSync(tmp, path);
}

/**
 * Non-destructive probe: does anyone currently hold the OS-level write lock
 * for `dbPath`? Implemented by attempting the exact same EXCLUSIVE acquire
 * used by `acquireStoreLease` and immediately rolling back on success — so
 * the answer is always ground truth, never inferred from metadata.
 */
export function isStoreLeaseHeld(dbPath) {
  if (!dbPath || dbPath === ":memory:") return false;
  const lockPath = lockPathFor(dbPath);
  if (!existsSync(lockPath)) return false;
  let db;
  try {
    db = new DatabaseSync(lockPath);
    db.exec("PRAGMA busy_timeout = 0;");
    db.exec("BEGIN EXCLUSIVE;");
    db.exec("ROLLBACK;");
    return false;
  } catch (error) {
    if (isSqliteBusyError(error)) return true;
    throw error;
  } finally {
    try { db?.close(); } catch { /* handle already unusable */ }
  }
}

/**
 * Acquire the Blueprint store write lease for `dbPath`.
 *
 * Throws typed `resident_owner_active` if the OS-level lock is already held
 * by a live owner — this function NEVER returns a second concurrent writer.
 * Returns a lease handle with `.lease` (the current BlueprintStoreLeaseV1
 * snapshot), `.heartbeat()`, and `.release()`.
 */
export function acquireStoreLease(dbPath, { ownerKind = "one_shot", ownerInstanceId = randomUUID() } = {}) {
  if (!dbPath || dbPath === ":memory:") {
    throw typedError("lease_requires_persistent_store", "A store write lease requires a persistent store path; \":memory:\" has no filesystem identity to lease.");
  }
  if (!STORE_LEASE_OWNER_KINDS.includes(ownerKind)) {
    throw typedError("invalid_owner_kind", `owner_kind must be one of ${STORE_LEASE_OWNER_KINDS.join(", ")}; got ${JSON.stringify(ownerKind)}.`);
  }

  const lockPath = lockPathFor(dbPath);
  mkdirSync(dirname(lockPath), { recursive: true });

  let lockDb;
  try {
    lockDb = new DatabaseSync(lockPath);
    // busy_timeout=0: acquisition is a try-immediately probe, never a wait.
    // A caller that wants to wait for an owner to release does so by
    // retrying acquireStoreLease itself (with its own backoff), not by
    // blocking inside this call — the resident_owner_active failure must
    // surface promptly so a one-shot caller can route or report it.
    lockDb.exec("PRAGMA busy_timeout = 0;");
    lockDb.exec("BEGIN EXCLUSIVE;");
  } catch (error) {
    try { lockDb?.close(); } catch { /* handle already unusable */ }
    if (isSqliteBusyError(error)) {
      throw typedError("resident_owner_active", `Blueprint store write lease for ${dbPath} is already held by an active owner.`, {
        dbPath,
        // Diagnosis only — may be stale, absent, or describe a since-crashed
        // owner. Never used to decide the outcome above; the OS lock already
        // decided it.
        lastKnownMetadata: readStoreLeaseMetadata(dbPath),
      });
    }
    throw error;
  }

  const previousEpoch = Number(readStoreLeaseMetadata(dbPath)?.lease_epoch ?? 0);
  const acquiredAt = new Date().toISOString();
  let current = Object.freeze({
    schema: STORE_LEASE_SCHEMA,
    owner_kind: ownerKind,
    owner_instance_id: ownerInstanceId,
    pid: process.pid,
    process_start_identity: currentProcessStartIdentity(),
    acquired_at: acquiredAt,
    heartbeat_at: acquiredAt,
    lease_epoch: previousEpoch + 1,
  });
  writeStoreLeaseMetadata(dbPath, current);

  let released = false;
  const assertLive = () => {
    if (released) throw typedError("lease_already_released", `Blueprint store write lease for ${dbPath} was already released.`);
  };

  return Object.freeze({
    get lease() { return current; },
    /** Refresh heartbeat_at in the diagnostic metadata sidecar. */
    heartbeat() {
      assertLive();
      current = Object.freeze({ ...current, heartbeat_at: new Date().toISOString() });
      writeStoreLeaseMetadata(dbPath, current);
      return current;
    },
    /** Release the OS lock. Idempotent. */
    release() {
      if (released) return;
      released = true;
      try { lockDb.exec("ROLLBACK;"); } catch { /* nothing to roll back if already gone */ }
      try { lockDb.close(); } catch { /* already unusable */ }
      // The metadata sidecar is deliberately left in place, describing the
      // lease that just ended, rather than deleted: `lease_epoch` stays
      // monotonic across clean releases (not just crashes), and diagnosis
      // never leans on "file present/absent" to infer whether the store is
      // currently held — isStoreLeaseHeld()'s real OS-lock probe is the only
      // authority for that, both here and after a crash.
    },
  });
}

/** Acquire a lease, run `fn(lease)`, and always release — for one-shot callers. */
export async function withStoreLease(dbPath, options, fn) {
  const handle = acquireStoreLease(dbPath, options);
  try {
    return await fn(handle);
  } finally {
    handle.release();
  }
}

/** Synchronous counterpart for publication paths that cannot yield mid-write. */
export function withStoreLeaseSync(dbPath, options, fn) {
  const handle = acquireStoreLease(dbPath, options);
  try {
    return fn(handle);
  } finally {
    handle.release();
  }
}
