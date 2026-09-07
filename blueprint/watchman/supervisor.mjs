import { randomUUID } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, relative, resolve } from "node:path";
import { closeStore, getGenerationEnvelope, openStoreReadOnly } from "../src/graph/store-sqlite.mjs";
import { RepositoryActor } from "./repo-actor.mjs";
import { reconcile as defaultReconcile } from "./reconcile.mjs";

// How many cold actor starts may run at once. Serial startup starved the tail
// of a 19-repo fleet; unbounded startup would run 19 full reconciles at once.
export const CONCURRENT_ACTOR_STARTS = 4;
export const CONCURRENT_STARTUP_RECONCILES = 2;

function startupSize(actor) {
  let db;
  try {
    db = openStoreReadOnly(resolve(actor.root, ".agent/graph/graph.db"));
    return Number(db.prepare("SELECT COUNT(*) AS n FROM generation_leaf WHERE kind='file'").get().n);
  } catch { return Number.POSITIVE_INFINITY; }
  finally { if (db) closeStore(db); }
}

export function defaultConfigPath() { return resolve(homedir(), ".blueprint", "watch.json"); }

export function readWatchConfig(configPath = defaultConfigPath()) {
  if (!existsSync(configPath)) return { version: 1, repos: [] };
  const parsed = JSON.parse(readFileSync(configPath, "utf8"));
  if (parsed?.version !== 1 || !Array.isArray(parsed.repos)) throw new Error("watch config must be version 1 with repos[]");
  return { version: 1, repos: parsed.repos.filter((repo) => repo?.enabled !== false && repo.root).map((repo) => ({ root: resolve(repo.root), enabled: true })) };
}

export function writeWatchConfig(config, configPath = defaultConfigPath()) {
  mkdirSync(dirname(configPath), { recursive: true });
  writeFileSync(configPath, `${JSON.stringify({ version: 1, repos: config.repos ?? [] }, null, 2)}\n`);
}

// Honest freshness states. A repository is "current" only when a live watcher
// owns it, no event gap is outstanding, and nothing is waiting to be applied.
// Every other case names itself explicitly — the failure mode this replaces
// is a repo silently reporting "current" because nothing had ever observed
// it (no watcher ever started, or the watcher process has since died).
export const FRESHNESS = Object.freeze({
  UNWATCHED: "unwatched",
  DEGRADED: "degraded",
  STALE: "stale",
  CURRENT: "current",
});

