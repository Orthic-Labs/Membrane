// D08: apply → uninstall round-trip restores original host files byte-for-byte
// for Claude Code, Codex, Cursor, and generic hosts.

import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, symlinkSync } from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { buildInitPlan } from "../lib/init/plan.mjs";
import { applyInitPlan } from "../lib/init/apply.mjs";
import { removeBlock } from "../scripts/cortex-install.mjs";

const CORTEX = fileURLToPath(new URL("../scripts/cortex.mjs", import.meta.url));

function uninstall(root) {
  const result = spawnSync(process.execPath, [CORTEX, "uninstall", "--root", root, "--json"], { cwd: root, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  return JSON.parse(result.stdout);
}

function makeRepo(host) {
  const root = mkdtempSync(join(tmpdir(), `cortex-roundtrip-${host}-`));
  mkdirSync(join(root, ".git"), { recursive: true });
  if (host === "claude-code") writeFileSync(join(root, "CLAUDE.md"), "# Existing Claude instructions\n");
  if (host === "codex") writeFileSync(join(root, "AGENTS.md"), "# Existing Codex instructions\n");
  if (host === "cursor") {
    mkdirSync(join(root, ".cursor", "rules"), { recursive: true });
    writeFileSync(join(root, ".cursor", "rules", "cortex.mdc"), "# Existing Cursor rules\n");
  }
  if (host === "generic") writeFileSync(join(root, "CORTEX-AGENT.md"), "# Existing generic instructions\n");
  return root;
}

function instructionPathFor(root, host) {
  if (host === "claude-code") return join(root, "CLAUDE.md");
  if (host === "codex") return join(root, "AGENTS.md");
  if (host === "cursor") return join(root, ".cursor", "rules", "cortex.mdc");
  return join(root, "CORTEX-AGENT.md");
}

for (const host of ["claude-code", "codex", "cursor", "generic"]) {
  test(`${host} apply→uninstall restores files byte-for-byte`, () => {
    const root = makeRepo(host);
    try {
      const instruction = instructionPathFor(root, host);
      const original = readFileSync(instruction, "utf8");
      const plan = buildInitPlan({ root, host, scope: "project", mcp: "off", watch: "off", hooks: "none", build: false });
      const applied = applyInitPlan({ root, plan, build: false });
      assert.equal(applied.ok, true, applied.error ?? "");
      const afterApply = readFileSync(instruction, "utf8");
      assert.notEqual(afterApply, original, "instruction file should be modified");
      assert.ok(afterApply.includes("cortex_orient"));
      const restored = uninstall(root);
      assert.equal(restored.ok, true);
      assert.equal(readFileSync(instruction, "utf8"), original, "byte-for-byte restore");
      assert.equal(uninstall(root).idempotent, true, "second uninstall is typed and idempotent");
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

test("applyInitPlan writes the managed instruction block", () => {
  const root = makeRepo("generic");
  try {
    const plan = buildInitPlan({ root, host: "generic", scope: "project", mcp: "off", watch: "off", hooks: "none" });
    const applied = applyInitPlan({ root, plan, build: false });
    assert.equal(applied.ok, true, applied.error ?? "");
    const content = readFileSync(join(root, "CORTEX-AGENT.md"), "utf8");
    assert.ok(content.includes("<!-- cortex:start -->"));
    assert.ok(content.includes("<!-- cortex:end -->"));
    assert.ok(content.includes("cortex_orient"));
    // removeBlock must strip exactly the managed block.
    const stripped = removeBlock(content);
    assert.ok(!stripped.includes("cortex_orient"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("CLI uninstall restores non-UTF8 host bytes", () => {
  const root = makeRepo("generic");
  try {
    const instruction = join(root, "CORTEX-AGENT.md");
    const original = Buffer.from([0, 255, 10, 128, 65]);
    writeFileSync(instruction, original);
    const applied = applyInitPlan({ root, plan: buildInitPlan({ root, host: "generic", mcp: "off", watch: "off" }), build: false });
    assert.equal(applied.ok, true, applied.error ?? "");
    assert.notDeepEqual(readFileSync(instruction), original);
    assert.equal(uninstall(root).idempotent, false);
    assert.deepEqual(readFileSync(instruction), original);
    assert.equal(uninstall(root).idempotent, true);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("init state validator rejects corrupt and escaping restore plans", async () => {
  const module = await import("../lib/init/apply.mjs");
  assert.equal(typeof module.validateInstallState, "function");
  if (typeof module.validateInstallState !== "function") return;
  const root = makeRepo("generic");
  try {
    assert.throws(() => module.validateInstallState(root, { version: 1, files: { [join(root, "..", "escape")]: { exists: false } } }), /state_invalid/);
    assert.throws(() => module.validateInstallState(root, { version: 1, files: { [root]: { exists: false } } }), /state_invalid/);
    assert.throws(() => module.validateInstallState(root, { version: 1, files: { [join(root, "corrupt")]: { exists: true, bytes: "*" } } }), /state_invalid/);
    assert.throws(() => module.validateInstallState(root, { version: 1, files: { [join(root, "random.txt")]: { exists: false, installed: "a".repeat(64) } } }), /state_invalid/);
    assert.throws(() => module.validateInstallState(root, { version: 1, files: { [join(root, "CORTEX-AGENT.md")]: { exists: false } } }), /state_invalid/);
    assert.doesNotThrow(() => module.validateInstallState(root, { version: 1, files: { [join(root, "CORTEX-AGENT.md")]: { exists: true, bytes: "", content: "", installed: "a".repeat(64) } } }));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("uninstall rejects a symlinked state parent", (t) => {
  const root = makeRepo("generic"), outside = mkdtempSync(join(tmpdir(), "cortex-state-outside-"));
  try {
    mkdirSync(join(outside, "graph")); writeFileSync(join(outside, "graph", "cortex-install-state.json"), '{"version":1,"files":{}}');
    try { symlinkSync(outside, join(root, ".agent"), process.platform === "win32" ? "junction" : "dir"); } catch { t.skip("symlink privilege unavailable"); return; }
    const result = spawnSync(process.execPath, [CORTEX, "uninstall", "--root", root, "--json"], { encoding: "utf8" });
    assert.notEqual(result.status, 0); assert.ok(readFileSync(join(outside, "graph", "cortex-install-state.json"), "utf8"));
  } finally { rmSync(root, { recursive: true, force: true }); rmSync(outside, { recursive: true, force: true }); }
});

test("uninstall preserves host edits made after init", () => {
  const root = makeRepo("generic");
  try {
    const file = join(root, "CORTEX-AGENT.md");
    assert.equal(applyInitPlan({ root, plan: buildInitPlan({ root, host: "generic", mcp: "off", watch: "off" }), build: false }).ok, true);
    writeFileSync(file, "# user edit\n");
    const result = spawnSync(process.execPath, [CORTEX, "uninstall", "--root", root, "--json"], { encoding: "utf8" });
    assert.notEqual(result.status, 0); assert.equal(readFileSync(file, "utf8"), "# user edit\n");
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("install state integrity rejects repo-side snapshot tampering", async () => {
  const root = makeRepo("generic"), keyDir = mkdtempSync(join(tmpdir(), "cortex-state-keys-"));
  try {
    const integrity = await import("../lib/init/state-integrity.mjs");
    assert.equal(typeof integrity.sealInstallState, "function");
    const state = { version: 1, files: { [join(root, "CORTEX-AGENT.md")]: { exists: false, bytes: null, installed: "a".repeat(64) } } };
    const sealed = integrity.sealInstallState(root, state, { stateKeyDir: keyDir });
    assert.doesNotThrow(() => integrity.verifyInstallState(root, sealed, { stateKeyDir: keyDir }));
    sealed.files[join(root, "CORTEX-AGENT.md")].exists = true;
    assert.throws(() => integrity.verifyInstallState(root, sealed, { stateKeyDir: keyDir }), /state_integrity_invalid/);
  } finally { rmSync(root, { recursive: true, force: true }); rmSync(keyDir, { recursive: true, force: true }); }
});

test("init apply preserves corrupt state and its external key", async () => {
  const root = makeRepo("generic"), integrity = await import("../lib/init/state-integrity.mjs"), path = join(root, ".agent", "graph", "cortex-install-state.json");
  try {
    mkdirSync(join(root, ".agent", "graph"), { recursive: true });
    const sealed = integrity.sealInstallState(root, { version: 1, files: {} }), corrupt = { ...sealed, integrity: { ...sealed.integrity, tag: "0".repeat(64) } };
    writeFileSync(path, JSON.stringify(corrupt));
    assert.equal(applyInitPlan({ root, plan: buildInitPlan({ root, host: "generic", mcp: "off", watch: "off" }), build: false }).ok, false);
    assert.deepEqual(JSON.parse(readFileSync(path, "utf8")), corrupt);
    assert.doesNotThrow(() => integrity.verifyInstallState(root, sealed));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

async function aliveAfter(command, args, ms) {
  const child = spawn(command, args, { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
  let exited = false;
  let code = null;
  child.on("exit", (c) => { exited = true; code = c; });
  try {
    await new Promise((resolve) => setTimeout(resolve, ms));
    return { alive: !exited, code };
  } finally {
    if (!exited) {
      child.kill();
      child.removeAllListeners("exit");
      child.unref();
    }
  }
}

test("init writes an MCP config that actually launches", async () => {
  const root = makeRepo("generic");
  try {
    const plan = buildInitPlan({ root, host: "generic", scope: "project", mcp: "on", watch: "off", hooks: "none", build: false });
    const applied = applyInitPlan({ root, plan, build: false });
    assert.equal(applied.ok, true, applied.error ?? "");
    const cfg = JSON.parse(readFileSync(join(root, ".mcp.json"), "utf8"));
    const entry = cfg.mcpServers.cortex;
    assert.ok(entry, ".mcp.json must record a cortex MCP server");
    const { alive, code } = await aliveAfter(entry.command, entry.args, 1500);
    assert.ok(alive, `MCP server exited early with code ${code}; args were ${JSON.stringify(entry.args)}`);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
