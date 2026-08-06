import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { DatabaseSync } from "node:sqlite";
import { isPidAlive, readCortexReadiness, READINESS_STATES } from "./cortex-readiness.mjs";

const LIVE_PID = process.pid; // This process is alive for the whole run.
const DEAD_PID = 999999999;   // No such pid.

async function fixtureGraph({ pid, owner, eventGap, gapReason, lastError, sourceClock = 7, appliedClock = 7, pending = 0 } = {}) {
  const root = await mkdtemp(join(tmpdir(), "membrane-readiness-"));
  await mkdir(join(root, ".agent", "graph"), { recursive: true });
  const dbPath = join(root, ".agent", "graph", "graph.db");
  const db = new DatabaseSync(dbPath);
  db.exec("CREATE TABLE watch_state (key TEXT PRIMARY KEY, value TEXT)");
  db.exec("CREATE TABLE event_journal (id INTEGER PRIMARY KEY, applied INTEGER)");
  const set = db.prepare("INSERT INTO watch_state (key, value) VALUES (?, ?)");
  if (pid !== undefined && pid !== null) set.run("watcher_pid", String(pid));
  if (owner !== undefined && owner !== null) set.run("watcher_owner", String(owner));
  if (eventGap !== undefined) set.run("event_gap", String(eventGap));
  if (gapReason !== undefined && gapReason !== null) set.run("event_gap_reason", String(gapReason));
  if (lastError !== undefined && lastError !== null) set.run("last_error", String(lastError));
  set.run("source_clock", String(sourceClock));
  set.run("applied_clock", String(appliedClock));
  const ins = db.prepare("INSERT INTO event_journal (applied) VALUES (0)");
  for (let i = 0; i < pending; i++) ins.run();
  db.close();
  return { root, cleanup: () => rm(root, { recursive: true, force: true }) };
}

test("readiness surfaces exactly the four authoritative freshness states", () => {
  assert.deepEqual([...READINESS_STATES].sort(), ["current", "degraded", "stale", "unwatched"]);
});

test("no graph.db is unwatched with no_graph_built", async () => {
  const root = await mkdtemp(join(tmpdir(), "membrane-readiness-nograph-"));
  const readiness = readCortexReadiness(root);
  assert.equal(readiness.freshness, "unwatched");
  assert.equal(readiness.reason, "no_graph_built");
  assert.equal(readiness.alive, false);
  await rm(root, { recursive: true, force: true });
});

test("no persisted pid is unwatched with watcher_never_started", async () => {
  const { root, cleanup } = await fixtureGraph({ pid: null, owner: null, eventGap: 0 });
  const readiness = readCortexReadiness(root);
  assert.equal(readiness.freshness, "unwatched");
  assert.equal(readiness.reason, "watcher_never_started");
  assert.equal(readiness.alive, false);
  await cleanup();
});

test("a dead persisted pid is a stale ACTOR (unwatched, watcher_process_dead), not stale data", async () => {
  const { root, cleanup } = await fixtureGraph({ pid: DEAD_PID, owner: "sup-1", eventGap: 0 });
  const readiness = readCortexReadiness(root);
  // Acceptance: a dead watcher is a stale actor → unwatched, NEVER degraded.
  assert.equal(readiness.freshness, "unwatched");
  assert.equal(readiness.reason, "watcher_process_dead");
  assert.equal(readiness.alive, false);
  await cleanup();
});

test("a live pid owned by a foreign supervisor instance is a stale ACTOR (watcher_owner_stale)", async () => {
  const { root, cleanup } = await fixtureGraph({ pid: LIVE_PID, owner: "sup-old", eventGap: 0 });
  const readiness = readCortexReadiness(root, { ownInstanceId: "sup-new" });
  // Supervisor restart: persisted owner names a superseded instance. This is a
  // stale actor (unwatched), not current and not degraded.
  assert.equal(readiness.freshness, "unwatched");
  assert.equal(readiness.reason, "watcher_owner_stale");
  assert.equal(readiness.alive, false);
  await cleanup();
});

test("a live pid owned by the SAME supervisor instance with no gap is current", async () => {
  const { root, cleanup } = await fixtureGraph({ pid: LIVE_PID, owner: "sup-new", eventGap: 0 });
  const readiness = readCortexReadiness(root, { ownInstanceId: "sup-new" });
  assert.equal(readiness.freshness, "current");
  assert.equal(readiness.reason, null);
  assert.equal(readiness.alive, true);
  await cleanup();
});

test("a bare reader trusts a live pid with no gap as current", async () => {
  const { root, cleanup } = await fixtureGraph({ pid: LIVE_PID, owner: "sup-any", eventGap: 0 });
  const readiness = readCortexReadiness(root);
  assert.equal(readiness.freshness, "current");
  assert.equal(readiness.alive, true);
  await cleanup();
});

test("a live actor behind on an event gap is stale DATA (degraded), distinct from a stale actor", async () => {
  const { root, cleanup } = await fixtureGraph({ pid: LIVE_PID, owner: "sup-1", eventGap: 1, gapReason: "watchman_overflow" });
  const readiness = readCortexReadiness(root);
  // Live actor + event gap → degraded (stale data), never unwatched.
  assert.equal(readiness.freshness, "degraded");
  assert.equal(readiness.reason, "watchman_overflow");
  assert.equal(readiness.alive, true);
  await cleanup();
});

test("a live actor with pending unapplied events is stale (events_pending_apply)", async () => {
  const { root, cleanup } = await fixtureGraph({ pid: LIVE_PID, owner: "sup-1", eventGap: 0, pending: 3 });
  const readiness = readCortexReadiness(root);
  assert.equal(readiness.freshness, "stale");
  assert.equal(readiness.reason, "events_pending_apply");
  assert.equal(readiness.alive, true);
  assert.equal(readiness.pendingEvents, 3);
  await cleanup();
});

test("isPidAlive distinguishes the running process from a dead pid", () => {
  assert.equal(isPidAlive(LIVE_PID), true);
  assert.equal(isPidAlive(DEAD_PID), false);
  assert.equal(isPidAlive(null), false);
  assert.equal(isPidAlive(0), false);
});
