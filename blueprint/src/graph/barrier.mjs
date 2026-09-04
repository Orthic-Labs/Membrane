import { randomUUID } from "node:crypto";
import { existsSync, realpathSync } from "node:fs";
import { join, resolve } from "node:path";
import { reconcile } from "../../watchman/reconcile.mjs";
import { closeStore, getGenerationEnvelope, insertGenerationReceipt, openStore, openStoreReadOnly } from "./store-sqlite.mjs";
import { recordBarrierDuration } from "../lib/telemetry.mjs";
import { withStoreLease } from "./store-lease.mjs";
import { gitSourceObservation } from "./git-source-observation.mjs";
import { computeManifestDigest } from "./generation-identity.mjs";

const POLL_MS = 25;
function canonicalRoot(value) { const root = resolve(value); try { return realpathSync(root); } catch { return root; } }

function state(db) {
  const rows = db.prepare("SELECT key,value FROM watch_state").all();
  return Object.fromEntries(rows.map((row) => [row.key, row.value]));
}

function pendingDomains(values) {
  return String(values.domains_pending ?? "").split(",").map((item) => item.trim()).filter(Boolean).sort();
}

function watcherAlive(pid) {
  const value = Number(pid ?? 0);
  if (!value) return false;
  try { process.kill(value, 0); return true; } catch { return false; }
}

function sleep(ms) { return new Promise((resolvePromise) => setTimeout(resolvePromise, ms)); }
function cancelled() { return Object.assign(new Error("request cancelled"), { code: "request_cancelled" }); }
function throwIfAborted(signal) { if (signal?.aborted) throw cancelled(); }

async function bounded(promise, timeoutMs, signal, onTimeout) {
  let timer;
  let abort;
  const work = Promise.resolve(promise);
  try {
    throwIfAborted(signal);
    return await Promise.race([
      work,
      new Promise((_, reject) => { timer = setTimeout(() => reject(Object.assign(new Error("barrier timeout"), { code: "barrier_timeout" })), timeoutMs); }),
      new Promise((_, reject) => { abort = () => reject(cancelled()); signal?.addEventListener("abort", abort, { once: true }); }),
    ]);
  } catch (error) {
    if (["barrier_timeout", "request_cancelled"].includes(error?.code)) {
      // The drain below exists for DB-handle safety: the abandoned work still
      // uses the caller's connection, and returning before it settles would let
      // the caller's closeStore() yank the handle mid-write. But draining
      // WITHOUT aborting made the timeout decorative — a reconcile triggered by
      // any HEAD movement ran minutes of reindex while the "timed out" caller
      // blocked on this await. That single line stalled every barrier command
      // (candidates, search, impact) behind the watcher on busy workspaces.
      // Abort first — reconcile checks its signal throughout — so the drain
      // settles in milliseconds, then surface the timeout.
      onTimeout?.();
      await work.catch(() => {});
    }
    throw error;
  } finally { clearTimeout(timer); signal?.removeEventListener("abort", abort); }
}

/**
 * Bring structural graph state to the current source tree, then issue a
 * durable receipt. `db` must be a writable graph store connection.
 */
