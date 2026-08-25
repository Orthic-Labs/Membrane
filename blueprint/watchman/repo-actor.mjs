import { EventEmitter } from "node:events";
import { existsSync, mkdirSync, appendFileSync, realpathSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { applyFileDelta, DOC_PROVIDER, MAX_DEPENDENT_FILES, MAX_HOPS, STRUCTURAL_PROVIDER } from "../src/graph/delta-store.mjs";
import { parseFileFacts } from "../src/graph/static-provider.mjs";
import { buildIncrementalTreeSitterFacts, SUPPORTED_EXTENSIONS } from "../src/graph/treesitter-provider.mjs";
import { extractDoc, isDoc, loadConfig } from "../scripts/blueprint.mjs";
import { MAX_SOURCE_FILE_BYTES, stableRead } from "../src/graph/stable-read.mjs";
import { assertSafeMutableStorePath, collectDependents, closeStore, listFileMetadata, listSymbolMetadata, maintainStore, openStore, openStoreReadOnly } from "../src/graph/store-sqlite.mjs";
import { acquireStoreLease } from "../src/graph/store-lease.mjs";
import { eventsSince, isEligibleWatchPath, startWatch, writeSnapshot } from "./adapter.mjs";
import { normalizeIgnoredPrefixes } from "../src/graph/ignored-prefixes.mjs";

const REPAIR_BATCH = 50;
const DEBOUNCE_MS = 1000;
const MAX_DRAIN_PASSES = 5;

function normalizePath(value) { return String(value).replaceAll("\\", "/").replace(/^\.\//, ""); }
function canonicalRoot(value) { const root = resolve(value); try { return realpathSync(root); } catch { return root; } }
function dbPath(root, outDir) { return join(resolve(root), outDir, "graph", "graph.db"); }
function stateValue(db, key, fallback = null) { return db.prepare("SELECT value FROM watch_state WHERE key=?").get(key)?.value ?? fallback; }
function setState(db, key, value) { db.prepare("INSERT INTO watch_state(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(key, String(value)); }
function isDocumentPath(path) {
  const normalized = normalizePath(path);
  return normalized.endsWith(".md") || ["AGENTS.md", "CLAUDE.md", "README.md"].includes(normalized);
}
function isUnchangedDocumentStartupEvent(db, root, event) {
  if (!isDocumentPath(event?.path) || !["create", "modify"].includes(event?.eventKind)) return false;
  const path = normalizePath(event.path);
  const leaf = db.prepare("SELECT digest FROM generation_leaf WHERE path=? AND kind='file'").get(path);
  if (!leaf) return false;
  try { return stableRead(join(root, path)).contentDigest === leaf.digest; }
  catch { return false; }
}
function sourceFilesFromStore(db) {
  return listFileMetadata(db).map((file) => ({
    path: file.path,
    contentHash: String(file.contentDigest ?? "").replace(/^xxh128:/, ""),
    size: Number(file.size ?? 0),
  }));
}

export function coalesceWatchEvents(events) {
  const latest = new Map();
  for (const event of events ?? []) if (event?.path) latest.set(normalizePath(event.path), event);
  return [...latest.values()];
}

export function appendWatchEvents(db, events) {
  let clock = Number(stateValue(db, "source_clock", 0));
  const insert = db.prepare("INSERT INTO event_journal(observed_ms,event_kind,path,rename_to,source_clock) VALUES (?,?,?,?,?)");
  const result = [];
  db.exec("BEGIN IMMEDIATE");
  try {
    for (const event of coalesceWatchEvents(events)) {
      clock += 1;
      const info = insert.run(Number(event.observedMs ?? Date.now()), event.eventKind, normalizePath(event.path), event.renameTo ? normalizePath(event.renameTo) : null, clock);
      result.push({ seq: Number(info.lastInsertRowid), sourceClock: clock });
    }
    setState(db, "source_clock", clock);
    db.exec("COMMIT");
    return result;
  } catch (error) { db.exec("ROLLBACK"); throw error; }
}

function descriptorFor(root, sourceFiles, path, renameTo = null, readStable = stableRead) {
  const normalized = normalizePath(path);
  const target = normalizePath(renameTo ?? normalized);
  const absolute = resolve(root, target);
  if (!existsSync(absolute)) return null;
  // Only a regular file within the source bound is readable. existsSync() is
  // true for directories too, and the journal genuinely carries directory
  // paths (watchers report `create` for a new directory), so reading without
  // this check throws EISDIR — inside the journal-apply write transaction,
  // which is the same corruption path an oversized file took. Anything else,
  // oversized or not-a-file, routes through the same branch as a path that
  // does not exist and is simply dropped from the graph.
  try {
    const stat = statSync(absolute);
    if (!stat.isFile() || stat.size > MAX_SOURCE_FILE_BYTES) return null;
  } catch { return null; }
  const read = readStable(absolute);
  const text = read.bytes.toString("utf8");
  const descriptor = {
    absolutePath: absolute,
    path: target,
    text,
    lines: text.split(/\r?\n/),
    contentHash: read.contentDigest.replace(/^xxh128:/, ""),
    size: read.bytes.length,
  };
  return { descriptor, read, files: sourceFiles.filter((file) => ![normalized, target].includes(normalizePath(file.path))).concat(descriptor) };
}

async function deltaFor(db, root, event, readStable = stableRead, signal) {
  throwIfAborted(signal);
  const sourceFiles = sourceFilesFromStore(db);
  const normalized = normalizePath(event.path);
  const target = normalizePath(event.renameTo ?? normalized);
  const current = descriptorFor(root, sourceFiles, normalized, event.renameTo, readStable);
  const document = current && isDoc(target)
    ? extractDoc(root, target, new Set(current.files.map((file) => normalizePath(file.path))), loadConfig(root, ".agent"))
    : null;
  const provider = (document || isDoc(normalized) || isDoc(target)) ? DOC_PROVIDER : { id: "lexical", version: "repo-local-delta-v1" };
  if (!current) return { eventKind: event.eventKind === "rename" ? "rename" : "delete", path: normalized, renameTo: event.renameTo ?? null, parsed: null, provider, ...(provider.id === DOC_PROVIDER.id ? { domain: "doc" } : {}) };
  const lexical = parseFileFacts(root, current.descriptor, { files: current.files, symbols: listSymbolMetadata(db, "lexical") });
  const extension = target.includes(".") ? target.slice(target.lastIndexOf(".") + 1).toLowerCase() : "";
  const factBatches = [{ provider: STRUCTURAL_PROVIDER, parsed: lexical }];
  if (!document && SUPPORTED_EXTENSIONS.includes(extension)) {
    factBatches.push(await buildIncrementalTreeSitterFacts(current.descriptor, {
      filePaths: current.files.map((file) => file.path),
      symbols: listSymbolMetadata(db, "treesitter"),
    }));
    throwIfAborted(signal);
  }
  return {
    eventKind: event.eventKind,
    path: normalized,
    renameTo: event.renameTo ?? null,
    parsed: lexical,
    factBatches,
    contentDigest: current.read.contentDigest,
    fileIdentity: current.read.fileIdentity,
    size: current.read.bytes.length,
    mtimeMs: current.read.statAfter.mtimeMs,
    provider: document ? provider : STRUCTURAL_PROVIDER,
    ...(document ? { domain: "doc", document } : {}),
  };
}

function writeRepairState(db, state) {
  if (state) setState(db, "repair_truncated", JSON.stringify(state));
  else db.prepare("DELETE FROM watch_state WHERE key='repair_truncated'").run();
}

function throwIfAborted(signal) { if (signal?.aborted) throw Object.assign(new Error("request cancelled"), { code: "request_cancelled" }); }

async function prepareRepairPaths(db, root, paths, readStable = stableRead, signal) {
  const prepared = [];
  for (const path of paths) { throwIfAborted(signal); prepared.push({ path, delta: await deltaFor(db, root, { eventKind: "repair", path }, readStable, signal) }); }
  return prepared;
}

function applyRepairPaths(db, root, prepared, sourceClock, inOuterTransaction) {
  const batches = [];
  for (let index = 0; index < prepared.length; index += REPAIR_BATCH) batches.push(prepared.slice(index, index + REPAIR_BATCH));
  for (const batch of batches) {
    const ownBatch = !inOuterTransaction;
    if (ownBatch) db.exec("BEGIN IMMEDIATE");
    try {
      for (const { path, delta } of batch) {
        applyFileDelta(db, { ...delta, sourceClock }, { inTransaction: true, repoRoot: root, outDir: ".agent" });
      }
      if (ownBatch) db.exec("COMMIT");
    } catch (error) {
      if (ownBatch) db.exec("ROLLBACK");
      throw error;
    }
  }
}

async function applyJournalEvent(db, root, row, maxDependentFiles = MAX_DEPENDENT_FILES, readStable = stableRead, signal) {
  throwIfAborted(signal);
  const closure = collectDependents(db, row.path, { maxHops: MAX_HOPS, maxFiles: maxDependentFiles });
  const event = { eventKind: row.event_kind, path: row.path, renameTo: row.rename_to };
  const baseDelta = await deltaFor(db, root, event, readStable, signal);
  throwIfAborted(signal);
  const base = applyFileDelta(db, { ...baseDelta, sourceClock: row.source_clock, journalSeq: row.seq }, { repoRoot: root, outDir: ".agent" });
  let repairDeltas = [];
  try {
    // The base delta is committed before dependent parsing starts. This keeps
    // all awaits outside write locks while ensuring repair deltas observe the
    // freshly-applied source facts, matching a cold build.
    if (base.applied && !base.noop && closure.paths.length) {
      repairDeltas = await prepareRepairPaths(db, root, closure.paths, readStable, signal);
      throwIfAborted(signal);
      if (repairDeltas.length) applyRepairPaths(db, root, repairDeltas, row.source_clock, false);
    }
    db.exec("BEGIN IMMEDIATE");
    if (closure.truncated) writeRepairState(db, { path: row.path, remaining: closure.remaining });
    else writeRepairState(db, null);
    db.prepare("UPDATE event_journal SET applied=1, applied_clock=? WHERE seq=?").run(base.appliedClock ?? row.source_clock, row.seq);
    db.exec("COMMIT");
    return base;
  } catch (error) {
    try { db.exec("ROLLBACK"); } catch {}
    throw error;
  }
}

function pendingRows(db, force = false) {
  const threshold = Date.now() - DEBOUNCE_MS;
  const rows = db.prepare("SELECT * FROM event_journal WHERE applied=0 AND (? OR observed_ms<=?) ORDER BY seq").all(force ? 1 : 0, threshold);
  const latest = new Map();
  for (const row of rows) latest.set(row.path, row);
  for (const row of rows) {
    if (latest.get(row.path)?.seq !== row.seq) db.prepare("UPDATE event_journal SET applied=2 WHERE seq=?").run(row.seq);
  }
  return [...latest.values()].sort((left, right) => left.seq - right.seq);
}

export async function drainJournal(db, root, { force = true, maxDependentFiles = MAX_DEPENDENT_FILES, readStable = stableRead, signal } = {}) {
  let applied = 0;
  for (let pass = 0; pass < MAX_DRAIN_PASSES; pass += 1) {
    const rows = pendingRows(db, force);
    if (!rows.length) break;
    for (const row of rows) { await applyJournalEvent(db, root, row, maxDependentFiles, readStable, signal); applied += 1; }
    force = true;
  }
  const pending = db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=0").get().n;
  if (pending > 0) {
    setState(db, "event_gap", 1);
    setState(db, "event_gap_reason", "journal_overflow");
    setState(db, "last_error", `watcher journal exceeded ${MAX_DRAIN_PASSES} drain passes (${pending} pending)`);
  } else maintainStore(db);
  return applied;
}

// @parcel/watcher surfaces a dropped-events condition ("Events were dropped by
// the FSEvents client. File system must be re-scanned." on macOS; inotify's
// queue overflow reads similarly) as an ERROR delivered to the subscribe
// callback, not as a normal create/update/delete event — normalizeEvents()
// already filters non-EVENT_TYPES entries, so the synthetic `eventKind:
// "overflow"` path in ingest() below is reachable only via direct/manual
// injection (BlueprintRepositoryWorker's own "overflow" kind, and tests). Both
// paths converge on the same typed reason so `blueprint-watch status` reports
// the real condition instead of an opaque wrapped error string.
const OVERFLOW_ERROR_PATTERN = /dropped by the .*client|must be re-scanned|queue overflow/i;
function isOverflowError(error) {
  return OVERFLOW_ERROR_PATTERN.test(String(error?.message ?? error ?? ""));
}

export class RepositoryActor extends EventEmitter {
  constructor({ root, outDir = ".agent", snapshotPath = null, reconcile = null, adapter = { startWatch, writeSnapshot, eventsSince }, maxDependentFiles = MAX_DEPENDENT_FILES, readStable = stableRead, ignore = [], ownerId = null }) {
    super();
    this.root = canonicalRoot(root);
    this.outDir = outDir;
    // The owning supervisor's instance token (G6): stamped into watch_state
    // alongside watcher_pid so a later status read can tell "this actor was
    // (re)started by the CURRENTLY live supervisor" apart from "this actor's
    // pid record was last written by some prior, possibly-dead incarnation"
    // — see repoStatus() in watchman/supervisor.mjs. null when constructed
    // outside a WatchSupervisor (e.g. directly in tests); no owner stamp is
    // written in that case, which leaves status()'s legacy pid-alive check
    // as the sole liveness signal, unchanged from before this fix.
    this.ownerId = ownerId;
    this.dbPath = dbPath(this.root, outDir);
    this.snapshotPath = snapshotPath ? resolve(snapshotPath) : join(this.root, outDir, "graph", "watch.snapshot");
    this.reconcile = reconcile ?? (async () => ({ ok: true }));
    this.autoReconcileOnGap = Boolean(reconcile);
    this.adapter = adapter;
    this.maxDependentFiles = maxDependentFiles;
    this.readStable = readStable;
    // Exact relative paths (never globs — see adapter.mjs) to exclude from this
    // actor's own subscription: today, an enrolled sibling repo nested under
    // this actor's root. Own `.agent` output is already excluded by the base
    // ignore set in adapter.mjs, at every actor, not just this one.
    const configuredIgnore = normalizeIgnoredPrefixes(loadConfig(this.root, outDir)?.ignoredPrefixes);
    this.ignore = [...new Set([...ignore, ...configuredIgnore])];
    this.subscription = null;
    this.timer = null;
    this.running = false;
    this.failures = 0;
    this.retryTimer = null;
    // The store handle is opened once and held for the actor's lifetime
    // (D3): every prior revision reopened a fresh DatabaseSync — which also
    // re-runs migrate()'s schema-version check — on every single ingest,
    // flush, and gap, multiplying open/close churn against the same file
    // that `blueprint build` and the status poller also touch concurrently.
    this.db = null;
    // Hub owns this lease for exactly the actor's writable lifetime. The DB is
    // never opened before the OS lock is won, and the lock is released only
    // after the final writable handle closes.
    this.storeLease = null;
    // Coalesces gap-repair reconciles: a real overflow burst can re-fire the
    // gap callback repeatedly (that IS the overflow reported in production).
    // Without this guard, each firing would open its own concurrent reconcile
    // against the same handle. At most one reconcile runs at a time; a gap
    // signal that arrives mid-reconcile is remembered and re-run exactly once
    // after the in-flight one finishes, never spawned as a second concurrent
    // pass.
    this.reconcileInFlight = null;
    this.reconcilePending = false;
    this.drainInFlight = null;
    // Native callbacks can arrive faster than SQLite writes. Buffering through
    // debounce coalesces before journal persistence instead of recording rows
    // that will immediately become superseded.
    this.eventBuffer = [];
    this.stopController = new AbortController();
    this.epoch = 0;
    this.stopGeneration = 0;
    this.lifecycle = Promise.resolve();
    this.run = null;
  }

  log(error) {
    if (!existsSync(this.root)) return;
    const logPath = join(this.root, this.outDir, "graph", "watchman.log");
    mkdirSync(dirname(logPath), { recursive: true });
    appendFileSync(logPath, `${new Date().toISOString()} ${error?.stack ?? error}\n`);
  }

  openDbOnce() {
    // CX-F165: the watcher is the canonical mutable-state owner, so it refuses
    // typed rather than silently landing a WAL store on synced/shared storage.
    if (!this.db) {
      assertSafeMutableStorePath(this.dbPath);
      this.storeLease = acquireStoreLease(this.dbPath, {
        ownerKind: "hub",
        ...(this.ownerId ? { ownerInstanceId: this.ownerId } : {}),
      });
      try {
        this.db = openStore(this.dbPath, { mutablePathPolicy: "refuse" });
      } catch (error) {
        this.storeLease.release();
        this.storeLease = null;
        throw error;
      }
    }
    return this.db;
  }

  async initialize() {
    this.hadSnapshot = existsSync(this.snapshotPath);
    const db = this.openDbOnce();
    setState(db, "watcher_pid", process.pid);
    if (this.ownerId) setState(db, "watcher_owner", this.ownerId);
    setState(db, "event_gap", 0);
    db.prepare("DELETE FROM watch_state WHERE key='last_error'").run();
    db.prepare("DELETE FROM watch_state WHERE key='event_gap_reason'").run();
  }

  beginRun() {
    const run = { epoch: ++this.epoch, controller: new AbortController(), work: new Set() };
    this.run = run;
    this.stopController = run.controller;
    return run;
  }

  current(run) { return this.run === run && !run.controller.signal.aborted; }

  active(run) { return this.current(run) && this.running; }

  track(run, work) {
    const tracked = Promise.resolve(work);
    run.work.add(tracked);
    void tracked.then(
      () => run.work.delete(tracked),
      () => run.work.delete(tracked),
    );
    return tracked;
  }

  queueLifecycle(work) {
    const queued = this.lifecycle.then(work, work);
    this.lifecycle = queued.catch(() => {});
    return queued;
  }

  start() {
    const expectedEpoch = this.epoch;
    return this.queueLifecycle(async () => {
      if (this.running || this.epoch !== expectedEpoch) return;
      const run = this.beginRun();
      this.running = true;
      return this.track(run, this.startRun(run));
    });
  }

  async startRun(run) {
    try {
      if (!existsSync(this.root)) throw new Error(`watch root is unavailable: ${this.root}`);
      await this.initialize();
      if (!this.active(run)) return;
      // Both callbacks are invoked by the watcher outside any promise chain, so
      // anything they throw is an unhandled exception that takes down the whole
      // supervisor — every repo, not just this one. That is exactly what
      // happened in production: one repo's `database is locked` inside
      // markGap()'s setState killed the process, launchd restarted it, the
      // 19-repo cold sweep began again from the top and died around repo 10, so
      // the fleet sat permanently short of converging. A single repo failing to
      // record its own gap must degrade that repo, never the fleet.
      const subscription = await this.track(run, this.adapter.startWatch(
        this.root,
        (events) => this.guardCallback(run, () => this.ingest(events, run)),
        (error) => this.guardCallback(run, () => this.markGap(error, isOverflowError(error) ? "event_overflow" : "watch_subscription_error", run)),
        this.ignore,
        { signal: run.controller.signal },
      ));
      if (!this.active(run)) {
        try { await subscription.unsubscribe(); } catch (error) { this.log(error); }
        return;
      }
      this.subscription = subscription;
      // startWatch resolves only after the native callback observes its probe.
      // Reconcile the saved-snapshot gap only after that readiness barrier,
      // then checkpoint this exact post-reconcile state for next startup.
      if (this.hadSnapshot) {
        const events = await this.track(run, this.adapter.eventsSince(this.root, this.snapshotPath, this.ignore));
        if (!this.active(run)) return;
        const startupEvents = events.filter((event) => !isUnchangedDocumentStartupEvent(this.openDbOnce(), this.root, event));
        if (startupEvents.length) { this.ingest(startupEvents, run); await this.flush(true, run); }
      }
      if (!this.active(run)) return;
      const startupReconcile = this.track(run, this.reconcile(this.openDbOnce(), this.root, { outDir: this.outDir, snapshotPath: this.snapshotPath, maxDependentFiles: this.maxDependentFiles, ignore: this.ignore, signal: run.controller.signal }));
      this.reconcileInFlight = startupReconcile;
      try { await startupReconcile; } finally { if (this.active(run) && this.reconcileInFlight === startupReconcile) this.reconcileInFlight = null; if (this.active(run) && this.reconcilePending) this.runPendingReconcile(run); }
      if (!this.active(run)) return;
      await this.track(run, this.adapter.writeSnapshot(this.root, this.snapshotPath, this.ignore));
      this.failures = 0;
    } catch (error) {
      if (!this.current(run) || error?.code === "request_cancelled") return;
      this.handleFailure(error, null, run);
      this.running = false;
      throw error;
    }
  }

  // Runs a watcher callback so that no failure inside it can escape into the
  // process's unhandled-exception path. The failure is still recorded against
  // this actor (log + failure count), so it stays visible rather than silent.
  guardCallback(run, work) {
    if (!this.active(run)) return undefined;
    try { return work(); } catch (error) {
      try { this.handleFailure(error, "watch_callback_error", run); } catch { this.log(error); }
      return undefined;
    }
  }

  // Marks the store honestly stale and schedules exactly one repair reconcile
  // (coalesced — see the constructor comment). `reason` names the condition
  // (`event_overflow`, `watch_subscription_error`, or a caller-supplied value)
  // so status reads the typed condition instead of reason-sniffing free text.
  markGap(error, reason = "watch_error", run = null) {
    if (run && !this.current(run)) return;
    const db = this.openDbOnce();
    setState(db, "event_gap", 1);
    setState(db, "event_gap_reason", reason);
    if (error) setState(db, "last_error", String(error?.message ?? error).slice(0, 500));
    if (error) this.log(error);
    this.emit("gap", error);
    if (!this.autoReconcileOnGap) return;
    this.reconcilePending = true;
    this.runPendingReconcile(run ?? this.run);
  }

  runPendingReconcile(run = this.run) {
    if (run && !this.active(run)) return undefined;
    if (this.reconcileInFlight) return this.reconcileInFlight;
    const signal = run?.controller.signal ?? this.stopController.signal;
    const work = Promise.resolve().then(async () => {
      try {
        while (this.reconcilePending) {
          if (run && !this.active(run)) return;
          this.reconcilePending = false;
          const db = this.openDbOnce();
          try { await this.reconcile(db, this.root, { outDir: this.outDir, snapshotPath: this.snapshotPath, maxDependentFiles: this.maxDependentFiles, ignore: this.ignore, signal }); }
          catch (reconcileError) { this.log(reconcileError); }
        }
      } finally { if (!run || this.active(run)) this.reconcileInFlight = null; }
    });
    this.reconcileInFlight = run ? this.track(run, work) : work;
    return this.reconcileInFlight;
  }

  ingest(events, run = null) {
    if (run && !this.active(run)) return [];
    if (!events?.length) return [];
    if (events.some((event) => event.eventKind === "overflow")) {
      this.markGap(new Error("watcher overflow"), "event_overflow", run);
      return [];
    }
    const eligible = events.filter((event) => isEligibleWatchPath(event.path, this.ignore)
      && (!event.renameTo || isEligibleWatchPath(event.renameTo, this.ignore)));
    if (!eligible.length) return [];
    this.eventBuffer.push(...eligible);
    this.scheduleFlush(run);
    return eligible;
  }

  flush(force = false, run = null) {
    if (run && !this.active(run)) return Promise.resolve(0);
    if (this.drainInFlight) return this.drainInFlight;
    const db = this.openDbOnce();
    const signal = run?.controller.signal ?? this.stopController.signal;
    const drain = (async () => {
      let applied = 0;
      // Events can arrive while parsing yields. Repeat until both the buffer
      // and journal are caught up, bounded inside drainJournal itself.
      do {
        const batch = this.eventBuffer.splice(0);
        if (batch.length) appendWatchEvents(db, batch);
        applied += await drainJournal(db, this.root, { force, maxDependentFiles: this.maxDependentFiles, readStable: this.readStable, signal });
        force = true;
      } while (this.eventBuffer.length);
      return applied;
    })();
    const settledDrain = drain.then((applied) => {
      if (Number(stateValue(db, "event_gap", 0)) === 1 && stateValue(db, "event_gap_reason") === "journal_overflow") this.markGap(new Error("watcher journal overflow"), "journal_overflow", run);
      return applied;
    }).finally(() => {
      if (this.drainInFlight === settledDrain) this.drainInFlight = null;
    });
    this.drainInFlight = settledDrain;
    if (run) this.track(run, settledDrain);
    return settledDrain;
  }

  scheduleFlush(run = null) {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      if (!run || this.active(run)) this.flush(false, run).catch((error) => this.handleFailure(error, null, run));
    }, DEBOUNCE_MS);
    this.timer.unref?.();
  }

  handleFailure(error, reason = null, run = null) {
    if (run && !this.active(run)) return;
    this.failures += 1;
    this.log(error);
    if (reason) this.markGap(error, reason, run);
    else if (this.failures >= 5) this.markGap(error, isOverflowError(error) ? "event_overflow" : "watch_subscription_error", run);
    const delay = this.failures >= 5 ? 60000 : Math.min(30000, 1000 * 2 ** (this.failures - 1));
    clearTimeout(this.retryTimer);
    this.retryTimer = setTimeout(() => {
      if (run && !this.current(run)) return;
      const retryEpoch = this.epoch;
      const stopGeneration = this.stopGeneration;
      this.stop({ retry: true }).then(() => {
        if (this.epoch !== retryEpoch + 1 || this.stopGeneration !== stopGeneration) return;
        return this.start();
      }).catch((retryError) => this.log(retryError));
    }, delay);
    this.retryTimer.unref?.();
  }

  stop({ retry = false } = {}) {
    const run = this.run;
    const subscription = this.subscription;
    this.run = null;
    this.epoch += 1;
    if (!retry) this.stopGeneration += 1;
    this.running = false;
    clearTimeout(this.timer);
    clearTimeout(this.retryTimer);
    run?.controller.abort();
    this.stopController.abort();
    this.subscription = null;
    return this.queueLifecycle(() => this.finishStop(run, subscription));
  }

  async finishStop(run, subscription) {
    let stopError;
    if (subscription) {
      try { await subscription.unsubscribe(); }
      catch (error) { this.log(error); }
    }
    const work = run?.work ?? new Set([this.drainInFlight, this.reconcileInFlight].filter(Boolean));
    while (work.size) {
      const pendingWork = [...work];
      const settled = await Promise.allSettled(pendingWork);
      for (const result of settled) if (result.status === "rejected") stopError ??= result.reason;
      for (const pending of pendingWork) work.delete(pending);
    }
    if (this.db) {
      try {
        this.db.prepare("DELETE FROM watch_state WHERE key='watcher_pid'").run();
        this.db.prepare("DELETE FROM watch_state WHERE key='watcher_owner'").run();
      } catch { /* best-effort on a store that may already be gone */ }
      try {
        closeStore(this.db);
      } finally {
        this.db = null;
        this.storeLease?.release();
        this.storeLease = null;
      }
    } else if (existsSync(this.dbPath)) {
      // Never started (no held handle) but the store already exists — a
      // one-off writable open is unavoidable here, matched by an immediate
      // close; there is no long-lived handle to reuse.
      assertSafeMutableStorePath(this.dbPath);
      const lease = acquireStoreLease(this.dbPath, { ownerKind: "one_shot" });
      let db;
      try {
        db = openStore(this.dbPath, { mutablePathPolicy: "refuse" });
        db.prepare("DELETE FROM watch_state WHERE key='watcher_pid'").run();
        db.prepare("DELETE FROM watch_state WHERE key='watcher_owner'").run();
      } finally {
        if (db) closeStore(db);
        lease.release();
      }
    }
    if (stopError && stopError.code !== "request_cancelled") throw stopError;
  }
}

export class BlueprintRepositoryWorker {
  constructor(options) { this.actor = new RepositoryActor(options); }
  async ingest(path, eventKind = "modify", renameTo = null) {
    try { return await this.#ingestOnce(path, eventKind, renameTo); }
    finally { await this.actor.stop(); }
  }
  async #ingestOnce(path, eventKind = "modify", renameTo = null) {
    const event = { path, eventKind, renameTo, observedMs: Date.now() };
    if (eventKind === "overflow") {
      this.actor.markGap(new Error("watcher overflow"), "event_overflow");
      return { eventGap: true, reconciled: false };
    }
    this.actor.ingest([event]);
    const appliedCount = await this.actor.flush(true);
    // Read-only lookback at a row this call itself just wrote — no mutation,
    // so this must never be the writable opener (D3).
    const db = openStoreReadOnly(this.actor.dbPath);
    try {
      const row = db.prepare("SELECT * FROM event_journal WHERE path=? ORDER BY seq DESC LIMIT 1").get(normalizePath(path));
      return { sourceClock: Number(row.source_clock), applied: appliedCount > 0, journalSeq: Number(row.seq), appliedClock: row.applied_clock, eventGap: false };
    } finally { closeStore(db); }
  }
}
