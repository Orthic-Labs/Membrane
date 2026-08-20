import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { CortexRepositoryWorker, RepositoryActor } from "../src/graph/watchman.mjs";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";
import { MAX_SOURCE_FILE_BYTES, stableRead } from "../src/graph/stable-read.mjs";
import { isEligibleWatchPath, normalizeEvents, startWatch, waitForNativeProbe } from "../watchman/adapter.mjs";

const ROOT = join(import.meta.dirname, "..");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

test("watch paths survive macOS /var to /private/var canonicalization", { skip: process.platform !== "darwin" }, () => {
  const repo = mkdtempSync("/var/tmp/cortex-watchman-path-");
  try {
    const eventPath = join(realpathSync(repo), "src/service.ts");
    assert.equal(normalizeEvents(repo, [{ type: "update", path: eventPath }])[0].path, "src/service.ts");
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("watch worker persists source/apply clocks and applies one-file delta", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-"));
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const path = join(repo, "src/service.ts");
    writeFileSync(path, `${readFileSync(path, "utf8")}\nexport const watchmanChange = true;\n`);
    const result = await new CortexRepositoryWorker({ root: repo }).ingest("src/service.ts");
    assert.equal(result.applied, true);
    assert.equal(result.sourceClock, 1);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      assert.equal(db.prepare("SELECT value FROM watch_state WHERE key='source_clock'").get().value, "1");
      assert.equal(db.prepare("SELECT applied FROM event_journal WHERE seq=?").get(result.journalSeq).applied, 1);
      assert.equal(db.prepare("SELECT value FROM watch_state WHERE key='applied_clock'").get().value, "1");
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("overflow is durable and does not claim reconciliation", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-gap-"));
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const result = await new CortexRepositoryWorker({ root: repo }).ingest(".", "overflow");
    assert.equal(result.eventGap, true);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try { assert.equal(db.prepare("SELECT value FROM watch_state WHERE key='event_gap'").get().value, "1"); }
    finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("worker awaits overflow reconciliation before closing", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-reconcile-"));
  let reconciled = false;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const worker = new CortexRepositoryWorker({ root: repo, reconcile: async () => { await new Promise((resolve) => setImmediate(resolve)); reconciled = true; } });
    const result = await worker.ingest(".", "overflow");
    assert.equal(result.eventGap, true);
    assert.equal(reconciled, true);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("worker closes its store after an ingest error", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-worker-error-"));
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const worker = new CortexRepositoryWorker({ root: repo, readStable() { throw new Error("sentinel"); } });
    await assert.rejects(worker.ingest("src/service.ts"), /sentinel/);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("burst coalesces before persistence to one journal row and one applied delta", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-burst-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    actor = new RepositoryActor({ root: repo });
    const path = join(repo, "src/service.ts");
    for (let index = 0; index < 20; index += 1) {
      writeFileSync(path, `${readFileSync(path, "utf8")}\nexport const burst${index} = true;\n`);
      actor.ingest([{ eventKind: "modify", path: "src/service.ts", observedMs: Date.now() }]);
    }
    await actor.flush(true);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=1").get().n, 1);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal").get().n, 1);
    } finally { closeStore(db); }
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("event eligibility rejects files below directories created after subscription", () => {
  assert.equal(isEligibleWatchPath("src/node_modules/created-later/noise.js"), false);
  assert.equal(isEligibleWatchPath("nested/.agent-test-run/graph.db"), false);
  assert.equal(isEligibleWatchPath("nested/child-repo/new-file.ts", ["nested/child-repo"]), false);
  assert.equal(isEligibleWatchPath("src/keep.ts"), true);
  const root = mkdtempSync(join(tmpdir(), "cortex-watchman-dynamic-exclusion-"));
  try {
    const events = normalizeEvents(root, [{ type: "create", path: join(root, "src", "node_modules", "created-later", "noise.js") }]);
    assert.deepEqual(events, []);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("actor rejects excluded ingress before journal persistence", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-actor-exclusion-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    actor = new RepositoryActor({ root: repo });
    assert.deepEqual(actor.ingest([{ eventKind: "modify", path: "src/node_modules/later/noise.js", observedMs: Date.now() }]), []);
    assert.equal(await actor.flush(true), 0);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try { assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal").get().n, 0); }
    finally { closeStore(db); }
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("actor rejects configured ignored-prefix ingress before journal persistence", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-config-exclusion-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    mkdirSync(join(repo, ".agent"), { recursive: true });
    writeFileSync(join(repo, ".agent", "config.json"), JSON.stringify({ ignoredPrefixes: ["benchmarks/"] }));
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    actor = new RepositoryActor({ root: repo });
    assert.deepEqual(actor.ingest([{ eventKind: "modify", path: "benchmarks/new/noise.js", observedMs: Date.now() }]), []);
    assert.equal(await actor.flush(true), 0);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try { assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal").get().n, 0); }
    finally { closeStore(db); }
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("two files arriving during one drain are both applied", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-drain-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    writeFileSync(join(repo, "src/service.ts"), `${readFileSync(join(repo, "src/service.ts"), "utf8")}\nexport const drainService = true;\n`);
    writeFileSync(join(repo, "src/store.ts"), `${readFileSync(join(repo, "src/store.ts"), "utf8")}\nexport const drainStore = true;\n`);
    actor = new RepositoryActor({ root: repo });
    actor.ingest([
      { eventKind: "modify", path: "src/service.ts", observedMs: Date.now() },
      { eventKind: "modify", path: "src/store.ts", observedMs: Date.now() },
    ]);
    assert.equal(await actor.flush(true), 2);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try { assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=1").get().n, 2); }
    finally { closeStore(db); }
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("an event arriving while parsing yields is queued, never lost, and drains", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-late-event-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const firstPath = join(repo, "src/service.ts");
    const secondPath = join(repo, "src/store.ts");
    writeFileSync(firstPath, `${readFileSync(firstPath, "utf8")}\nexport const pausedService = true;\n`);
    writeFileSync(secondPath, `${readFileSync(secondPath, "utf8")}\nexport const lateStore = true;\n`);
    let parseStarted;
    const parseReached = new Promise((resolve) => { parseStarted = resolve; });
    let reads = 0;
    const readStable = (path) => {
      const result = stableRead(path);
      if (reads++ === 0) parseStarted();
      return result;
    };
    actor = new RepositoryActor({ root: repo, readStable });
    actor.ingest([{ eventKind: "modify", path: "src/service.ts", observedMs: Date.now() }]);
    const drain = actor.flush(true);
    await parseReached;
    assert.doesNotThrow(() => actor.ingest([{ eventKind: "modify", path: "src/store.ts", observedMs: Date.now() }]));
    assert.equal(await drain, 2);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=1").get().n, 2);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=0").get().n, 0);
    } finally { closeStore(db); }
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("concurrent flush calls share one in-flight drain", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-flush-guard-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const path = join(repo, "src/service.ts");
    writeFileSync(path, `${readFileSync(path, "utf8")}\nexport const oneDrain = true;\n`);
    let parseStarted;
    const parseReached = new Promise((resolve) => { parseStarted = resolve; });
    actor = new RepositoryActor({
      root: repo,
      readStable(pathname) {
        const result = stableRead(pathname);
        parseStarted();
        return result;
      },
    });
    actor.ingest([{ eventKind: "modify", path: "src/service.ts", observedMs: Date.now() }]);
    const first = actor.flush(true);
    await parseReached;
    const second = actor.flush(true);
    assert.strictEqual(second, first);
    const [firstResult, secondResult] = await Promise.all([first, second]);
    assert.equal(firstResult, 1);
    assert.equal(secondResult, 1);
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("a gap recorded during a failed parse survives transaction rollback", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-gap-rollback-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    let onGap;
    actor = new RepositoryActor({
      root: repo,
      adapter: {
        writeSnapshot: async () => {},
        eventsSince: async () => [],
        startWatch: async (_root, _onEvents, gap) => { onGap = gap; return { unsubscribe() {} }; },
      },
      readStable(path) {
        onGap(new Error("parse failed after callback gap"));
        throw new Error("parse failed after callback gap");
      },
    });
    await actor.start();
    actor.ingest([{ eventKind: "modify", path: "src/service.ts", observedMs: Date.now() }]);
    await assert.rejects(actor.flush(true), /parse failed after callback gap/);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      assert.equal(db.prepare("SELECT value FROM watch_state WHERE key='event_gap'").get().value, "1");
      assert.equal(db.prepare("SELECT value FROM watch_state WHERE key='event_gap_reason'").get().value, "watch_subscription_error");
      assert.match(db.prepare("SELECT value FROM watch_state WHERE key='last_error'").get().value, /parse failed after callback gap/);
    } finally { closeStore(db); }
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

// Regression: heardright's graph was corrupted twice in one day by a file too
// large to become a JS string. descriptorFor() unconditionally did
// `read.bytes.toString("utf8")`, which throws past ~512 MB — and the throw
// landed inside applyJournalEvent's write transaction, so the store came back
// "malformed database schema" rather than merely skipping the file. The full
// build never hit this because its readers have always capped source at 2 MiB;
// the incremental watcher path was the one reader missing the bound. Asserted
// by refusing to read at all: a reader that throws if invoked proves the
// oversized file is skipped before any read, which is what keeps the write
// transaction intact.
test("a file past the source-size bound is skipped, never read, and leaves the store usable", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-oversized-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    writeFileSync(join(repo, "src/huge.ts"), `export const pad = "${"x".repeat(MAX_SOURCE_FILE_BYTES + 1)}";\n`);
    const readStable = () => { throw new Error("Cannot create a string longer than 0x1fffffe8 characters"); };
    actor = new RepositoryActor({ root: repo, readStable });
    actor.ingest([{ eventKind: "create", path: "src/huge.ts", observedMs: Date.now() }]);
    assert.equal(await actor.flush(true), 1);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      assert.equal(Object.values(db.prepare("PRAGMA integrity_check").get())[0], "ok");
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=0").get().n, 0);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM files WHERE path=?").get("src/huge.ts").n, 0);
    } finally { closeStore(db); }
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

// Same corruption path, different trigger: watchers report `create` for a new
// directory, so the journal genuinely carries directory paths. existsSync() is
// true for those, and reading one throws EISDIR inside the write transaction.
// Seen live in coderight and heardright once the oversized-file fix let the
// journal drain far enough to reach them.
test("a directory in the journal is skipped, never read, and leaves the store usable", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-eisdir-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    mkdirSync(join(repo, "src/newdir"), { recursive: true });
    const readStable = () => { throw new Error("EISDIR: illegal operation on a directory, read"); };
    actor = new RepositoryActor({ root: repo, readStable });
    actor.ingest([{ eventKind: "create", path: "src/newdir", observedMs: Date.now() }]);
    assert.equal(await actor.flush(true), 1);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      assert.equal(Object.values(db.prepare("PRAGMA integrity_check").get())[0], "ok");
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=0").get().n, 0);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM files WHERE path=?").get("src/newdir").n, 0);
    } finally { closeStore(db); }
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

// Regression: the watcher invokes its callbacks outside any promise chain, so a
// throw inside one is an unhandled exception that kills the SUPERVISOR — all 19
// repos, not the one that failed. Live on 2026-08-03: one repo's "database is
// locked" inside markGap()'s setState killed the process, launchd restarted it,
// the cold sweep began again from the top and died around repo 10 — so the fleet
// sat permanently at 13 noncurrent, never converging, with no crash visible in
// `status` output at all.
test("a throw inside a watcher callback degrades that repo, never the process", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-guard-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    let onEvents = null, onError = null;
    const adapter = {
      startWatch: async (_root, events, error) => { onEvents = events; onError = error; return { unsubscribe() {} }; },
      writeSnapshot: async () => {},
      eventsSince: async () => [],
    };
    actor = new RepositoryActor({ root: repo, adapter });
    await actor.start();
    actor.markGap = () => { throw new Error("database is locked"); };
    actor.ingest = () => { throw new Error("database is locked"); };
    assert.doesNotThrow(() => onError(new Error("watch subscription failed")));
    assert.doesNotThrow(() => onEvents([{ eventKind: "modify", path: "src/service.ts", observedMs: Date.now() }]));
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("failed batch remains pending and resumes on next drain", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-retry-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    actor = new RepositoryActor({ root: repo });
    actor.ingest([{ eventKind: "modify", path: "src/service.ts", observedMs: Date.now() }]);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      db.prepare("UPDATE event_journal SET applied=0 WHERE seq=1").run();
      db.prepare("UPDATE watch_state SET value='0' WHERE key='applied_clock'").run();
    } finally { closeStore(db); }
    assert.equal(await actor.flush(true), 1);
    const reopened = openStore(join(repo, ".agent/graph/graph.db"));
    try { assert.equal(reopened.prepare("SELECT applied FROM event_journal WHERE seq=1").get().applied, 1); }
    finally { closeStore(reopened); }
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("watch subscription failure sets event_gap", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-gap-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    actor = new RepositoryActor({ root: repo, adapter: {
      writeSnapshot: async () => {},
      startWatch: async (_root, _onEvents, onGap) => { onGap(new Error("unsubscribable")); throw new Error("unsubscribable"); },
      eventsSince: async () => [],
    } });
    await assert.rejects(actor.start(), /unsubscribable/);
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try { assert.equal(db.prepare("SELECT value FROM watch_state WHERE key='event_gap'").get().value, "1"); }
    finally { closeStore(db); }
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("watch probes never enter normalized event batches", () => {
  const root = join(tmpdir(), "cortex-watch-probe-normalization");
  const probe = join(root, "cortex-watch-probe-123e4567-e89b-12d3-a456-426614174000");
  const events = normalizeEvents(root, [{ type: "create", path: probe }], Date.now(), [probe]);
  assert.deepEqual(events, []);
});

test("legitimate paths that share a probe prefix remain indexed", () => {
  const root = join(tmpdir(), "cortex-watch-probe-prefix");
  const path = join(root, "cortex-watch-probe-manual.ts");
  const events = normalizeEvents(root, [{ type: "create", path }], Date.now(), []);
  assert.deepEqual(events.map((event) => event.path), ["cortex-watch-probe-manual.ts"]);
});

test("stop waits for an active drain before closing its store", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-stop-drain-"));
  let actor, release, stopping;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    actor = new RepositoryActor({ root: repo });
    actor.openDbOnce();
    actor.drainInFlight = new Promise((_, reject) => actor.stopController.signal.addEventListener("abort", () => reject(Object.assign(new Error("request cancelled"), { code: "request_cancelled" })), { once: true }));
    actor.reconcileInFlight = new Promise((resolve) => { release = resolve; });
    stopping = actor.stop();
    await Promise.resolve();
    assert.ok(actor.db, "store stays open until reconciliation settles");
    release();
    await assert.doesNotReject(stopping);
    assert.equal(actor.db, null);
  } finally { release?.(); if (actor) actor.drainInFlight = null; await stopping?.catch(() => {}); await actor?.stop().catch(() => {}); rmSync(repo, { recursive: true, force: true }); }
});

test("stop aborts an active reconcile before closing its store", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-stop-reconcile-"));
  let actor;
  let release;
  let reconcileSignal;
  let stopping;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const gate = new Promise((resolve) => { release = resolve; });
    actor = new RepositoryActor({
      root: repo,
      reconcile: async (_db, _root, { signal }) => { reconcileSignal = signal; await gate; },
    });
    actor.openDbOnce();
    actor.markGap(new Error("test gap"));
    await new Promise((resolve) => setImmediate(resolve));
    stopping = actor.stop();
    await Promise.resolve();
    assert.equal(reconcileSignal?.aborted, true, "stop aborts the active reconcile");
    assert.ok(actor.db, "store stays open until the active reconcile settles");
    release();
    await stopping;
    assert.equal(actor.db, null);
  } finally {
    release?.();
    await stopping?.catch(() => {});
    await actor?.stop();
    rmSync(repo, { recursive: true, force: true });
  }
});

test("start renews its cancellation controller after stop", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-restart-controller-"));
  let reconcileSignal; cpSync(FIXTURE, repo, { recursive: true });
  const actor = new RepositoryActor({ root: repo, adapter: { startWatch: async () => ({ unsubscribe() {} }), eventsSince: async () => [], writeSnapshot: async () => {} }, reconcile: async (_db, _root, { signal }) => { reconcileSignal = signal; } });
  try { buildGraphGeneration(repo, { outDir: ".agent", persist: true }); await actor.stop(); await actor.start(); assert.equal(reconcileSignal?.aborted, false, "restart receives a fresh controller"); }
  finally { await actor.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("stop waits for startup reconciliation before closing its store", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-stop-startup-"));
  let actor, release, starting, stopping; cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true }); const gate = new Promise((resolve) => { release = resolve; });
    actor = new RepositoryActor({ root: repo, adapter: { startWatch: async () => ({ unsubscribe() {} }), eventsSince: async () => [], writeSnapshot: async () => {} }, reconcile: async () => gate });
    starting = actor.start();
    await new Promise((resolve) => setImmediate(resolve));
    stopping = actor.stop();
    await Promise.resolve();
    assert.ok(actor.db, "store stays open while startup reconciliation is active");
    release(); await Promise.all([starting, stopping]);
    assert.equal(actor.db, null);
  } finally { release?.(); await starting?.catch(() => {}); await stopping?.catch(() => {}); await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("native watcher readiness timeout remains typed", async () => {
  await assert.rejects(
    waitForNativeProbe(new Promise(() => {}), 1),
    (error) => error?.code === "watch_probe_timeout",
  );
});

test("aborted readiness does not report a watcher gap", async () => {
  const controller = new AbortController();
  queueMicrotask(() => controller.abort());
  await assert.rejects(waitForNativeProbe(new Promise(() => {}), 20, controller.signal), (error) => error?.code === "request_cancelled");
});

test("readiness retries UUID probes under one absolute deadline", async () => {
  const root = mkdtempSync(join(tmpdir(), "cortex-probe-retry-"));
  const probes = [], gaps = [];
  let callback;
  try {
    const subscription = await startWatch(root, () => {}, (error) => gaps.push(error), [], {
      probeCadenceMs: 1, probeDeadlineMs: 30,
      subscribe: async (_root, next) => { callback = next; return { unsubscribe: async () => {} }; },
      writeProbe: (path) => { probes.push(path); if (probes.length === 2) callback(null, [{ type: "create", path }]); },
    });
    assert.equal(probes.length, 2);
    assert.notEqual(probes[0], probes[1]);
    assert.deepEqual(gaps, []);
    await subscription.unsubscribe();
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("generated probe paths stay transient through delayed delete callbacks", async () => {
  const root = mkdtempSync(join(tmpdir(), "cortex-probe-delayed-delete-"));
  let callback, probe;
  const events = [];
  try {
    const subscription = await startWatch(root, (batch) => events.push(...batch), () => {}, [], {
      subscribe: async (_root, next) => { callback = next; return { unsubscribe: async () => {} }; },
      writeProbe: (path) => { probe = path; callback(null, [{ type: "create", path }]); },
    });
    callback(null, [{ type: "delete", path: probe }]);
    assert.deepEqual(events, []);
    await subscription.unsubscribe();
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("UUID probe events stay out of snapshot event batches", () => {
  const root = join(tmpdir(), "cortex-watch-probe-snapshot");
  const probe = join(root, "cortex-watch-probe-123e4567-e89b-12d3-a456-426614174000");
  assert.deepEqual(normalizeEvents(root, [{ type: "create", path: probe }], Date.now(), [probe]), []);
});

test("UUID-shaped user paths remain indexed unless they are this run's transient probes", () => {
  const root = join(tmpdir(), "cortex-watch-probe-user-file");
  const path = join(root, "cortex-watch-probe-123e4567-e89b-12d3-a456-426614174000");
  const events = normalizeEvents(root, [{ type: "create", path }]);
  assert.deepEqual(events.map((event) => event.path), ["cortex-watch-probe-123e4567-e89b-12d3-a456-426614174000"]);
});

test("callbacks from a stopped run cannot reopen its store", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-stale-callback-"));
  let actor, onEvents;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    actor = new RepositoryActor({ root: repo, adapter: { startWatch: async (_root, events) => { onEvents = events; return { unsubscribe: async () => {} }; }, eventsSince: async () => [], writeSnapshot: async () => {} } });
    await actor.start();
    await actor.stop();
    onEvents([{ eventKind: "modify", path: "src/service.ts", observedMs: Date.now() }]);
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(actor.db, null);
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("failure retry tears down its run before restart", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-retry-teardown-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    actor = new RepositoryActor({ root: repo });
    const run = actor.beginRun();
    actor.running = true;
    actor.log = () => {};
    const calls = [];
    actor.stop = async () => { calls.push("stop"); actor.epoch += 1; };
    actor.start = async () => { calls.push("start"); };
    actor.handleFailure(new Error("watch failed"), null, run);
    await new Promise((resolve) => setTimeout(resolve, 1050));
    assert.deepEqual(calls, ["stop", "start"]);
  } finally { clearTimeout(actor?.retryTimer); rmSync(repo, { recursive: true, force: true }); }
});

test("explicit stop during retry teardown prevents a stale restart", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-retry-explicit-stop-"));
  let actor, releaseStop;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    actor = new RepositoryActor({ root: repo });
    const run = actor.beginRun();
    actor.running = true;
    actor.log = () => {};
    const stopped = new Promise((resolve) => { releaseStop = resolve; });
    const calls = [];
    actor.stop = async () => { calls.push("retry-stop"); actor.epoch += 1; await stopped; };
    actor.start = async () => { calls.push("start"); };
    actor.handleFailure(new Error("watch failed"), null, run);
    await new Promise((resolve) => setTimeout(resolve, 1050));
    actor.run = null;
    actor.epoch += 1;
    actor.stopGeneration = 1;
    run.controller.abort();
    releaseStop();
    await new Promise((resolve) => setImmediate(resolve));
    assert.deepEqual(calls, ["retry-stop"]);
  } finally { releaseStop?.(); clearTimeout(actor?.retryTimer); rmSync(repo, { recursive: true, force: true }); }
});

test("a new start waits for an in-progress stop to release its store", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-serialized-start-"));
  let actor, releaseUnsubscribe, stopping, restarting;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const unsubscription = new Promise((resolve) => { releaseUnsubscribe = resolve; });
    let starts = 0;
    actor = new RepositoryActor({
      root: repo,
      adapter: {
        startWatch: async () => { starts += 1; return { unsubscribe: async () => unsubscription }; },
        eventsSince: async () => [],
        writeSnapshot: async () => {},
      },
    });
    await actor.start();
    stopping = actor.stop();
    restarting = actor.start();
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(starts, 1, "new start stays queued until old stop has closed its store");
    releaseUnsubscribe();
    await Promise.all([stopping, restarting]);
    assert.equal(starts, 2);
  } finally { releaseUnsubscribe?.(); await stopping?.catch(() => {}); await restarting?.catch(() => {}); await actor?.stop().catch(() => {}); rmSync(repo, { recursive: true, force: true }); }
});

test("stop prevents startup from reconciling after snapshot retrieval settles", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-stop-startup-snapshot-"));
  let actor, releaseEvents, starting, stopping;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    writeFileSync(join(repo, ".agent", "graph", "watch.snapshot"), "snapshot");
    const events = new Promise((resolve) => { releaseEvents = resolve; });
    let reconciles = 0;
    actor = new RepositoryActor({
      root: repo,
      adapter: {
        startWatch: async () => ({ unsubscribe: async () => {} }),
        eventsSince: async () => events,
        writeSnapshot: async () => {},
      },
      reconcile: async () => { reconciles += 1; },
    });
    starting = actor.start();
    await new Promise((resolve) => setImmediate(resolve));
    stopping = actor.stop();
    releaseEvents([]);
    await Promise.all([starting, stopping]);
    assert.equal(reconciles, 0);
  } finally { releaseEvents?.([]); await starting?.catch(() => {}); await stopping?.catch(() => {}); await actor?.stop().catch(() => {}); rmSync(repo, { recursive: true, force: true }); }
});

test("unsubscribe failure still drains work and closes the store", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-unsubscribe-failure-"));
  let actor;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    actor = new RepositoryActor({
      root: repo,
      adapter: {
        startWatch: async () => ({ unsubscribe: async () => { throw new Error("unsubscribe failed"); } }),
        eventsSince: async () => [],
        writeSnapshot: async () => {},
      },
    });
    await actor.start();
    await assert.doesNotReject(actor.stop());
    assert.equal(actor.db, null);
  } finally {
    if (actor?.subscription) actor.subscription = null;
    await actor?.stop().catch(() => {});
    rmSync(repo, { recursive: true, force: true });
  }
});

test("native watcher child timeouts cover production readiness deadline", () => {
  const adapter = readFileSync(join(ROOT, "watchman", "adapter.mjs"), "utf8");
  const deadline = Number(adapter.match(/PROBE_DEADLINE_MS = ([\d_]+)/)?.[1]?.replaceAll("_", ""));
  const source = readFileSync(import.meta.filename, "utf8");
  const timeouts = [...source.matchAll(/timeout: ([\d_]+) \}\);/g)].map((match) => Number(match[1].replaceAll("_", "")));
  assert.ok(timeouts.length >= 3);
  assert.ok(timeouts.every((timeout) => timeout >= deadline), `${JSON.stringify(timeouts)} must cover ${deadline}ms readiness`);
});

test("post-subscribe reconciliation applies an edit from the startup gap", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-startup-gap-"));
  let actor;
  let subscribed = false;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    writeFileSync(join(repo, ".agent", "graph", "watch.snapshot"), "snapshot");
    actor = new RepositoryActor({ root: repo, adapter: {
      writeSnapshot: async () => {},
      startWatch: async () => { subscribed = true; return { unsubscribe() {} }; },
      eventsSince: async () => subscribed ? [{ eventKind: "modify", path: "src/service.ts", observedMs: Date.now() }] : [],
    } });
    await actor.start();
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try { assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE path='src/service.ts' AND applied=1").get().n, 1); }
    finally { closeStore(db); }
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("reconciliation starts only after native watcher readiness", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-ready-reconcile-"));
  let actor;
  let ready = false;
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    actor = new RepositoryActor({
      root: repo,
      adapter: {
        startWatch: async () => { ready = true; return { unsubscribe() {} }; },
        writeSnapshot: async () => {},
        eventsSince: async () => [],
      },
      reconcile: async () => assert.equal(ready, true),
    });
    await actor.start();
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("real Parcel watcher applies an edit within the debounce window", async () => {
  const script = `import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { buildGraphGeneration } from "./graph/static-provider.mjs";
import { RepositoryActor } from "./watchman/repo-actor.mjs";
import { closeStore, openStore } from "./graph/store-sqlite.mjs";
const repo = mkdtempSync(join(tmpdir(), "cortex-watchman-child-"));
let actor;
cpSync(${JSON.stringify(FIXTURE)}, repo, { recursive: true });
try {
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  actor = new RepositoryActor({ root: repo });
  await actor.start();
  const path = join(repo, "src/service.ts");
  writeFileSync(join(repo, "cortex-watch-probe-manual"), "must stay in journal");
  writeFileSync(path, \`${"${readFileSync(path, \"utf8\")}"}\\nexport const liveWatch = true;\\n\`);
  const deadline = Date.now() + 3000;
  let applied = 0;
  let manualRows = 0;
  let manualGraphRows = 0;
  let probeRows = 0;
  while (Date.now() < deadline && applied !== 1) {
    await new Promise((resolve) => setTimeout(resolve, 100));
    const pollingDb = openStore(join(repo, ".agent/graph/graph.db"));
    applied = pollingDb.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE path='src/service.ts' AND applied=1").get().n;
    manualRows = pollingDb.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE path='cortex-watch-probe-manual' AND applied=1").get().n;
    manualGraphRows = pollingDb.prepare("SELECT COUNT(*) AS n FROM files WHERE path='cortex-watch-probe-manual'").get().n;
    probeRows = pollingDb.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE path LIKE 'cortex-watch-probe-%' AND path != 'cortex-watch-probe-manual'").get().n;
    closeStore(pollingDb);
  }
  if (applied !== 1) throw new Error("live watcher applied " + applied + " rows");
  if (manualRows !== 1) throw new Error("probe-prefix file missing from journal: " + manualRows);
  if (manualGraphRows !== 1) throw new Error("probe-prefix file missing from graph: " + manualGraphRows);
  if (probeRows !== 0) throw new Error("readiness probe leaked into journal: " + probeRows);
  console.log("live-ok");
} finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }`;
  const result = spawnSync(process.execPath, ["--input-type=module", "-e", script], { cwd: ROOT, encoding: "utf8", timeout: 35_000 });
  assert.equal(result.status, 0, result.error?.message || result.stderr || result.stdout);
  assert.match(result.stdout, /live-ok/);
});

// The half of that same defect that shipped: a bare excluded NAME only filters
// where it sits directly under the watched root, but real repos nest their
// build output. coderight's Rust output lives at `engine/target`, so `target`
// in the exclusion set excluded nothing at all — 395 of its last 400 unapplied
// events on 2026-08-03 were build churn under `engine/`, and that load starved
// the rest of the fleet's actors. The scanner prunes by name at any depth; the
// watcher must be given the resolved paths to match it.
test("real Parcel watcher never surfaces writes inside a NESTED excluded directory", async () => {
  const script = `import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { startWatch } from "./watchman/adapter.mjs";
const root = mkdtempSync(join(tmpdir(), "cortex-adapter-nested-"));
mkdirSync(join(root, "engine", "target", "release"), { recursive: true });
mkdirSync(join(root, "engine", "src"), { recursive: true });
try {
  await new Promise((resolve) => setTimeout(resolve, 300));
  const events = [];
  const sub = await startWatch(root, (evs) => events.push(...evs), () => {});
  await new Promise((resolve) => setTimeout(resolve, 400));
  writeFileSync(join(root, "engine", "target", "release", "noise.rlib"), "noise");
  writeFileSync(join(root, "engine", "src", "keep.rs"), "keep");
  await new Promise((resolve) => setTimeout(resolve, 800));
  await sub.unsubscribe();
  if (events.some((event) => event.path.includes("target"))) throw new Error("nested target leaked: " + JSON.stringify(events));
  if (!events.some((event) => event.path.endsWith("keep.rs"))) throw new Error("keep.rs was never observed: " + JSON.stringify(events));
  console.log("nested-ok");
} finally { rmSync(root, { recursive: true, force: true }); }`;
  const result = spawnSync(process.execPath, ["--input-type=module", "-e", script], { cwd: ROOT, encoding: "utf8", timeout: 35_000 });
  assert.equal(result.status, 0, result.error?.message || result.stderr || result.stdout);
  assert.match(result.stdout, /nested-ok/);
});

// D1 regression guard: @parcel/watcher's `ignore` option only excludes a
// directory reliably when passed as a literal relative path (its base name
// here — "node_modules" is already in the built-in high-churn ignore set).
// Empirically confirmed 2026-08-03: the glob forms `node_modules/**` and
// `**/node_modules/**` do NOT filter at all in the installed 2.6.0 native
// build — writes inside an "ignored" directory still surface as live events.
// If adapter.mjs's ignorePatterns() is ever "cleaned up" back into globs,
// this must fail.
test("real Parcel watcher never surfaces writes inside a base-ignored directory", async () => {
  const script = `import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { startWatch } from "./watchman/adapter.mjs";
const root = mkdtempSync(join(tmpdir(), "cortex-adapter-ignore-"));
mkdirSync(join(root, "node_modules"), { recursive: true });
try {
  await new Promise((resolve) => setTimeout(resolve, 300));
  const events = [];
  const sub = await startWatch(root, (evs) => events.push(...evs), () => {});
  await new Promise((resolve) => setTimeout(resolve, 400));
  writeFileSync(join(root, "node_modules", "noise.js"), "noise");
  writeFileSync(join(root, "keep.js"), "keep");
  await new Promise((resolve) => setTimeout(resolve, 800));
  await sub.unsubscribe();
  if (events.some((event) => event.path.includes("node_modules"))) throw new Error("node_modules leaked: " + JSON.stringify(events));
  if (!events.some((event) => event.path.endsWith("keep.js"))) throw new Error("keep.js was never observed: " + JSON.stringify(events));
  console.log("ignore-ok");
} finally { rmSync(root, { recursive: true, force: true }); }`;
  const result = spawnSync(process.execPath, ["--input-type=module", "-e", script], { cwd: ROOT, encoding: "utf8", timeout: 35_000 });
  assert.equal(result.status, 0, result.error?.message || result.stderr || result.stdout);
  assert.match(result.stdout, /ignore-ok/);
});
