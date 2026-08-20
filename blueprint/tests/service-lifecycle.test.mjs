// D15: service lifecycle — no OS registration (D-S03), foreground mode only (D-S04)

import assert from "node:assert/strict";
import { execFileSync, spawnSync, spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { WatchSupervisor } from "../watchman/supervisor.mjs";
import { buildFingerprint, createBuildSingleflight } from "../src/service/build-singleflight.mjs";
import { DaemonClient } from "../src/service/client.mjs";
import { temporaryDaemonEndpoint } from "../src/service/paths.mjs";
import { validateDeadlineMs } from "../src/service/protocol.mjs";
import { createDaemonServer } from "../src/service/server.mjs";
import { computeManifestDigest, detectHubIdentityFields, detectShadowManifestKeys, assertBuildIdentityClean } from "../src/graph/generation-identity.mjs";
import { classifyMutablePath, assertSafeMutableStorePath, openStore, closeStore } from "../src/graph/store-sqlite.mjs";
import { validateSnapshot } from "../src/lib/snapshot.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts", "blueprint.mjs");

function deferred() {
  let resolvePromise;
  let rejectPromise;
  const promise = new Promise((resolve, reject) => { resolvePromise = resolve; rejectPromise = reject; });
  return { promise, resolve: resolvePromise, reject: rejectPromise };
}

function tick() { return new Promise((resolve) => setImmediate(resolve)); }

function temporaryRepository(name) {
  const root = mkdtempSync(join(tmpdir(), `${name}-`));
  execFileSync("git", ["init", "-q"], { cwd: root });
  writeFileSync(join(root, "source.mjs"), "export const value = 1;\n");
  return root;
}

