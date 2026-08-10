import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { diagnostics, transition, trayView } from "../apps/cortex-tray/src/state.mjs";
import { serviceControlPlan } from "../service/install.mjs";
import { serviceStatus } from "../service/status.mjs";

const ROOT = join(import.meta.dirname, "..");

test("tray never promotes missing or stale evidence to healthy", () => {
  assert.equal(trayView(null).state, "offline");
  assert.equal(trayView({ state: "stale" }).state, "degraded");
  assert.equal(trayView({ state: "fresh" }).state, "healthy");
});

test("tray lifecycle covers starting and stopping deterministically", () => {
  assert.equal(transition("offline", "start"), "starting");
  assert.equal(transition("healthy", "stop"), "stopping");
  assert.equal(transition("starting", "degraded"), "degraded");
});

test("diagnostics omit service token and authenticated URL", () => {
  const value = diagnostics({ state: "healthy", repository: "repo", token: "secret", url: "http://127.0.0.1/#token=secret" });
  assert.deepEqual(value, { state: "healthy", freshness: null, watcher: null, repository: "repo", repositories: [] });
});

test("tray reports every enrolled repository without promoting watcher health", () => {
  const value = trayView({ state: "fresh", enrolledRepos: [{ root: "one" }, { root: "two" }], watcher: { state: "stopped" } });
  assert.deepEqual(value.repositories, ["one", "two"]);
  assert.equal(value.watcher, "stopped");
});

test("tray consumes Cortex explorer and service commands without token arguments", () => {
  const rust = readFileSync(join(ROOT, "apps", "cortex-tray", "src-tauri", "src", "main.rs"), "utf8");
  assert.match(rust, /\["explore", "--no-open", "--json"/);
  assert.match(rust, /\["service", action, "--json"\]/);
  assert.match(rust, /get\("enrolledRepos"\)/);
  assert.doesNotMatch(rust, /--token|--authorization/);
});

test("service controller maps Windows and macOS without shell interpolation", () => {
  assert.deepEqual(serviceControlPlan("restart", { platform: "win32", target: "ignored" }), [
    { command: "schtasks", args: ["/End", "/TN", "OrthicCortex"] },
    { command: "schtasks", args: ["/Run", "/TN", "OrthicCortex"] },
  ]);
  assert.deepEqual(serviceControlPlan("start", { platform: "darwin", target: "/tmp/cortex.plist" }), [
    { command: "launchctl", args: ["load", "/tmp/cortex.plist"] },
  ]);
});

test("service status derives running state from enrolled watcher evidence", () => {
  const target = join(ROOT, "package.json");
  const status = serviceStatus({ target, fleetStatus: () => ({ repos: [{ root: ROOT, alive: true, pid: process.pid }] }) });
  assert.equal(status.registered, true);
  assert.equal(status.running, true);
  assert.equal(status.pid, process.pid);
  assert.deepEqual(status.enrolledRepos, [{ root: ROOT, enabled: true }]);
});

test("tray autostart is native on Windows and macOS", () => {
  const rust = readFileSync(join(ROOT, "apps", "cortex-tray", "src-tauri", "src", "main.rs"), "utf8");
  assert.match(rust, /OrthicCortexTray/);
  assert.match(rust, /io\.orthic\.cortex-tray\.plist/);
  assert.doesNotMatch(rust, /tauri-plugin-autostart/);
});
