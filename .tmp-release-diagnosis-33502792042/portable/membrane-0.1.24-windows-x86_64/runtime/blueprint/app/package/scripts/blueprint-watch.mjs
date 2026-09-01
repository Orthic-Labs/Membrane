#!/usr/bin/env node
import { existsSync, readFileSync, unlinkSync, writeFileSync, openSync, closeSync, mkdirSync, writeSync } from "node:fs";
import { homedir } from "node:os";
import { timingSafeEqual } from "node:crypto";
import { dirname, resolve, join } from "node:path";
import { spawn } from "node:child_process";
import { WatchSupervisor, defaultConfigPath, readWatchConfig, writeWatchConfig } from "../watchman/supervisor.mjs";
import { reconcile } from "../watchman/reconcile.mjs";
import { syncToCurrentSourceAtPath } from "../src/graph/barrier.mjs";

const configPath = defaultConfigPath();
const pidPath = join(dirname(configPath), "watchman.pid");
const command = process.argv[2] ?? "status";
const args = process.argv.slice(3);
const WATCHER_PARENT_PID_ENV = "MEMBRANE_BLUEPRINT_PARENT_PID";
const WATCHER_LAUNCH_TOKEN_ENV = "MEMBRANE_BLUEPRINT_LAUNCH_TOKEN";
const WATCHER_HANDSHAKE_TIMEOUT_MS = 2000;

function json(value) { console.log(JSON.stringify(value, null, 2)); }

function pidAlive(pid) {
  try { process.kill(pid, 0); return true; } catch { return false; }
}

function launchTokenMatches(received, expected) {
  const left = Buffer.from(String(received).trim());
  const right = Buffer.from(String(expected));
  return left.length === right.length && timingSafeEqual(left, right);
}

function readHubWatcherToken(expected) {
  return new Promise((resolve) => {
    const stdin = process.stdin;
    let buffer = "";
    let settled = false;
    let timer;
    const cleanup = () => {
      clearTimeout(timer);
      stdin.off("data", onData);
      stdin.off("end", onEnd);
      stdin.off("error", onError);
    };
    const finish = (ok) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(ok);
    };
    const onData = (chunk) => {
      buffer += chunk.toString();
      const newline = buffer.indexOf("\n");
      if (newline < 0) return;
      finish(launchTokenMatches(buffer.slice(0, newline), expected));
    };
    const onEnd = () => finish(false);
    const onError = () => finish(false);
    stdin.setEncoding("utf8");
    stdin.on("data", onData);
    stdin.once("end", onEnd);
    stdin.once("error", onError);
    stdin.resume();
    timer = setTimeout(() => finish(false), WATCHER_HANDSHAKE_TIMEOUT_MS);
  });
}

async function authorizeHubWatcher() {
  if (process.env.MEMBRANE_HUB_CHILD !== "1" || process.env.BLUEPRINT_SERVICE_CHILD !== "1") return false;
  const parentPid = Number(process.env[WATCHER_PARENT_PID_ENV]);
  const expected = process.env[WATCHER_LAUNCH_TOKEN_ENV];
  if (!Number.isSafeInteger(parentPid) || parentPid <= 0 || parentPid !== process.ppid) return false;
  if (!/^[0-9a-f]{64}$/.test(expected ?? "")) return false;
  return readHubWatcherToken(expected);
}

function claimPidfile() {
  mkdirSync(dirname(pidPath), { recursive: true });
  try {
    const fd = openSync(pidPath, "wx");
    writeSync(fd, String(process.pid));
    closeSync(fd);
    return true;
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
    const existingPid = Number(readFileSync(pidPath, "utf8"));
    if (pidAlive(existingPid)) return false;
    try { unlinkSync(pidPath); } catch {}
    try {
      const fd = openSync(pidPath, "wx");
      writeSync(fd, String(process.pid));
      closeSync(fd);
      return true;
    } catch (retryError) {
      if (retryError.code === "EEXIST") return false;
      throw retryError;
    }
  }
}

function enroll(root) {
  const absolute = resolve(root ?? process.cwd());
  const config = readWatchConfig(configPath);
  if (!config.repos.some((repo) => repo.root === absolute)) config.repos.push({ root: absolute, enabled: true });
  writeWatchConfig(config, configPath);
  json({ enrolled: absolute, config: configPath });
}