// Reads a repo's state without ever mutating it: a read-only handle skips
// migrate() (never upgrades another process's schema mid-flight) and never
// takes a write lock, so a status poll can never contend with an actor's own
// writes or a concurrent `blueprint build` on the same 19 databases (D3). Errors
// from a corrupt store (e.g. "database disk image is malformed") are caught
// per repo — one bad store must never blank the other 18 repos' status (D2).
//
// `live` carries the CALLING supervisor's own view of this repo (G6 fix):
// `live.actor` is that supervisor's in-process RepositoryActor for this root,
// if any, and `live.instanceId` is that supervisor's own instance token, set
// only once it has itself reload()ed/start()ed at least once. All repo
// actors run in one process (no per-actor OS process — see repo-actor.mjs),
// so `live.actor.running` is ground truth: it cannot lie the way a persisted
// pid can. A persisted `watcher_pid` survives across supervisor restarts
// (a repo's watch_state is only ever touched by whichever actor last held
// it), so after a restart an actor that has not yet re-initialized under the
// new supervisor still carries its dead predecessor's pid — reported
// "watcher_process_dead" even though nothing has actually changed about
// whether the CURRENT supervisor watches it (it simply never did). The one
// case the persisted pid alone gets wrong is the opposite: the live
// supervisor's own in-process actor for THIS root, which the pid check
// cannot see when read from a bare status snapshot taken on that very
// supervisor object. `live.actor?.running` covers exactly that case.
function repoStatus(root, outDir = ".agent", live = {}) {
  const dbPath = resolve(root, outDir, "graph", "graph.db");
  if (!existsSync(dbPath)) {
    return {
      root, pid: null, alive: false, sourceClock: 0, appliedClock: 0, eventGap: 1, pendingEvents: 0,
      freshness: FRESHNESS.UNWATCHED, reason: "no_graph_built",
    };
  }
  let db;
  try {
    db = openStoreReadOnly(dbPath);
    const manifest = getGenerationEnvelope(db).manifest;
    if (!manifest?.complete || !manifest.generationId) {
      return { root, pid: null, alive: false, sourceClock: 0, appliedClock: 0, eventGap: 1, pendingEvents: 0,
        freshness: FRESHNESS.UNWATCHED, reason: "no_graph_built" };
    }
    const state = Object.fromEntries(db.prepare("SELECT key,value FROM watch_state").all().map((row) => [row.key, row.value]));
    const pid = Number(state.watcher_pid ?? 0) || null;
    const persistedOwner = state.watcher_owner ?? null;
    // Ground truth first: the calling supervisor's own actor for this root,
    // if it is currently running, IS being watched right now — no persisted
    // value can contradict that.
    let alive = Boolean(live.actor?.running);
    let ownerStale = false;
    if (!alive && pid) {
      let pidAlive = false;
      try { process.kill(pid, 0); pidAlive = true; } catch {}
      if (pidAlive) {
        // A bare status reader that has never itself reload()ed/start()ed
        // (no `live.instanceId` — e.g. the `blueprint-watch status` CLI, which
        // constructs a fresh WatchSupervisor purely to read state) has no
        // ownership claim of its own and defers entirely to the plain
        // pid-alive signal, exactly as before this fix. Only a supervisor
        // that is itself acting (has an instanceId) and does NOT own this
        // repo's actor additionally distrusts a persisted owner stamp that
        // names a DIFFERENT instance — otherwise a still-alive-but-since-
        // superseded predecessor's pid (or, since actors never fork a
        // per-repo OS process, a coincidentally shared pid across two
        // supervisor incarnations) would be mistaken for a live claim on
        // this repo that isn't actually this run's.
        const foreignOwner = Boolean(live.instanceId) && Boolean(persistedOwner) && persistedOwner !== live.instanceId;
        if (foreignOwner) ownerStale = true; else alive = true;
      }
    }
    const eventGap = Number(state.event_gap ?? 0);
    const pendingEvents = db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=0").get().n;
    const lastError = state.last_error ?? null;
    const gapReason = state.event_gap_reason ?? null;
    let freshness = FRESHNESS.CURRENT;
    let reason = null;
    if (!alive) {
      freshness = FRESHNESS.UNWATCHED;
      reason = ownerStale ? "watcher_owner_stale" : (pid ? "watcher_process_dead" : "watcher_never_started");
    } else if (eventGap) {
      freshness = FRESHNESS.DEGRADED;
      reason = gapReason ?? (lastError ? `event_gap: ${lastError}` : "event_gap_unreconciled");
    } else if (pendingEvents > 0) {
      freshness = FRESHNESS.STALE;
      reason = "events_pending_apply";
    }
    return {
      root, pid, alive, sourceClock: Number(state.source_clock ?? 0), appliedClock: Number(state.applied_clock ?? 0),
      eventGap, pendingEvents, freshness, reason,
    };
  } catch (error) {
    return {
      root, pid: null, alive: false, sourceClock: 0, appliedClock: 0, eventGap: 1, pendingEvents: 0,
      freshness: FRESHNESS.DEGRADED, reason: "store_unreadable", error: String(error?.message ?? error),
    };
  } finally {
    if (db) { try { closeStore(db); } catch { /* already unusable; nothing left to release cleanly */ } }
  }
}

// Exact relative paths (never globs — see watchman/adapter.mjs) from `root` to
// every OTHER enrolled repo nested under it. Used so a parent actor (today,
// principally the workspace-root enrollment) excludes sibling repos' subtrees
// from its own FSEvents subscription instead of double-watching them — every
// child edit was hitting both its own actor and the root actor, and the
// resulting volume (plus each child's own `.agent` WAL churn) is what
// overflowed the root subscription in production.
function siblingIgnoreList(root, repos) {
  return repos
    .map((repo) => repo.root)
    .filter((other) => other !== root)
    .map((other) => relative(root, other))
    .filter((rel) => rel && rel !== ".." && !rel.startsWith(`..${"/"}`) && !isAbsolute(rel));
}

