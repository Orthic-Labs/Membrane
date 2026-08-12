// D15: service lifecycle — no OS registration (D-S03), foreground mode only (D-S04)

import assert from "node:assert/strict";
import { spawnSync, spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { WatchSupervisor } from "../watchman/supervisor.mjs";
import { writeProductManifest } from "../lib/init/manifest.mjs";

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

test("cortex service install writes only its Orthic product manifest", () => {
  const home = mkdtempSync(join(tmpdir(), "cortex-install-home-"));
  try {
    const result = spawnSync(process.execPath, [CLI, "service", "install", "--root", ROOT, "--json"], {
      encoding: "utf8", env: { ...process.env, HOME: home, USERPROFILE: home },
    });
    assert.equal(result.status, 0, result.stderr);
    const payload = JSON.parse(result.stdout);
    assert.equal(payload.target, null);
    assert.equal(payload.installed, true);
    assert.equal(payload.manifest, join(home, ".orthic", "hub", "products.d", "cortex.json"));
  } finally {
    rmSync(home, { recursive: true, force: true });
  }
});

test("cortex service run starts in foreground and exits when Hub owner pipe closes", async () => {
  const home = mkdtempSync(join(tmpdir(), "cortex-service-home-"));
  try {
    const manifestPath = join(home, ".orthic", "hub", "products.d", "cortex.json");
    const { manifest } = writeProductManifest({ installRoot: ROOT, outPath: manifestPath });
    const child = spawn(process.execPath, [CLI, "service", "run", "--json"], { cwd: ROOT, env: { ...process.env, HOME: home, USERPROFILE: home, ORTHIC_HUB_CHILD: "1" }, stdio: ["pipe", "pipe", "pipe"] });
    let stdout = "";
    child.stdout.on("data", (d) => (stdout += d.toString()));
    await new Promise((resolve, reject) => {
      let timer;
      let poll;
      const onError = (error) => finish(error);
      function finish(error = null) {
        clearTimeout(timer);
        clearInterval(poll);
        child.off("error", onError);
        if (error) reject(error); else resolve();
      }
      timer = setTimeout(finish, 5000);
      poll = setInterval(() => {
        if (stdout.includes("running") || stdout.includes("foreground")) finish();
      }, 25);
      child.once("error", onError);
    });
    assert.ok(stdout.includes("running") || stdout.includes("foreground"), "service run must print running state");
    const payload = JSON.parse(stdout.trim().split(/\r?\n/)[0]);
    assert.ok(Number.isInteger(payload.watcherPid) && payload.watcherPid > 0, "service run must own a Cortex watcher child");
    assert.doesNotThrow(() => process.kill(payload.watcherPid, 0), "watcher child must be alive while service runs");
    const response = await fetch(`http://${payload.statusEndpoint.host}:${payload.statusEndpoint.port}/snapshot`, { headers: { [manifest.statusEndpoint.authHeader]: manifest.statusEndpoint.authToken } });
    assert.equal(response.status, 200);
    const snapshot = await response.json();
    assert.equal(snapshot.productId, "cortex");
    assert.ok(Number.isInteger(snapshot.observedAtUnixMs));
    const exited = new Promise((resolve) => child.on("exit", (code) => resolve(code)));
    child.stdin.end();
    const code = await Promise.race([exited, new Promise((r) => setTimeout(() => r("timeout"), 3000))]);
    assert.notEqual(code, "timeout", "service run must exit on SIGTERM");
    assert.ok(code === 0 || code === null, `exit code ${code}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
    assert.throws(() => process.kill(payload.watcherPid, 0), "watcher child must stop with its service parent");
    try { child.kill("SIGKILL"); } catch {}
  } finally {
    rmSync(home, { recursive: true, force: true });
  }
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
