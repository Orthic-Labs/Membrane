import { randomUUID } from "node:crypto";
import { existsSync, realpathSync } from "node:fs";
import { join, resolve } from "node:path";
import { reconcile } from "../watchman/reconcile.mjs";
import { closeStore, getGenerationEnvelope, insertGenerationReceipt, openStore } from "./store-sqlite.mjs";
import { recordBarrierDuration } from "../lib/telemetry.mjs";

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

async function bounded(promise, timeoutMs, signal) {
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
    if (["barrier_timeout", "request_cancelled"].includes(error?.code)) await work.catch(() => {});
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
  const runReconcile = () => bounded(reconcileFn(db, repoRoot, { outDir, signal }), Math.max(1, timeoutMs - (Date.now() - startedMs)), signal);

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
  db.exec("BEGIN;");
  try {
    recordBarrierDuration(db, receipt.details.elapsedMs);
    insertGenerationReceipt(db, receipt);
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
  const db = openStore(dbPath);
  try { return await syncToCurrentSource(db, root, { ...options, outDir }); }
  finally { closeStore(db); }
}
