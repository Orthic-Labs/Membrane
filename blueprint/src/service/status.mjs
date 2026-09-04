// D15: service status — Hub-owned lifecycle (D-S03), no OS registration.
// Reports watcher liveness via fleet status; registered is always false.

import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { closeStore, openStoreReadOnly } from "../graph/store-sqlite.mjs";

function pidAlive(pid) {
  try { process.kill(Number(pid), 0); return true; } catch { return false; }
}

function residentWatcherPid() {
  try {
    return Number(readFileSync(join(homedir(), ".blueprint", "watchman.pid"), "utf8").trim()) || null;
  } catch {
    return null;
  }
}

function readFleetStatus(target = null, configPath = join(homedir(), ".blueprint", "watch.json")) {
  if (!existsSync(configPath)) return { repos: [] };
  const config = JSON.parse(readFileSync(configPath, "utf8"));
  const targetRoot = target ? resolve(target) : null;
  const repos = [];
  for (const item of config.repos ?? []) {
    if (!item?.root || item.enabled === false) continue;
    const root = resolve(item.root);
    if (targetRoot && root !== targetRoot) continue;
    let db;
    let pid = null;
    try {
      db = openStoreReadOnly(join(root, ".agent", "graph", "graph.db"));
      pid = Number(db.prepare("SELECT value FROM watch_state WHERE key='watcher_pid'").get()?.value ?? 0) || null;
    } catch {
      // A live actor may hold the graph writer while status is requested.
      // The resident pidfile still gives us bounded liveness evidence without
      // treating an enrolled repository as absent.
      pid = residentWatcherPid();
    } finally { if (db) closeStore(db); }
    repos.push({ root, pid, alive: Boolean(pid && pidAlive(pid)) });
  }
  return { repos };
}

export function serviceStatus({ target = null, fleetStatus = readFleetStatus } = {}) {
  // An unreadable fleet status reported an empty repo list, which reads
  // exactly like "nothing is enrolled". Keep the failure so a caller can tell
  // the two apart.
  let fleet = { repos: [] };
  let fleetError = null;
  try {
    fleet = fleetStatus(target);
  } catch (error) {
    fleetError = String(error?.message ?? error);
  }
  const enrolledRepos = (fleet.repos ?? []).map((repo) => ({ root: repo.root, enabled: true }));
  const active = (fleet.repos ?? []).find((repo) => repo.alive);
  const targetRepo = target ? (fleet.repos ?? [])[0] ?? null : null;
  return {
    schemaVersion: 1,
    ...(fleetError ? { fleetError } : {}),
    platform: process.platform,
    registered: false, // D-S03: no OS registration
    running: Boolean(active),
    serviceLive: Boolean(active),
    hubAvailable: fleetError === null,
    targetEnrolled: target ? Boolean(targetRepo) : false,
    targetWatcherLive: target ? Boolean(targetRepo?.alive) : false,
    pid: active?.pid ?? null,
    target: target ? resolve(target) : null,
    enrolledRepos,
    foreground: "blueprint service run",
  };
}
