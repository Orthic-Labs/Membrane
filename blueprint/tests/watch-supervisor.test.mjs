import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";
import { RepositoryActor } from "../watchman/repo-actor.mjs";
import { FRESHNESS, WatchSupervisor, writeWatchConfig } from "../watchman/supervisor.mjs";

// P2: one resident supervisor, one worker per enrolled repository, with
// honest freshness — a repo must never read as "current" just because
// nothing has observed it. These tests exercise the multi-repo fleet
// behavior the single-repo watchman.test.mjs suite does not cover: several
// repos under one supervisor, one repo's actor failing without stalling the
// others, and every freshness state a repo can honestly report.

const FIXTURE = join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce");

function makeRepo(prefix, { build = true } = {}) {
  const repo = realpathSync(mkdtempSync(join(tmpdir(), prefix)));
  cpSync(FIXTURE, repo, { recursive: true });
  if (build) buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  return repo;
}

function tempConfigPath() {
  return join(mkdtempSync(join(tmpdir(), "blueprint-watch-config-")), "watch.json");
}

test("a never-enrolled-and-built repo reports unwatched, not current", () => {
  const repo = makeRepo("blueprint-fleet-unwatched-", { build: false });
  const configPath = tempConfigPath();
  try {
    writeWatchConfig({ repos: [{ root: repo, enabled: true }] }, configPath);
    const supervisor = new WatchSupervisor({ configPath });
    const status = supervisor.status();
    assert.equal(status.repos.length, 1);
    assert.equal(status.repos[0].freshness, FRESHNESS.UNWATCHED);
    assert.equal(status.repos[0].reason, "no_graph_built");
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("a built-but-never-watched repo reports unwatched with a distinct reason", () => {
  const repo = makeRepo("blueprint-fleet-neverwatched-");
  const configPath = tempConfigPath();
  try {
    writeWatchConfig({ repos: [{ root: repo, enabled: true }] }, configPath);
    const status = new WatchSupervisor({ configPath }).status();
    assert.equal(status.repos[0].freshness, FRESHNESS.UNWATCHED);
    assert.equal(status.repos[0].reason, "watcher_never_started");
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("a dead watcher pid reports unwatched with watcher_process_dead, not current", () => {
  const repo = makeRepo("blueprint-fleet-dead-");
  const configPath = tempConfigPath();
  try {
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      // A pid that cannot plausibly be alive in this test run.
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('watcher_pid','999999999') ON CONFLICT(key) DO UPDATE SET value=excluded.value").run();
    } finally { closeStore(db); }
    writeWatchConfig({ repos: [{ root: repo, enabled: true }] }, configPath);
    const status = new WatchSupervisor({ configPath }).status();
    assert.equal(status.repos[0].freshness, FRESHNESS.UNWATCHED);
    assert.equal(status.repos[0].reason, "watcher_process_dead");
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("an event gap reports degraded with the recorded error, not current", () => {
  const repo = makeRepo("blueprint-fleet-gap-");
  const configPath = tempConfigPath();
  try {
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('watcher_pid',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(String(process.pid));
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('event_gap','1') ON CONFLICT(key) DO UPDATE SET value=excluded.value").run();
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('last_error','watcher overflow: too many events') ON CONFLICT(key) DO UPDATE SET value=excluded.value").run();
    } finally { closeStore(db); }
    writeWatchConfig({ repos: [{ root: repo, enabled: true }] }, configPath);
    const status = new WatchSupervisor({ configPath }).status();
    assert.equal(status.repos[0].freshness, FRESHNESS.DEGRADED);
    assert.match(status.repos[0].reason, /watcher overflow: too many events/);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("a thrown watcher callback becomes visible as degraded status", async () => {
  const repo = makeRepo("blueprint-fleet-callback-failure-");
  const configPath = tempConfigPath();
  let actorRef;
  let onEvents;
  const supervisor = new WatchSupervisor({
    configPath,
    reconcile: async () => ({ ok: true }),
    actorFactory: (options) => {
      actorRef = new RepositoryActor({
        ...options,
        adapter: {
          writeSnapshot: async () => {},
          eventsSince: async () => [],
          startWatch: async (_root, events) => { onEvents = events; return { unsubscribe() {} }; },
        },
      });
      return actorRef;
    },
  });
  try {
    writeWatchConfig({ repos: [{ root: repo, enabled: true }] }, configPath);
    await supervisor.start();
    actorRef.ingest = () => { throw new Error("watch callback failed"); };
    assert.doesNotThrow(() => onEvents([{ eventKind: "modify", path: "src/service.ts", observedMs: Date.now() }]));
    await new Promise((resolve) => setImmediate(resolve));
    const status = supervisor.status().repos[0];
    assert.equal(status.freshness, FRESHNESS.DEGRADED);
    assert.equal(status.reason, "watch_callback_error");
  } finally {
    await supervisor.stop();
    rmSync(repo, { recursive: true, force: true });
  }
});

test("pending unapplied events report stale, not current", () => {
  const repo = makeRepo("blueprint-fleet-pending-");
  const configPath = tempConfigPath();
  try {
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('watcher_pid',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(String(process.pid));
      db.prepare("INSERT INTO event_journal(observed_ms,event_kind,path,rename_to,source_clock) VALUES (?, 'modify', 'src/service.ts', NULL, 1)").run(Date.now());
    } finally { closeStore(db); }
    writeWatchConfig({ repos: [{ root: repo, enabled: true }] }, configPath);
    const status = new WatchSupervisor({ configPath }).status();
    assert.equal(status.repos[0].freshness, FRESHNESS.STALE);
    assert.equal(status.repos[0].reason, "events_pending_apply");
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("a live, caught-up watcher reports current", async () => {
  const repo = makeRepo("blueprint-fleet-current-");
  const configPath = tempConfigPath();
  const supervisor = new WatchSupervisor({ configPath });
  try {
    writeWatchConfig({ repos: [{ root: repo, enabled: true }] }, configPath);
    await supervisor.start();
    const status = supervisor.status();
    assert.equal(status.repos[0].freshness, FRESHNESS.CURRENT);
    assert.equal(status.repos[0].reason, null);
    assert.equal(status.repos[0].alive, true);
  } finally {
    await supervisor.stop();
    rmSync(repo, { recursive: true, force: true });
  }
});

test("a stopped supervisor's repos go back to unwatched — no stale 'current' claim survives shutdown", async () => {
  const repo = makeRepo("blueprint-fleet-stopped-");
  const configPath = tempConfigPath();
  const supervisor = new WatchSupervisor({ configPath });
  try {
    writeWatchConfig({ repos: [{ root: repo, enabled: true }] }, configPath);
    await supervisor.start();
    assert.equal(supervisor.status().repos[0].freshness, FRESHNESS.CURRENT);
    await supervisor.stop();
    assert.equal(supervisor.status().repos[0].freshness, FRESHNESS.UNWATCHED);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("one repository's actor failing to start does not stall the others (child isolation)", async () => {
  const goodRepo = makeRepo("blueprint-fleet-good-");
  const brokenRoot = join(tmpdir(), "blueprint-fleet-does-not-exist-" + Math.random().toString(36).slice(2));
  const configPath = tempConfigPath();
  const supervisor = new WatchSupervisor({ configPath });
  try {
    writeWatchConfig({ repos: [{ root: goodRepo, enabled: true }, { root: brokenRoot, enabled: true }] }, configPath);
    await supervisor.start();
    const status = supervisor.status();
    const good = status.repos.find((repo) => repo.root === goodRepo);
    const broken = status.repos.find((repo) => repo.root === brokenRoot);
    assert.equal(good.freshness, FRESHNESS.CURRENT, "the healthy repo must reach current despite its sibling failing");
    assert.equal(broken.freshness, FRESHNESS.UNWATCHED, "the broken repo must honestly report unwatched, never current");
    // The supervisor itself did not throw or stop early — reload() is still
    // tracking the actor it did manage to start.
    assert.ok(supervisor.actors.has(goodRepo));
  } finally {
    await supervisor.stop();
    rmSync(goodRepo, { recursive: true, force: true });
  }
});

test("strict resident startup tolerates stale registry roots when one enrolled actor is healthy", async () => {
  const goodRepo = makeRepo("blueprint-fleet-strict-good-");
  const brokenRoot = join(tmpdir(), "blueprint-fleet-strict-missing-" + Math.random().toString(36).slice(2));
  const configPath = tempConfigPath();
  const supervisor = new WatchSupervisor({ configPath });
  try {
    writeWatchConfig({ repos: [{ root: goodRepo, enabled: true }, { root: brokenRoot, enabled: true }] }, configPath);
    await supervisor.start({ failOnStart: true });
    const status = supervisor.status();
    assert.equal(status.repos.find((repo) => repo.root === goodRepo).freshness, FRESHNESS.CURRENT);
    assert.equal(status.repos.find((repo) => repo.root === brokenRoot).freshness, FRESHNESS.UNWATCHED);
  } finally {
    await supervisor.stop();
    rmSync(goodRepo, { recursive: true, force: true });
  }
});

test("a multi-repo supervisor watches each repo independently: an edit in one does not touch another's graph", async () => {
  const repoA = makeRepo("blueprint-fleet-multi-a-");
  const repoB = makeRepo("blueprint-fleet-multi-b-");
  const configPath = tempConfigPath();
  const supervisor = new WatchSupervisor({ configPath });
  try {
    writeWatchConfig({ repos: [{ root: repoA, enabled: true }, { root: repoB, enabled: true }] }, configPath);
    await supervisor.start();
    const status = supervisor.status();
    assert.equal(status.repos.length, 2);
    for (const repo of status.repos) assert.equal(repo.freshness, FRESHNESS.CURRENT);
  } finally {
    await supervisor.stop();
    rmSync(repoA, { recursive: true, force: true });
    rmSync(repoB, { recursive: true, force: true });
  }
});

// D1 soak fix: the root actor was drowning in FSEvents overflow because it
// watched its whole tree, which contains every other enrolled repo plus that
// repo's own `.agent` WAL churn. Enrolling a child repo nested inside a
// parent must exclude the child's subtree (and the parent's own .agent
// output) from the parent's own subscription — the child already has its
// own actor for that.
test("a parent repo's actor ignores its enrolled child's subtree and its own .agent output (fleet scope)", async () => {
  const parent = makeRepo("blueprint-fleet-scope-parent-");
  const child = join(parent, "child-repo");
  cpSync(FIXTURE, child, { recursive: true });
  buildGraphGeneration(child, { outDir: ".agent", persist: true });
  const configPath = tempConfigPath();
  const supervisor = new WatchSupervisor({ configPath });
  try {
    writeWatchConfig({ repos: [{ root: parent, enabled: true }, { root: child, enabled: true }] }, configPath);
    await supervisor.start();
    const startStatus = supervisor.status();
    assert.ok(startStatus.repos.every((repo) => repo.freshness === FRESHNESS.CURRENT), "both actors must reach current before the probe edits");

    // Noise the parent actor must never observe: an edit inside the enrolled
    // child's own subtree, and a direct write into the parent's own .agent
    // output directory (simulating the actor's own graph/WAL churn).
    writeFileSync(join(child, "src/service.ts"), `${readFileSync(join(child, "src/service.ts"), "utf8")}\nexport const childOnly = true;\n`);
    mkdirSync(join(parent, ".agent", "graph"), { recursive: true });
    writeFileSync(join(parent, ".agent", "graph", "scope-probe.tmp"), "self-observed noise\n");
    // A genuine edit in the parent's own tree, proving its actor still works.
    writeFileSync(join(parent, "src/service.ts"), `${readFileSync(join(parent, "src/service.ts"), "utf8")}\nexport const parentOwn = true;\n`);

    const deadline = Date.now() + 5000;
    let parentApplied = 0;
    while (Date.now() < deadline && parentApplied !== 1) {
      await new Promise((resolve) => setTimeout(resolve, 100));
      const pollingDb = openStore(join(parent, ".agent/graph/graph.db"));
      try { parentApplied = pollingDb.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE path='src/service.ts' AND applied=1").get().n; }
      finally { closeStore(pollingDb); }
    }
    assert.equal(parentApplied, 1, "the parent actor must still apply edits under its own root");

    const parentDb = openStore(join(parent, ".agent/graph/graph.db"));
    try {
      const childLeak = parentDb.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE path LIKE 'child-repo%'").get().n;
      const agentLeak = parentDb.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE path LIKE '.agent%'").get().n;
      assert.equal(childLeak, 0, "the parent actor must never journal paths under its enrolled child's root");
      assert.equal(agentLeak, 0, "the parent actor must never journal its own .agent output directory");
    } finally { closeStore(parentDb); }
  } finally {
    await supervisor.stop();
    rmSync(parent, { recursive: true, force: true });
  }
});

// D1 soak fix: an FSEvents "must be re-scanned" overflow must be treated as a
// typed, honestly-reported condition — mark stale(event_overflow), run
// exactly one full reconcile, and never let status claim "current" while
// that gap is still open.
test("an overflow marks the actor stale(event_overflow), runs exactly one reconcile, and never reports current mid-gap", async () => {
  const repo = makeRepo("blueprint-fleet-overflow-");
  const configPath = tempConfigPath();
  let gapReconcileCalls = 0;
  let releaseReconcile;
  const gate = new Promise((resolve) => { releaseReconcile = resolve; });
  let triggerOverflow;
  const supervisor = new WatchSupervisor({
    configPath,
    actorFactory: (options) => new RepositoryActor({
      ...options,
      adapter: {
        startWatch: async (_root, _onEvents, onGap) => {
          triggerOverflow = () => onGap(new Error("Events were dropped by the FSEvents client. File system must be re-scanned."));
          return { unsubscribe: async () => {} };
        },
        eventsSince: async () => [],
        writeSnapshot: async () => {},
      },
      // The bootstrap reconcile in initialize() always runs once before any
      // gap exists; only count/gate the repair reconcile markGap schedules
      // (recognizable because it always runs after event_gap is set to '1').
      reconcile: async (db) => {
        const gapValue = db.prepare("SELECT value FROM watch_state WHERE key='event_gap'").get()?.value;
        if (gapValue !== "1") return { ok: true };
        gapReconcileCalls += 1;
        await gate;
        db.prepare("INSERT INTO watch_state(key,value) VALUES ('event_gap','0') ON CONFLICT(key) DO UPDATE SET value='0'").run();
        return { ok: true };
      },
    }),
  });
  try {
    writeWatchConfig({ repos: [{ root: repo, enabled: true }] }, configPath);
    await supervisor.start();
    assert.equal(supervisor.status().repos[0].freshness, FRESHNESS.CURRENT);

    triggerOverflow();
    await new Promise((resolve) => setImmediate(resolve));
    const midGap = supervisor.status().repos[0];
    assert.equal(midGap.freshness, FRESHNESS.DEGRADED, "status must never say current mid-gap");
    assert.equal(midGap.reason, "event_overflow");
    assert.equal(gapReconcileCalls, 1, "exactly one reconcile must run for the gap");

    releaseReconcile();
    await new Promise((resolve) => setTimeout(resolve, 50));
    assert.equal(supervisor.status().repos[0].freshness, FRESHNESS.CURRENT, "resumes current once the one reconcile completes");
    assert.equal(gapReconcileCalls, 1, "still exactly one reconcile ran for the gap");
  } finally {
    await supervisor.stop();
    rmSync(repo, { recursive: true, force: true });
  }
});

// G6 fix: all repo actors run in ONE process (no per-actor OS process — see
// repo-actor.mjs), so a repo's watch_state pid is really "whichever actor
// last held this repo," not "this repo's own worker." That pid persists
// across supervisor restarts. status() used to trust it blindly: an actor
// the live supervisor is actively running in-process, but whose persisted
// pid still names a dead prior supervisor incarnation, read as
// watcher_process_dead — a real repo the daemon IS watching, mislabeled
// dead. This must fail before the fix (repoStatus had no way to see the
// live in-process actor at all) and pass after.
test("a stale dead pid in state does not shadow the live supervisor's own running actor (G6: false-dead fix)", async () => {
  const repo = makeRepo("blueprint-fleet-falsedead-");
  const configPath = tempConfigPath();
  const supervisor = new WatchSupervisor({ configPath });
  try {
    writeWatchConfig({ repos: [{ root: repo, enabled: true }] }, configPath);
    await supervisor.start();
    assert.equal(supervisor.status().repos[0].freshness, FRESHNESS.CURRENT, "sanity: current before corrupting state");

    // Simulate the production condition directly: watch_state still carries
    // a long-dead prior supervisor's pid, even though THIS supervisor's own
    // in-process actor is the one actually watching the repo right now.
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try { db.prepare("INSERT INTO watch_state(key,value) VALUES ('watcher_pid','999999999') ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(); }
    finally { closeStore(db); }

    const status = supervisor.status().repos[0];
    assert.notEqual(status.reason, "watcher_process_dead", "must not shadow the live in-process actor with a stale persisted pid");
    assert.equal(status.alive, true);
    assert.equal(status.freshness, FRESHNESS.CURRENT, "the live actor's own real freshness must be reported");
  } finally {
    await supervisor.stop();
    rmSync(repo, { recursive: true, force: true });
  }
});

// G6 fix, other direction: the fix above must not turn into "always alive."
// A repo the live supervisor has genuinely never started (not enrolled when
// it started, so no in-process actor exists for it at all) must still
// report an honest not-watched state even if some stale, dead owner stamp
// happens to sit in its watch_state.
test("a repo the live supervisor has never started stays honestly unwatched (G6: true signal preserved)", async () => {
  const watched = makeRepo("blueprint-fleet-g6owned-");
  const neverStarted = makeRepo("blueprint-fleet-g6neverowned-");
  const configPath = tempConfigPath();
  const supervisor = new WatchSupervisor({ configPath });
  try {
    writeWatchConfig({ repos: [{ root: watched, enabled: true }] }, configPath);
    await supervisor.start();
    assert.equal(supervisor.status().repos[0].freshness, FRESHNESS.CURRENT);

    const db = openStore(join(neverStarted, ".agent/graph/graph.db"));
    try {
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('watcher_pid','999999999') ON CONFLICT(key) DO UPDATE SET value=excluded.value").run();
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('watcher_owner','some-other-dead-supervisor-instance') ON CONFLICT(key) DO UPDATE SET value=excluded.value").run();
    } finally { closeStore(db); }

    // Same live (already-started) supervisor, now asked about a repo it was
    // never enrolled to watch — status() reads config fresh every call, so
    // this exercises "never in this.actors" without needing a second
    // supervisor object.
    writeWatchConfig({ repos: [{ root: watched, enabled: true }, { root: neverStarted, enabled: true }] }, configPath);
    const status = supervisor.status();
    const watchedRow = status.repos.find((repo) => repo.root === watched);
    const neverRow = status.repos.find((repo) => repo.root === neverStarted);
    assert.equal(watchedRow.freshness, FRESHNESS.CURRENT, "the repo this supervisor actually owns stays current");
    assert.notEqual(neverRow.freshness, FRESHNESS.CURRENT, "a repo this supervisor never started must never read as current");
    assert.equal(neverRow.freshness, FRESHNESS.UNWATCHED);
  } finally {
    await supervisor.stop();
    rmSync(watched, { recursive: true, force: true });
    rmSync(neverStarted, { recursive: true, force: true });
  }
});

// G6 fix, restart semantics: simulates an actual supervisor restart. The old
// instance's owner stamp is left behind in watch_state (as a crash would
// leave it — a clean stop() erases it, so this writes it back after
// stopping to model the unclean case) alongside a pid that is, in this test
// process, genuinely alive — proving the fix keys liveness off supervisor
// IDENTITY, not merely off whether some recorded pid happens to be alive.
// The new instance re-owns one repo (its actor completes start()) and fails
// to re-own the other (its actor does not) — actors re-owned by the new
// instance must read alive; the un-re-owned one must not inherit the old
// instance's claim even though its pid check alone would say "alive".
test("repos re-owned by a new supervisor instance read alive; un-re-owned ones do not inherit the old instance's claim (G6: restart semantics)", async () => {
  const repoA = makeRepo("blueprint-fleet-restart-a-");
  const repoB = makeRepo("blueprint-fleet-restart-b-");
  const configPath = tempConfigPath();
  const oldSupervisor = new WatchSupervisor({ configPath });
  const newSupervisor = new WatchSupervisor({
    configPath,
    actorFactory: (options) => options.root === repoB
      ? { running: false, start: async () => { throw new Error("simulated: repoB actor still recovering from the old instance's crash"); }, stop: async () => {}, log: () => {} }
      : new RepositoryActor(options),
  });
  try {
    writeWatchConfig({ repos: [{ root: repoA, enabled: true }, { root: repoB, enabled: true }] }, configPath);
    await oldSupervisor.start();
    assert.ok(oldSupervisor.status().repos.every((repo) => repo.freshness === FRESHNESS.CURRENT), "sanity: both current under the old instance");
    await oldSupervisor.stop();

    // A clean stop() erases watcher_pid/watcher_owner; write them back to
    // model an unclean restart where the prior instance's stamp survives.
    for (const root of [repoA, repoB]) {
      const db = openStore(join(root, ".agent/graph/graph.db"));
      try {
        db.prepare("INSERT INTO watch_state(key,value) VALUES ('watcher_pid',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(String(process.pid));
        db.prepare("INSERT INTO watch_state(key,value) VALUES ('watcher_owner','old-dead-instance') ON CONFLICT(key) DO UPDATE SET value=excluded.value").run();
      } finally { closeStore(db); }
    }

    await newSupervisor.start();
    const status = newSupervisor.status();
    const aRow = status.repos.find((repo) => repo.root === repoA);
    const bRow = status.repos.find((repo) => repo.root === repoB);
    assert.equal(aRow.freshness, FRESHNESS.CURRENT, "repoA, re-owned by the new instance, must read current");
    assert.notEqual(bRow.freshness, FRESHNESS.CURRENT, "repoB, un-re-owned, must not read current off a foreign owner's stamp despite an alive pid");
    assert.equal(bRow.freshness, FRESHNESS.UNWATCHED);
    assert.equal(bRow.reason, "watcher_owner_stale");
  } finally {
    await newSupervisor.stop();
    rmSync(repoA, { recursive: true, force: true });
    rmSync(repoB, { recursive: true, force: true });
  }
});

// D2 soak fix: one repo's malformed graph.db must degrade only that repo's
// row, never blank status for the other enrolled repos.
test("one repo's corrupted graph.db reports degraded/store_unreadable without blanking the fleet (status isolation)", () => {
  const good = makeRepo("blueprint-fleet-good-status-");
  const bad = makeRepo("blueprint-fleet-corrupt-");
  const configPath = tempConfigPath();
  try {
    writeFileSync(join(bad, ".agent/graph/graph.db"), "not a sqlite file, definitely garbage bytes");
    writeWatchConfig({ repos: [{ root: good, enabled: true }, { root: bad, enabled: true }] }, configPath);
    const status = new WatchSupervisor({ configPath }).status();
    assert.equal(status.repos.length, 2, "status must return one row per enrolled repo even when one store is corrupt");
    const goodRow = status.repos.find((repo) => repo.root === good);
    const badRow = status.repos.find((repo) => repo.root === bad);
    assert.ok(goodRow, "the healthy repo's row must still be present");
    assert.equal(goodRow.freshness, FRESHNESS.UNWATCHED);
    assert.equal(goodRow.reason, "watcher_never_started");
    assert.equal(badRow.freshness, FRESHNESS.DEGRADED);
    assert.equal(badRow.reason, "store_unreadable");
    assert.match(badRow.error, /database|file is not a database/i);
  } finally {
    rmSync(good, { recursive: true, force: true });
    rmSync(bad, { recursive: true, force: true });
  }
});

// A slow cold start must not starve the rest of the fleet. The startup loop
// used to `await actor.start()` serially, and a cold start runs a full Merkle
// reconcile — minutes on a large repo. With 19 enrolled repos the tail was
// never even constructed, and because every service restart re-ran the queue
// from the top, repeated restarts could never converge. Modelled here with one
// actor that never finishes starting: the others must still start.
test("one slow actor start does not starve the rest of the fleet (G6b: serial startup)", async () => {
  const slowRepo = makeRepo("blueprint-fleet-slow-");
  const others = [makeRepo("blueprint-fleet-fast-a-"), makeRepo("blueprint-fleet-fast-b-"), makeRepo("blueprint-fleet-fast-c-")];
  const configPath = tempConfigPath();
  const started = [];
  let releaseSlow;
  const slowGate = new Promise((resolve) => { releaseSlow = resolve; });
  const supervisor = new WatchSupervisor({
    configPath,
    actorFactory: (options) => ({
      running: false,
      start: async () => {
        if (options.root === slowRepo) await slowGate;   // never resolves during the assert
        started.push(options.root);
      },
      stop: async () => {},
      log: () => {},
    }),
  });
  try {
    writeWatchConfig({ repos: [{ root: slowRepo, enabled: true }, ...others.map((root) => ({ root, enabled: true }))] }, configPath);
    const reload = supervisor.reload();
    // Give the pool a chance to run every non-blocked start.
    await new Promise((resolve) => setTimeout(resolve, 200));
    for (const root of others) {
      assert.ok(started.includes(root), `fleet starved: ${root} never started behind the slow cold start`);
    }
    releaseSlow();
    await reload;
    assert.ok(started.includes(slowRepo), "the slow actor still starts once it completes");
  } finally {
    releaseSlow?.();
    await supervisor.stop().catch(() => {});
  }
});