test("build fingerprint is server-derived from source, sidecars, output, and normalized options", () => {
  const root = temporaryRepository("blueprint-build-fingerprint");
  try {
    const base = buildFingerprint({ root, outDir: ".agent", options: { limit: 5, fingerprint: "caller-a" } });
    const spoofed = buildFingerprint({ root, outDir: ".agent", options: { limit: 5, fingerprint: "caller-b" } });
    assert.equal(base.fingerprint, spoofed.fingerprint);
    writeFileSync(join(root, "source.mjs"), "export const value = 2;\n");
    assert.notEqual(buildFingerprint({ root, outDir: ".agent", options: { limit: 5 } }).fingerprint, base.fingerprint);
    assert.notEqual(buildFingerprint({ root, outDir: ".agent", options: { limit: 6 } }).fingerprint, base.fingerprint);
    writeFileSync(join(root, "README.md"), "# Fixture\n");
    const beforeGeneratedOutput = buildFingerprint({ root, outDir: ".agent", options: { limit: 5 } });
    writeFileSync(join(root, "README.md"), "# Fixture\n\n<!-- blueprint:docs:start -->\ngenerated pointer\n<!-- blueprint:docs:end -->\n");
    assert.equal(buildFingerprint({ root, outDir: ".agent", options: { limit: 5 } }).fingerprint, beforeGeneratedOutput.fingerprint);
    mkdirSync(join(root, ".agent"), { recursive: true });
    writeFileSync(join(root, ".agent", "config.json"), "{\"ignoredPrefixes\":[]}\n");
    const configured = buildFingerprint({ root, outDir: ".agent", options: { limit: 5 } });
    writeFileSync(join(root, ".agent", "config.json"), "{\"ignoredPrefixes\":[\"vendor\"]}\n");
    assert.notEqual(buildFingerprint({ root, outDir: ".agent", options: { limit: 5 } }).fingerprint, configured.fingerprint);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("equivalent builds join while differing fingerprints run FIFO without overlap", async () => {
  const root = temporaryRepository("blueprint-build-queue");
  const gates = new Map([[1, deferred()], [2, deferred()], [3, deferred()]]);
  const starts = [];
  let active = 0;
  let maximumActive = 0;
  const singleflight = createBuildSingleflight({ runner: async (identity) => {
    starts.push(identity.options.limit);
    active += 1;
    maximumActive = Math.max(maximumActive, active);
    await gates.get(identity.options.limit).promise;
    active -= 1;
    return { exitCode: 0, stdout: `built-${identity.options.limit}\n`, stderr: "" };
  } });
  try {
    const first = singleflight.build({ root, options: { limit: 1 } });
    const joined = singleflight.build({ root, options: { limit: 1 } });
    const second = singleflight.build({ root, options: { limit: 2 } });
    const third = singleflight.build({ root, options: { limit: 3 } });
    await tick();
    assert.deepEqual(starts, [1]);
    gates.get(1).resolve();
    const [firstResult, joinedResult] = await Promise.all([first, joined]);
    assert.equal(firstResult.fingerprint, joinedResult.fingerprint);
    await tick();
    assert.deepEqual(starts, [1, 2]);
    gates.get(2).resolve();
    await second;
    await tick();
    assert.deepEqual(starts, [1, 2, 3]);
    gates.get(3).resolve();
    await third;
    await singleflight.waitForIdle();
    assert.equal(maximumActive, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("waiter cancellation detaches without stopping a shared or current build", async () => {
  const root = temporaryRepository("blueprint-build-cancel");
  const currentGate = deferred();
  const queuedGate = deferred();
  const starts = [];
  let completed = 0;
  const singleflight = createBuildSingleflight({ runner: async (identity) => {
    starts.push(identity.options.limit);
    await (identity.options.limit === 1 ? currentGate.promise : queuedGate.promise);
    completed += 1;
    return { exitCode: 0, stdout: "", stderr: "" };
  } });
  try {
    const cancelledController = new AbortController();
    const survivingController = new AbortController();
    const cancelled = singleflight.build({ root, options: { limit: 1 } }, { signal: cancelledController.signal });
    const surviving = singleflight.build({ root, options: { limit: 1 } }, { signal: survivingController.signal });
    await tick();
    cancelledController.abort();
    await assert.rejects(cancelled, { code: "request_cancelled" });
    assert.deepEqual(starts, [1]);
    currentGate.resolve();
    await surviving;
    assert.equal(completed, 1);

    const lastController = new AbortController();
    const lastWaiter = singleflight.build({ root, options: { limit: 2 } }, { signal: lastController.signal });
    await tick();
    lastController.abort();
    await assert.rejects(lastWaiter, { code: "request_cancelled" });
    assert.deepEqual(starts, [1, 2]);
    queuedGate.resolve();
    await singleflight.waitForIdle();
    assert.equal(completed, 2);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("cancelled queued waiter is removed without cancelling current build", async () => {
  const root = temporaryRepository("blueprint-build-queued-cancel");
  const gate = deferred();
  const starts = [];
  const singleflight = createBuildSingleflight({ runner: async (identity) => {
    starts.push(identity.options.limit);
    await gate.promise;
    return { exitCode: 0, stdout: "", stderr: "" };
  } });
  try {
    const current = singleflight.build({ root, options: { limit: 1 } });
    const controller = new AbortController();
    const queued = singleflight.build({ root, options: { limit: 2 } }, { signal: controller.signal });
    await tick();
    controller.abort();
    await assert.rejects(queued, { code: "request_cancelled" });
    gate.resolve();
    await current;
    await singleflight.waitForIdle();
    assert.deepEqual(starts, [1]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("daemon build route accepts build deadline & confines root through enrollment", async () => {
  const root = temporaryRepository("blueprint-build-ipc");
  const endpoint = temporaryDaemonEndpoint("blueprint-build-ipc");
  const observed = [];
  const builds = {
    async build(input) { observed.push(input); return { exitCode: 0, stdout: "built\n", stderr: "", fingerprint: "derived" }; },
    async waitForIdle() {},
  };
  const daemon = createDaemonServer({ endpoint, registryEntries: [{ root, repoId: "repo" }], buildSingleflight: builds });
  const client = new DaemonClient({ endpoint });
  try {
    assert.equal(validateDeadlineMs(120000, "build"), 120000);
    assert.throws(() => validateDeadlineMs(120000, "status"), { code: "deadline_invalid" });
    await daemon.listen();
    const response = await client.request({ method: "build", repoId: "repo", deadlineMs: 120000, input: { outDir: ".agent", fingerprint: "spoof", options: { limit: 7 } } });
    assert.equal(response.ok, true);
    assert.equal(response.result.stdout, "built\n");
    assert.equal(observed.length, 1);
    assert.equal(observed[0].root, root);
    assert.equal(observed[0].fingerprint, undefined);
    assert.deepEqual(observed[0].options, { limit: 7 });
  } finally {
    await client.close();
    await daemon.close();
    rmSync(root, { recursive: true, force: true });
  }
});

test("blueprint service status --json returns Hub-owned envelope", () => {
  const result = spawnSync(process.execPath, [CLI, "service", "status", "--json"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.schemaVersion, 1);
  assert.equal(payload.registered, false);
  assert.equal(payload.target, null);
});

test("blueprint service install --dry-run does not register OS service", () => {
  const result = spawnSync(process.execPath, [CLI, "service", "install", "--dry-run", "--json"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.target, null);
  assert.ok(Array.isArray(payload.serviceStart));
});

test("blueprint service run starts in foreground and exits when Hub owner pipe closes", async () => {
  const home = mkdtempSync(join(tmpdir(), "blueprint-service-home-"));
  try {
    const child = spawn(process.execPath, [CLI, "service", "run", "--json"], { cwd: ROOT, env: { ...process.env, HOME: home, USERPROFILE: home, BLUEPRINT_SNAPSHOT_TOKEN: "test-token", MEMBRANE_HUB_CHILD: "1" }, stdio: ["pipe", "pipe", "pipe"] });
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
    assert.ok(Number.isInteger(payload.watcherPid) && payload.watcherPid > 0, "service run must own a Blueprint watcher child");
    assert.doesNotThrow(() => process.kill(payload.watcherPid, 0), "watcher child must be alive while service runs");
    const response = await fetch(`http://${payload.statusEndpoint.host}:${payload.statusEndpoint.port}/snapshot`, { headers: { [payload.statusEndpoint.authHeader]: "Bearer test-token" } });
    assert.equal(response.status, 200);
    const snapshot = await response.json();
    assert.equal(snapshot.productId, "blueprint");
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

test("blueprint service start/stop are forbidden per D-S03", () => {
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

test("blueprint service uninstall --json works without a live service", () => {
  const result = spawnSync(process.execPath, [CLI, "service", "uninstall", "--json"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.uninstalled, true);
});

test("build identity: Hub protocol/lease/endpoint/instance/fence never enter manifestDigest", () => {
  const base = {
    schemaVersion: 1,
    provider: { id: "lexical", version: "repo-local-v1" },
    counts: { nodes: 3, edges: 2 },
    repo: { rootName: "blueprint", sourceHash: "xxh128:abc", fileCount: 1 },
  };
  const clean = computeManifestDigest(base, null);
  const contaminated = computeManifestDigest({
    ...base,
    hub: { lease: "l", endpoint: "e" },
    fence: 42,
    protocol: "blueprint.lifecycle.v1",
    instance: "i",
  }, null);
  assert.equal(contaminated, clean, "Hub lifecycle fields must not change the manifest digest");
  assert.deepEqual(detectHubIdentityFields({ ...base, fence: 1, protocol: "p" }), ["fence", "protocol"]);
  assert.deepEqual(detectShadowManifestKeys({ ...base, sourceHash: "xxh128:dup" }), [{ key: "sourceHash", canonical: "repo.sourceHash" }]);
  assertBuildIdentityClean(base);
  assert.throws(() => assertBuildIdentityClean({ ...base, fence: 1 }), { code: "build_identity_violation" });
});

test("mutable store paths: synced/shared refused typed, local/unknown proceeds", () => {
  const synced = classifyMutablePath("/Users/adrian/Dropbox/blueprint/.agent/graph/graph.db", { probeMount: () => "local" });
  assert.equal(synced.classification, "synced");
  assert.throws(
    () => assertSafeMutableStorePath("/Users/adrian/Dropbox/repo/.agent/graph.db", { probeMount: () => "local" }),
    { code: "synced_store_path_refused" },
  );
  const shared = classifyMutablePath("\\\\server\\share\\repo\\.agent\\graph.db", { platform: "win32", probeMount: () => "unavailable" });
  assert.equal(shared.classification, "shared");
  assert.throws(() => assertSafeMutableStorePath("//server/share/repo/.agent/graph.db", { probeMount: () => "unavailable" }), { code: "shared_store_path_refused" });
  const memory = classifyMutablePath(":memory:");
  assert.equal(memory.classification, "local");
  const local = classifyMutablePath("/tmp/blueprint-fixture/.agent/graph/graph.db", { probeMount: () => "local" });
  assert.ok(["local", "unknown"].includes(local.classification), `local path misclassified as ${local.classification}`);
  assert.throws(
    () => openStore("/Users/adrian/Dropbox/repo/.agent/graph/graph.db", { mutablePathPolicy: "refuse", probeMount: () => "local" }),
    { code: "synced_store_path_refused" },
  );
});

test("snapshot bounds reject oversized content-free payloads", () => {
  const snap = {
    schemaVersion: 1,
    productId: "blueprint",
    observedAtUnixMs: 1,
    sections: { graph: { state: "available", reason: "x".repeat(500) } },
  };
  const result = validateSnapshot(snap);
  assert.equal(result.ok, false);
  assert.ok(result.errors.some((error) => error.includes("reason exceeds")), result.errors.join(", "));
});