export class WatchSupervisor {
  constructor({ configPath = defaultConfigPath(), actorFactory = (options) => new RepositoryActor(options), reconcile = defaultReconcile, pollMs = 30000 } = {}) {
    this.configPath = resolve(configPath);
    this.actorFactory = actorFactory;
    this.reconcile = reconcile;
    this.pollMs = pollMs;
    this.actors = new Map();
    this.poller = null;
    this.signalHandler = null;
    this.configMtime = 0;
    this.deferReconcile = false;
    this.startupQueue = [];
    this.startupWork = new Set();
    this.startupQueued = new Map();
    this.startupActive = 0;
    this.startupLanes = new Set();
    this.stopping = false;
    // Minted once per supervisor OBJECT, not per OS process: this is what
    // lets status() tell "the supervisor that started this actor" apart from
    // "some supervisor incarnation that once did and may since be gone" even
    // when both share the same OS pid (a real restart gets a fresh pid; a
    // pid can still be reused over a long-lived machine's uptime, and every
    // repo actor already runs in ONE process, so per-actor pids are all
    // identical anyway — see repoStatus() in this file). `hasActed` gates
    // its use: a bare status reader that never itself reload()s/start()s has
    // no ownership claim of its own and must defer entirely to the legacy
    // pid-alive check (G6 fix; see repoStatus()).
    this.instanceId = randomUUID();
    this.hasActed = false;
  }

  async reload({ failOnStart = false, deferReconcile = this.deferReconcile } = {}) {
    this.hasActed = true;
    const configMtime = existsSync(this.configPath) ? statSync(this.configPath).mtimeMs : 0;
    const config = readWatchConfig(this.configPath);
    const wanted = new Map(config.repos.map((repo) => [repo.root, repo]));
    for (const [root, actor] of this.actors) {
      if (!wanted.has(root)) { await actor.stop(); this.actors.delete(root); }
    }
    // Construct every wanted actor FIRST, then start them in a bounded pool.
    // This loop used to `await actor.start()` serially, and a cold actor's
    // start() runs a full Merkle reconcile — minutes on a large repo. With 19
    // enrolled repos the tail never got constructed at all (owner unset, no
    // watchman.log, reported watcher_process_dead off a stale pid), and every
    // service restart began the queue again from the top, so repeated restarts
    // could never converge. Bounded rather than unbounded: a cold start on a
    // big repo peaks a few hundred MB, and 19 at once is what melts the host.
    const pendingStarts = [];
    for (const repo of config.repos) {
      if (this.actors.has(repo.root)) {
        const existing = this.actors.get(repo.root);
        if (!existing.running && !existing.retryTimer) pendingStarts.push(existing);
        continue;
      }
      let actor;
      actor = this.actorFactory({ root: repo.root, reconcile: this.reconcile, ignore: siblingIgnoreList(repo.root, config.repos), ownerId: this.instanceId,
        restart: async () => {
          await actor.start({ deferReconcile });
          if (deferReconcile) this.queueStartup(actor);
        },
      });
      this.actors.set(repo.root, actor);
      pendingStarts.push(actor);
    }
    const failures = [];
    let next = 0;
    await Promise.all(
      Array.from({ length: Math.min(CONCURRENT_ACTOR_STARTS, pendingStarts.length) }, async () => {
        while (next < pendingStarts.length) {
          const actor = pendingStarts[next++];
          try { await actor.start({ deferReconcile }); }
          catch (error) {
            actor.log(error);
            failures.push(error);
          }
        }
      }),
    );
    // Admit every native subscription before any cold scan. Heavy startup
    // reconciliation uses a bounded FIFO pool, so one slow root does not
    // prevent healthy peers from becoming current.
    if (deferReconcile) {
      for (const actor of pendingStarts) {
        if (!actor.running || typeof actor.resumeStartup !== "function") continue;
        this.queueStartup(actor, { deferPump: true });
      }
      this.pumpStartup();
    }
    // Resident Hub startup must remain available when registry contains an
    // old/missing root: healthy enrolled actors still form one watcher, while
    // each failed root stays visible as an honestly unwatched row. Fail only
    // when strict startup found no actor able to run at all, preserving typed
    // watcher-startup failure for a fully broken registry/installation.
    if (failOnStart && failures.length && ![...this.actors.values()].some((actor) => actor.running)) {
      throw failures[0];
    }
    // Startup can await slow actors. Never acknowledge a config revision
    // written during that wait until its roots have actually been admitted.
    this.configMtime = configMtime;
    return this.status();
  }

