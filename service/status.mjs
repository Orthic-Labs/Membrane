// D15: service status — whether the OS service is registered and the watcher
// process is alive. Every command supports JSON.

import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { serviceTarget } from "./install.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const WATCH_CONFIG = join(homedir(), ".cortex", "watch.json");

function pidAlive(pid) {
  try {
    process.kill(Number(pid), 0);
    return true;
  } catch {
    return false;
  }
}

export function serviceStatus() {
  const registered = existsSync(serviceTarget());
  let pid = null;
  if (existsSync(WATCH_CONFIG)) {
    try {
      const config = JSON.parse(readFileSync(WATCH_CONFIG, "utf8"));
      pid = config.watcher_pid ?? null;
    } catch {
      pid = null;
    }
  }
  return {
    schemaVersion: 1,
    platform: process.platform,
    registered,
    running: registered && pid ? pidAlive(pid) : false,
    pid: pid && pidAlive(pid) ? pid : null,
    target: serviceTarget(),
    enrolledRepos: [],
  };
}
