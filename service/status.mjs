// D15: service status — whether the OS service is registered and the watcher
// process is alive. Every command supports JSON.

import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { closeStore, openStoreReadOnly } from "../graph/store-sqlite.mjs";
import { serviceTarget } from "./install.mjs";

function pidAlive(pid) {
  try { process.kill(Number(pid), 0); return true; } catch { return false; }
}

function readFleetStatus(configPath = join(homedir(), ".cortex", "watch.json")) {
  if (!existsSync(configPath)) return { repos: [] };
  const config = JSON.parse(readFileSync(configPath, "utf8"));
  const repos = [];
  for (const item of config.repos ?? []) {
    if (!item?.root || item.enabled === false) continue;
    const root = resolve(item.root);
    let db;
    let pid = null;
    try {
      db = openStoreReadOnly(join(root, ".agent", "graph", "graph.db"));
      pid = Number(db.prepare("SELECT value FROM watch_state WHERE key='watcher_pid'").get()?.value ?? 0) || null;
    } catch {} finally { if (db) closeStore(db); }
    repos.push({ root, pid, alive: Boolean(pid && pidAlive(pid)) });
  }
  return { repos };
}

export function serviceStatus({ target = serviceTarget(), fleetStatus = readFleetStatus } = {}) {
  const registered = existsSync(target);
  let fleet = { repos: [] };
  try { fleet = fleetStatus(); } catch {}
  const enrolledRepos = (fleet.repos ?? []).map((repo) => ({ root: repo.root, enabled: true }));
  const active = (fleet.repos ?? []).find((repo) => repo.alive);
  return {
    schemaVersion: 1,
    platform: process.platform,
    registered,
    running: registered && Boolean(active),
    pid: registered ? active?.pid ?? null : null,
    target,
    enrolledRepos,
  };
}
