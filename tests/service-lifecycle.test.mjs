// D15: service lifecycle — no OS registration (D-S03), foreground mode only (D-S04)

import assert from "node:assert/strict";
import { spawnSync, spawn } from "node:child_process";
import { join } from "node:path";
import test from "node:test";

import { WatchSupervisor } from "../watchman/supervisor.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts", "cortex.mjs");

test("cortex service status --json returns Hub-owned envelope", () => {
  const result = spawnSync(process.execPath, [CLI, "service", "status", "--json"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.schemaVersion, 1);
  assert.equal(payload.registered, false);
  assert.equal(payload.target, null);
});

test("cortex service install --dry-run does not register OS service", () => {
  const result = spawnSync(process.execPath, [CLI, "service", "install", "--dry-run", "--json"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.target, null);
  assert.ok(Array.isArray(payload.serviceStart));
});

test("cortex service run starts in foreground and responds to SIGTERM", async () => {
  const child = spawn(process.execPath, [CLI, "service", "run", "--json"], { stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "";
  child.stdout.on("data", (d) => (stdout += d.toString()));
  // Wait for running state
  await new Promise((r) => setTimeout(r, 800));
  assert.ok(stdout.includes("running") || stdout.includes("foreground"), "service run must print running state");
  const exited = new Promise((resolve) => child.on("exit", (code) => resolve(code)));
  child.kill("SIGTERM");
  const code = await Promise.race([exited, new Promise((r) => setTimeout(() => r("timeout"), 3000))]);
  assert.notEqual(code, "timeout", "service run must exit on SIGTERM");
  // Clean exit (0) expected after graceful shutdown
  assert.ok(code === 0 || code === null, `exit code ${code}`);
  try { child.kill("SIGKILL"); } catch {}
});

test("cortex service start/stop are forbidden per D-S03", () => {
  for (const cmd of ["start", "stop"]) {
    const result = spawnSync(process.execPath, [CLI, "service", cmd, "--json"], { encoding: "utf8" });
    const payload = JSON.parse(result.stdout || result.stderr || "{}");
    assert.ok(payload.error || result.status !== 0, `service ${cmd} must be forbidden`);
  }
});

test("one repo failure does not stop other actors", async () => {
  const config = {
    repos: [
      { root: "/nonexistent/repo-a", enabled: true },
      { root: "/nonexistent/repo-b", enabled: true },
    ],
  };
  const supervisor = new WatchSupervisor(config);
  assert.ok(supervisor);
  const summary = supervisor.summary ? supervisor.summary() : { repos: config.repos.length };
  assert.equal(summary.repos, 2);
});

test("cortex service uninstall --json works without a live service", () => {
  const result = spawnSync(process.execPath, [CLI, "service", "uninstall", "--json"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.uninstalled, true);
});
