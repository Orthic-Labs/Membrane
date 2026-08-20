import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { checkScopeGrant, issueScopeGrant } from "../src/lib/receipt-store.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/cortex.mjs");
const INSTALLER = join(ROOT, "scripts/cortex-install.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function run(repo, args, input) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, input, encoding: "utf8" });
}
function install(repo, args) {
  return spawnSync(process.execPath, [INSTALLER, ...args], { cwd: repo, encoding: "utf8" });
}
function makeRepo() {
  const repo = mkdtempSync(join(tmpdir(), "cortex-grant-"));
  cpSync(FIXTURE, repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  return repo;
}

test("grant issue/check enforces expiry and rejects one-byte tampering", () => {
  const repo = makeRepo();
  try {
    const issued = run(repo, ["grant", "issue", "--task", "task-1", "--paths", "src/**", "docs/**", "--ttl-minutes", "120"]);
    assert.equal(issued.status, 0, issued.stderr);
    const grant = JSON.parse(issued.stdout);
    assert.deepEqual(Object.keys(grant).sort(), ["generationId", "issuedMs", "paths", "receiptId", "repoRoot", "signature", "taskId", "ttlMs"].sort());
    assert.equal(grant.taskId, "task-1");
    assert.deepEqual(grant.paths, ["docs/**", "src/**"]);
    assert.equal(run(repo, ["grant", "check", "--task", "task-1", "--path", "src/service.ts"]).status, 0);
    assert.equal(run(repo, ["grant", "check", "--task", "task-1", "--path", "private.txt"]).status, 4);

    const file = join(repo, ".agent", "graph", "grants", readdirSync(join(repo, ".agent", "graph", "grants"))[0]);
    const tampered = readFileSync(file, "utf8").replace("src/**", "src/x*");
    writeFileSync(file, tampered, "utf8");
    assert.equal(run(repo, ["grant", "check", "--task", "task-1", "--path", "src/service.ts"]).status, 4);

    const expired = issueScopeGrant({ repoRoot: repo, generationId: grant.generationId, taskId: "expired", paths: ["src/**"], ttlMinutes: 1 / 60000, issuedMs: Date.now() - 2000 });
    assert.equal(checkScopeGrant({ repoRoot: repo, generationId: grant.generationId, taskId: "expired", path: "src/service.ts" }).allowed, false);
    assert.ok(expired.path);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("grant redirect hook denies before expansion and allows after grant issuance", () => {
  const repo = makeRepo();
  try {
    assert.equal(spawnSync("git", ["init", "-q"], { cwd: repo }).status, 0);
    const installed = install(repo, ["--host", "claude-code", "--project", "--redirect=grants"]);
    assert.equal(installed.status, 0, installed.stderr);
    const settings = JSON.parse(readFileSync(join(repo, ".claude", "settings.json"), "utf8"));
    assert.equal(settings.hooks.PreToolUse.at(-1).matcher, "^(Read|Edit)$");
    assert.match(settings.hooks.PreToolUse.at(-1).hooks[0].command, /--grant-check/);
    const input = JSON.stringify({ task_id: "task-2", tool_input: { path: "src/service.ts" } });
    const denied = spawnSync(process.execPath, [INSTALLER, "--grant-check", "--root", repo], { cwd: repo, input, encoding: "utf8" });
    assert.equal(JSON.parse(denied.stdout).hookSpecificOutput.permissionDecision, "deny");
    const expanded = run(repo, ["grant", "issue", "--task", "task-2", "--paths", "src/**", "--ttl-minutes", "120"]);
    assert.equal(expanded.status, 0, expanded.stderr);
    const allowed = spawnSync(process.execPath, [INSTALLER, "--grant-check", "--root", repo], { cwd: repo, input, encoding: "utf8" });
    assert.equal(JSON.parse(allowed.stdout).hookSpecificOutput.permissionDecision, "allow");
  } finally { rmSync(repo, { recursive: true, force: true }); }
});