export async function syncToCurrentSource(db, root, { timeoutMs = 2000, allowDegraded = false, outDir = ".agent", signal, reconcileFn = reconcile } = {}) {
  const startedMs = Date.now();
  const repoRoot = canonicalRoot(root);
  const initial = state(db);
  const targetClock = Number(initial.source_clock ?? 0);
  let barrierResult = "caught_up";
  let error = null;
  const runReconcile = async () => {
    // Internal controller chained to the caller's signal: on timeout, bounded()
    // aborts the reconcile through it so the safety drain returns promptly
    // instead of waiting out a full reindex.
    const controller = new AbortController();
    const forward = () => controller.abort();
    signal?.addEventListener("abort", forward, { once: true });
    try {
      return await bounded(
        reconcileFn(db, repoRoot, { outDir, signal: controller.signal }),
        Math.max(1, timeoutMs - (Date.now() - startedMs)),
        signal,
        () => controller.abort(),
      );
    } finally { signal?.removeEventListener("abort", forward); }
  };

  try {
    if (initial.event_gap === "1") {
      await runReconcile();
      if (state(db).event_gap === "1") barrierResult = "gap_blocked";
    }
    if (barrierResult === "caught_up" && !watcherAlive(state(db).watcher_pid)) {
      await runReconcile();
    }
    while (barrierResult === "caught_up") {
      const current = state(db);
      if (Number(current.applied_clock ?? 0) >= targetClock && current.event_gap !== "1") break;
      if (Date.now() - startedMs >= timeoutMs) { barrierResult = "timeout"; break; }
      await bounded(sleep(Math.min(POLL_MS, Math.max(1, timeoutMs - (Date.now() - startedMs)))), Math.max(1, timeoutMs - (Date.now() - startedMs)), signal);
    }
  } catch (caught) {
    error = caught;
    if (caught?.code === "request_cancelled") throw caught;
    if (caught?.code === "barrier_timeout") barrierResult = "timeout";
    else if (state(db).event_gap === "1") barrierResult = "gap_blocked";
    else barrierResult = "timeout";
  }

  throwIfAborted(signal);
  const finalState = state(db);
  const envelope = getGenerationEnvelope(db);
  const receipt = {
    receiptId: `generation-${randomUUID()}`,
    createdMs: Date.now(),
    repoRoot,
    generationId: envelope?.manifest?.generationId ?? null,
    sourceClock: Number(finalState.source_clock ?? targetClock),
    appliedClock: Number(finalState.applied_clock ?? 0),
    eventGap: finalState.event_gap === "1",
    domainsPending: pendingDomains(finalState),
    barrierResult,
    details: {
      targetClock,
      elapsedMs: Date.now() - startedMs,
      allowDegraded: Boolean(allowDegraded),
      error: error?.message ?? null,
    },
  };
  // Re-seal the observation freshness is judged against once the watcher has
  // provably applied everything it saw.
  //
  // `evaluateFreshness` compares the sealed `sourceObservation` with the
  // worktree right now, but only a full `graph build` ever wrote that field.
  // So an ordinary commit — which moves HEAD without changing a single
  // indexed byte — left the graph permanently `changed_since_generation`:
  // recall degraded to `no_relevant_seed`, and a resident reader answered
  // `stale_blocked` for every query until someone rebuilt by hand. Caught up
  // with no event gap and nothing pending is precisely the state in which the
  // graph does describe the current tree, so that is when it is recorded.
  const reseal =
    barrierResult === "caught_up"
    && finalState.event_gap !== "1"
    && pendingDomains(finalState).length === 0
      ? gitSourceObservation(repoRoot)
      : null;
  db.exec("BEGIN;");
  try {
    recordBarrierDuration(db, receipt.details.elapsedMs);
    insertGenerationReceipt(db, receipt);
    if (reseal?.head && envelope?.manifest) {
      // `manifestDigest` is a checksum over an identity surface that includes
      // the observation, so the two move together or `manifestDigestValid`
      // goes false and the same staleness returns by another route. The
      // generation's own identity — nodes, edges, sourceHash — is untouched.
      const manifest = { ...envelope.manifest, manifestDigest: undefined };
      manifest.manifestDigest = computeManifestDigest(manifest, reseal);
      const put = db.prepare(
        "INSERT INTO generation (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
      );
      put.run("sourceObservation", JSON.stringify(reseal));
      put.run("manifest", JSON.stringify(manifest));
    }
    db.exec("COMMIT;");
  } catch (persistError) {
    db.exec("ROLLBACK;");
    throw persistError;
  }
  return receipt;
}

export async function syncToCurrentSourceAtPath(root, { outDir = ".agent", ...options } = {}) {
  const dbPath = join(resolve(root), outDir, "graph", "graph.db");
  if (!existsSync(dbPath)) throw new Error("graph store is missing");
  return withStoreLease(dbPath, { ownerKind: "one_shot" }, async () => {
    const db = openStore(dbPath);
    try { return await syncToCurrentSource(db, root, { ...options, outDir }); }
    finally { closeStore(db); }
  });
}

/**
 * Observe watcher progress without taking write ownership. Resident query
 * roles use this while the Hub-hosted RepositoryActor holds the store lease;
 * direct one-shot callers use syncToCurrentSourceAtPath instead.
 */
export function observeCurrentSourceAtPath(root, { outDir = ".agent" } = {}) {
  const repoRoot = canonicalRoot(root);
  const dbPath = join(repoRoot, outDir, "graph", "graph.db");
  if (!existsSync(dbPath)) throw new Error("graph store is missing");
  const db = openStoreReadOnly(dbPath);
  try {
    const current = state(db);
    const envelope = getGenerationEnvelope(db);
    const sourceClock = Number(current.source_clock ?? 0);
    const appliedClock = Number(current.applied_clock ?? 0);
    const pendingEvents = Number(db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=0").get()?.n ?? 0);
    const caughtUp = current.event_gap !== "1" && appliedClock >= sourceClock && pendingEvents === 0;
    return Object.freeze({
      receiptId: `generation-readonly-${envelope?.manifest?.generationId ?? "missing"}`,
      createdMs: Date.now(),
      repoRoot,
      generationId: envelope?.manifest?.generationId ?? null,
      sourceClock,
      appliedClock,
      eventGap: current.event_gap === "1",
      domainsPending: pendingDomains(current),
      barrierResult: caughtUp ? "caught_up" : "timeout",
      details: { readOnly: true, pendingEvents },
    });
  } finally {
    closeStore(db);
  }
}