  queueStartup(actor, { deferPump = false } = {}) {
    if (this.stopping) return;
    const epoch = actor.epoch;
    const epochs = this.startupQueued.get(actor) ?? new Set();
    if (epochs.has(epoch)) return;
    epochs.add(epoch);
    this.startupQueued.set(actor, epochs);
    let complete;
    const work = new Promise((resolve) => { complete = resolve; });
    this.startupWork.add(work);
    this.startupQueue.push({ actor, epoch, size: startupSize(actor), complete: () => {
      epochs.delete(epoch);
      if (!epochs.size) this.startupQueued.delete(actor);
      this.startupWork.delete(work);
      complete();
    } });
    if (!deferPump) this.pumpStartup();
  }

  get startupTail() { return Promise.all([...this.startupWork]); }

  pumpStartup() {
    while (!this.stopping && this.startupActive < CONCURRENT_STARTUP_RECONCILES && this.startupQueue.length) {
      const lane = this.startupLanes.has(0) ? 1 : 0;
      // One FIFO lane guarantees large-root progress; the other admits small
      // roots before queued bulk scans. Never preempt an active reconcile.
      let index = 0;
      if (lane === 1) {
        for (let i = 1; i < this.startupQueue.length; i += 1) {
          if (this.startupQueue[i].size < this.startupQueue[index].size) index = i;
        }
      }
      const { actor, epoch, complete } = this.startupQueue.splice(index, 1)[0];
      this.startupLanes.add(lane);
      this.startupActive += 1;
      new Promise((resolve) => setImmediate(resolve))
        .then(() => this.stopping ? undefined : actor.resumeStartup(epoch))
        .catch((error) => {
          try { actor.log(error); }
          catch (logError) {
            process.stderr.write(`${JSON.stringify({ event: "watcher_startup_log_failed", root: actor.root,
              error: String(error?.message ?? error), logError: String(logError?.message ?? logError) })}\n`);
          }
        })
        .finally(() => {
          this.startupActive -= 1;
          this.startupLanes.delete(lane);
          complete();
          this.pumpStartup();
        });
    }
  }

  async start(options = {}) {
    this.stopping = false;
    this.deferReconcile = options.deferReconcile === true;
    // Existing in-process supervisors may tolerate one repo failure and keep
    // serving healthy peers; resident blueprint-watch opts into strict
    // readiness explicitly with { failOnStart: true }.
    await this.reload({ failOnStart: false, ...options });
    this.signalHandler = () => this.reload().catch(() => {});
    process.on("SIGHUP", this.signalHandler);
    this.poller = setInterval(() => {
      const mtime = existsSync(this.configPath) ? statSync(this.configPath).mtimeMs : 0;
      if (mtime !== this.configMtime) this.reload().catch(() => {});
    }, this.pollMs);
    return this.status();
  }

  status() {
    const config = readWatchConfig(this.configPath);
    return {
      version: 1,
      repos: config.repos.map((repo) => repoStatus(repo.root, ".agent", {
        actor: this.actors.get(repo.root),
        instanceId: this.hasActed ? this.instanceId : null,
      })),
    };
  }

  async stop() {
    this.stopping = true;
    for (const entry of this.startupQueue.splice(0)) entry.complete();
    clearInterval(this.poller);
    if (this.signalHandler) process.off("SIGHUP", this.signalHandler);
    for (const actor of this.actors.values()) await actor.stop();
    await this.startupTail;
    this.actors.clear();
  }
}