function unenroll(root) {
  const absolute = resolve(root ?? process.cwd());
  const config = readWatchConfig(configPath);
  config.repos = config.repos.filter((repo) => repo.root !== absolute);
  writeWatchConfig(config, configPath);
  json({ unenrolled: absolute, config: configPath });
}

async function start() {
  if (!(await authorizeHubWatcher())) {
    json({ schemaVersion: 1, started: false, reason: "hub_inactive", error: { code: "hub_inactive", message: "Blueprint watcher residency requires an active Membrane Hub" } });
    process.exitCode = 2;
    return;
  }
  if (args.includes("--daemon")) {
    const child = spawn(process.execPath, [new URL(import.meta.url).pathname, "start"], {
      detached: true,
      stdio: "ignore",
      windowsHide: true,
    });
    child.unref();
    // The child claims the pidfile itself and may decline as already_running,
    // so this process only knows it spawned — reporting "started" would be a
    // claim the parent cannot verify.
    json({ spawned: true, daemon: true, pid: child.pid });
    return;
  }
  if (!claimPidfile()) {
    json({ started: false, reason: "already_running" });
    return;
  }
  const supervisor = new WatchSupervisor({ configPath });
  let stopping = false;
  const removePidfile = () => { try { unlinkSync(pidPath); } catch {} };
  const stop = async (exitCode = 0) => {
    if (stopping) return;
    stopping = true;
    try { await supervisor.stop(); }
    catch (error) { console.error(error.stack ?? error); exitCode ||= 1; }
    finally { removePidfile(); process.exit(exitCode); }
  };
  process.once("SIGINT", () => { void stop(0); });
  process.once("SIGTERM", () => { void stop(0); });
  if (process.env.BLUEPRINT_SERVICE_CHILD === "1") {
    process.stdin.resume();
    process.stdin.once("end", () => { void stop(0); });
    process.stdin.once("close", () => { void stop(0); });
    process.stdin.once("error", () => { void stop(0); });
  }
  try {
    // Initial actor startup is strict: a resident service is not ready when
    // any enrolled actor failed. Cold reconcile may take minutes, while the
    // Hub-owned parent publishes its own bounded running envelope promptly.
    await supervisor.start({ failOnStart: true });
    if (!stopping) json(supervisor.status());
  } catch (error) {
    console.error(error.stack ?? error);
    await stop(1);
  }
}

async function stop() {
  if (!existsSync(pidPath)) return json({ stopped: false, reason: "not_running" });
  const pid = Number(readFileSync(pidPath, "utf8"));
  try { process.kill(pid, "SIGTERM"); } catch {}
  try { unlinkSync(pidPath); } catch {}
  json({ stopped: true, pid });
}

async function barrierAll() {
  const repos = readWatchConfig(configPath).repos;
  const receipts = await Promise.all(repos.map(async ({ root }) => {
    try {
      const receipt = await syncToCurrentSourceAtPath(root, { outDir: ".agent", timeoutMs: 2000 });
      return { repoRoot: receipt.repoRoot, receipt };
    } catch (error) {
      return { repoRoot: root, receipt: { receiptId: null, repoRoot: root, barrierResult: "error", error: String(error?.message ?? error) } };
    }
  }));
  return json({ schemaVersion: 1, receipts });
}

async function main() {
  if (command === "enroll") return enroll(args[0]);
  if (command === "unenroll") return unenroll(args[0]);
  if (command === "status") return json(new WatchSupervisor({ configPath }).status());
  if (command === "barrier-all") return barrierAll();
  if (command === "nudge") return json(await reconcile(resolve(args[0] ?? process.cwd())));
  if (command === "logs") {
    const lines = Number(args[args.indexOf("-n") + 1] ?? 50);
    const repos = readWatchConfig(configPath).repos;
    return repos.forEach((repo) => {
      const path = join(repo.root, ".agent", "graph", "watchman.log");
      if (existsSync(path)) console.log(readFileSync(path, "utf8").trim().split("\n").slice(-lines).join("\n"));
    });
  }
  if (command === "start") return start();
  if (command === "stop") return stop();
  throw new Error(`unknown blueprint-watch command: ${command}`);
}

main().catch((error) => { console.error(error.stack ?? error); process.exitCode = 1; });
