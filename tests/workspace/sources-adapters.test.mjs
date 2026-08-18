import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync, cpSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  produceCandidates,
  produceCandidatesWithTelemetry,
  taskAnchor,
  dirtyFiles,
  graphResolve,
  gitMetadata,
  rulesDocuments,
  liveOverlay,
} from "../../src/sources/index.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
import os from "node:os";
import path from "node:path";
import { seedStore } from "../_store-helpers.mjs";
import { findWorkspaceRoot } from "../_workspace-root.mjs";

const ROOT = findWorkspaceRoot(HERE, { required: false });
const CORTEX = path.resolve(HERE, "../..");
const SCHEMA = ROOT ? path.join(ROOT, "tools/lib/context-contracts.schema.json") : null;
const workspaceSkip = ROOT ? false : "requires parent monorepo context contracts";
import { validateJsonSchema } from "../python-test-runtime.mjs";

function validateCandidate(candidate) {
  const result = validateJsonSchema(JSON.parse(readFileSync(SCHEMA, "utf8")), candidate, "#/$defs/Candidate");
  return { status: result.valid ? 0 : 1, errors: result.errors };
}

function validateCandidates(candidates) {
  for (const c of candidates) {
    const result = validateCandidate(c);
    assert.equal(result.status, 0, `Candidate ${c.id} failed schema: ${result.stderr || JSON.stringify(result.errors)}`);
  }
}

function withTempRepo(label, fn) {
  // os.tmpdir() honours TMPDIR/TEMP/TMP per platform. The previous
  // `process.env.TEMP || "/tmp"` only ever worked on Windows: TEMP is unset on
  // macOS and Linux, so every run fell through to a hardcoded /tmp — which a
  // sandboxed or read-only-/tmp environment refuses, failing the test for
  // reasons that have nothing to do with the code under test.
  const tmp = path.join(os.tmpdir(), `sources-${label}-${process.pid}-${Date.now()}`);
  mkdirSync(tmp, { recursive: true });
  try {
    fn(tmp);
  } finally {
    try { rmSync(tmp, { recursive: true, force: true }); } catch { /* best-effort */ }
  }
}

// ---- task-anchor ----

test("task-anchor: extracts paths and ranges from the task string", { skip: workspaceSkip }, () => {
  withTempRepo("anchor", (repo) => {
    mkdirSync(path.join(repo, "src"), { recursive: true });
    writeFileSync(path.join(repo, "src/routes.ts"), "export const a = 1;\nexport const b = 2;\nexport function handler() { return 1; }\n");
    writeFileSync(path.join(repo, "src/notes.md"), "# notes\nhello\n");
    const task = "fix src/routes.ts:2-4 and review src/notes.md";
    const result = taskAnchor.produce(task, { repoRoot: repo });
    assert.equal(result.candidates.length, 2);
    const kinds = result.candidates.map((c) => c.sourceKind);
    assert.ok(kinds.includes("task_anchor_path"));
    validateCandidates(result.candidates);
    for (const c of result.candidates) {
      assert.equal(c.layer, 1);
      assert.equal(c.trustClass, "user_direct");
      assert.equal(c.instructionPolicy, "data_only");
      assert.equal(c.providerScore, 1.0);
      assert.match(c.sourceRef, /^src\//);
    }
  });
});

test("task-anchor: rejects out-of-scope paths as omissions", { skip: workspaceSkip }, () => {
  withTempRepo("anchor-scope", (repo) => {
    mkdirSync(path.join(repo, "src"), { recursive: true });
    writeFileSync(path.join(repo, "src/inside.ts"), "export const x = 1;\n");
    const result = taskAnchor.produce("inspect ../outside.ts", { repoRoot: repo });
    assert.equal(result.candidates.length, 0);
    assert.ok(result.omissions.some((o) => o.reason === "anchor_outside_scope"));
  });
});

// ---- dirty-files ----

test("dirty-files: surfaces modified and untracked files via git status", { skip: workspaceSkip }, () => {
  withTempRepo("dirty", (repo) => {
    mkdirSync(path.join(repo, "src"), { recursive: true });
    spawnSync("git", ["init", "-q", "--initial-branch=main"], { cwd: repo, stdio: "ignore" });
    spawnSync("git", ["config", "user.email", "ci@example.com"], { cwd: repo, stdio: "ignore" });
    spawnSync("git", ["config", "user.name", "ci"], { cwd: repo, stdio: "ignore" });
    writeFileSync(path.join(repo, "src/a.ts"), "export const a = 1;\n");
    writeFileSync(path.join(repo, "src/b.ts"), "export const b = 1;\n");
    spawnSync("git", ["add", "-A"], { cwd: repo, stdio: "ignore" });
    spawnSync("git", ["commit", "-q", "-m", "init"], { cwd: repo, stdio: "ignore" });
    writeFileSync(path.join(repo, "src/a.ts"), "export const a = 2;\n");
    writeFileSync(path.join(repo, "src/c.ts"), "export const c = 3;\n");
    const result = dirtyFiles.produce("", { repoRoot: repo });
    assert.ok(result.candidates.length >= 2);
    const paths = result.candidates.map((c) => c.sourceRef.split(":")[0]);
    assert.ok(paths.includes("src/a.ts"));
    assert.ok(paths.includes("src/c.ts"));
    validateCandidates(result.candidates);
    const aCandidate = result.candidates.find((c) => c.sourceRef.startsWith("src/a.ts"));
    assert.match(aCandidate.text, /export const a = 2/);
  });
});

test("workingTreeSummary buckets modified-tracked vs untracked, sorted", { skip: workspaceSkip }, () => {
  withTempRepo("wt-summary", (repo) => {
    spawnSync("git", ["init", "-q", "--initial-branch=main"], { cwd: repo, stdio: "ignore" });
    spawnSync("git", ["config", "user.email", "ci@example.com"], { cwd: repo, stdio: "ignore" });
    spawnSync("git", ["config", "user.name", "ci"], { cwd: repo, stdio: "ignore" });
    writeFileSync(path.join(repo, "a.ts"), "export const a = 1;\n");
    writeFileSync(path.join(repo, "b.ts"), "export const b = 1;\n");
    spawnSync("git", ["add", "-A"], { cwd: repo, stdio: "ignore" });
    spawnSync("git", ["commit", "-q", "-m", "init"], { cwd: repo, stdio: "ignore" });
    writeFileSync(path.join(repo, "a.ts"), "export const a = 2;\n"); // modified tracked
    writeFileSync(path.join(repo, "z.ts"), "export const z = 9;\n"); // untracked
    writeFileSync(path.join(repo, "m.ts"), "export const m = 9;\n"); // untracked (sort check)

    const summary = dirtyFiles.workingTreeSummary(repo);
    assert.equal(summary.available, true);
    assert.deepEqual(summary.dirtyTracked, ["a.ts"]);
    assert.deepEqual(summary.untracked, ["m.ts", "z.ts"], "untracked must be sorted");
  });
});

test("workingTreeSummary reports unavailable outside a git repo", { skip: workspaceSkip }, () => {
  withTempRepo("wt-nogit", (repo) => {
    // No git init.
    const summary = dirtyFiles.workingTreeSummary(repo);
    assert.equal(summary.available, false);
    assert.deepEqual(summary.dirtyTracked, []);
    assert.deepEqual(summary.untracked, []);
  });
});

// ---- graph-resolve ----

test("graph-resolve: emits file and symbol candidates when graph is available", { skip: workspaceSkip }, () => {
  const fixture = path.join(CORTEX, "evals/fixture-repos/typescript-commerce");
  withTempRepo("resolve", (repo) => {
    cpSync(fixture, repo, { recursive: true });
    spawnSync(process.execPath, [path.join(CORTEX, "scripts/cortex.mjs"), "graph", "build", "--out", ".agent"], { cwd: repo, stdio: "pipe" });
    // The path must exist IN THE FIXTURE. This previously read
    // "src/services/checkout.ts", which the fixture does not contain — and the
    // test still passed, because graph-resolve was looking for the generation at
    // `.agent/graph.json` (a path the builder never wrote), so every run hit the
    // `graph_unavailable` early return below and asserted nothing at all.
    // Fixing the adapter's path exposed the vacuum.
    const task = "investigate placeOrder in src/service.ts";
    const result = graphResolve.produce(task, { repoRoot: repo });
    if (result.omissions.length && result.omissions.every((o) => o.reason === "graph_unavailable")) {
      throw new Error("graph must be available here — the build above writes .agent/graph/graph.db");
    }
    assert.ok(result.candidates.length > 0, "expected at least one graph-resolved candidate");
    validateCandidates(result.candidates);
    for (const c of result.candidates) {
      assert.equal(c.layer, 3);
      assert.equal(c.trustClass, "workspace_tracked");
      assert.match(c.sourceKind, /^graph_resolve_/);
    }
  });
});

test("graph-resolve: emits omission when no graph is available", { skip: workspaceSkip }, () => {
  withTempRepo("no-graph", (repo) => {
    mkdirSync(path.join(repo, "src"), { recursive: true });
    writeFileSync(path.join(repo, "src/foo.ts"), "export const x = 1;\n");
    const result = graphResolve.produce("look at src/foo.ts", { repoRoot: repo });
    assert.equal(result.candidates.length, 0);
    assert.ok(result.omissions.some((o) => o.reason === "graph_unavailable"));
  });
});

// ---- git-metadata ----

test("git-metadata: emits exactly one summary candidate per repo", { skip: workspaceSkip }, () => {
  withTempRepo("meta", (repo) => {
    spawnSync("git", ["init", "-q", "--initial-branch=main"], { cwd: repo, stdio: "ignore" });
    spawnSync("git", ["config", "user.email", "ci@example.com"], { cwd: repo, stdio: "ignore" });
    spawnSync("git", ["config", "user.name", "ci"], { cwd: repo, stdio: "ignore" });
    writeFileSync(path.join(repo, "README.md"), "hi\n");
    spawnSync("git", ["add", "-A"], { cwd: repo, stdio: "ignore" });
    spawnSync("git", ["commit", "-q", "-m", "init"], { cwd: repo, stdio: "ignore" });
    const result = gitMetadata.produce("", { repoRoot: repo });
    assert.equal(result.candidates.length, 1);
    const c = result.candidates[0];
    assert.match(c.text, /branch: main/);
    assert.match(c.text, /worktree: clean/);
    validateCandidates(result.candidates);
  });
});

// ---- rules-documents ----

test("rules-documents: includes AGENTS.md/CLAUDE.md and applies the per-rule byte cap", { skip: workspaceSkip }, () => {
  withTempRepo("rules", (repo) => {
    writeFileSync(path.join(repo, "AGENTS.md"), "a".repeat(4000) + "\n");
    writeFileSync(path.join(repo, "CLAUDE.md"), "short claude\n");
    mkdirSync(path.join(repo, ".claude/rules"), { recursive: true });
    writeFileSync(path.join(repo, ".claude/rules/skills.md"), "skill body\n");
    writeFileSync(path.join(repo, ".claude/rules/brands.md"), "brands body\n");
    mkdirSync(path.join(repo, ".codex/rules"), { recursive: true });
    writeFileSync(path.join(repo, ".codex/rules/skills.md"), "codex skills\n");
    const result = rulesDocuments.produce("skills audit", { repoRoot: repo, ruleByteCap: 1500 });
    const ids = result.candidates.map((c) => c.id);
    assert.ok(ids.some((id) => id.endsWith(":AGENTS.md")));
    assert.ok(ids.some((id) => id.endsWith(":CLAUDE.md")));
    const agents = result.candidates.find((c) => c.id.endsWith(":AGENTS.md"));
    assert.ok(agents.text.endsWith("\n\n[…truncated; per-rule byte cap applied…]"));
    assert.ok(agents.text.length <= 1500 + 64, `agents body unexpectedly long: ${agents.text.length}`);
    validateCandidates(result.candidates);
  });
});

test("rules-documents: never floods when many rules exist; respects maxRules", { skip: workspaceSkip }, () => {
  withTempRepo("rules-flood", (repo) => {
    mkdirSync(path.join(repo, ".claude/rules"), { recursive: true });
    for (let i = 0; i < 30; i += 1) writeFileSync(path.join(repo, `.claude/rules/rule-${i}.md`), `body ${i}\n`);
    writeFileSync(path.join(repo, "AGENTS.md"), "agents\n");
    const result = rulesDocuments.produce("", { repoRoot: repo, ruleByteCap: 1500, maxRules: 5 });
    // AGENTS.md always counts as one rule candidate; rule-file count must be capped.
    const ruleFileCandidates = result.candidates.filter((c) => c.sourceRef.includes(".claude/rules/"));
    assert.ok(ruleFileCandidates.length <= 5);
  });
});

// ---- live-overlay ----

test("live-overlay: emits candidates for files newer than the graph generation", { skip: workspaceSkip }, () => {
  withTempRepo("overlay", (repo) => {
    mkdirSync(path.join(repo, ".agent", "graph"), { recursive: true });
    mkdirSync(path.join(repo, "src"), { recursive: true });
    const generatedAt = new Date(Date.now() - 60_000).toISOString();
    writeFileSync(path.join(repo, "src/old.ts"), "export const x = 1;\n");
    const oldTs = new Date(Date.now() - 120_000).toISOString();
    const newTs = new Date(Date.now() + 5_000).toISOString();
    seedStore(repo, {
      manifest: { generatedAt, provider: { id: "test" } },
      nodes: [
        {
          id: "file:src/old.ts",
          kind: "file",
          path: "src/old.ts",
          evidence: [{ path: "src/old.ts", contentHash: "deadbeef".repeat(8) }],
        },
      ],
    });
    writeFileSync(path.join(repo, "src/new.ts"), "export const y = 2;\n");
    const result = liveOverlay.produce("", { repoRoot: repo });
    const newCandidate = result.candidates.find((c) => c.sourceRef.startsWith("src/new.ts"));
    assert.ok(newCandidate, "expected new.ts to be surfaced as a live overlay");
    validateCandidates(result.candidates);
  });
});

// ---- index / fan-out ----

test("index: produceCandidatesWithTelemetry runs every adapter once and dedupes", { skip: workspaceSkip }, () => {
  withTempRepo("index", (repo) => {
    mkdirSync(path.join(repo, "src"), { recursive: true });
    spawnSync("git", ["init", "-q", "--initial-branch=main"], { cwd: repo, stdio: "ignore" });
    spawnSync("git", ["config", "user.email", "ci@example.com"], { cwd: repo, stdio: "ignore" });
    spawnSync("git", ["config", "user.name", "ci"], { cwd: repo, stdio: "ignore" });
    writeFileSync(path.join(repo, "AGENTS.md"), "agents\n");
    writeFileSync(path.join(repo, "src/a.ts"), "export const a = 1;\n");
    spawnSync("git", ["add", "-A"], { cwd: repo, stdio: "ignore" });
    spawnSync("git", ["commit", "-q", "-m", "init"], { cwd: repo, stdio: "ignore" });
    writeFileSync(path.join(repo, "src/a.ts"), "export const a = 99;\n");
    const result = produceCandidatesWithTelemetry("audit src/a.ts", { repoRoot: repo });
    assert.equal(result.provider, "membrane-sources");
    assert.equal(result.adapters.length, 6);
    for (const entry of result.adapters) {
      assert.ok(typeof entry.id === "string");
      assert.ok(entry.ok === true || entry.ok === false);
    }
    // AGENTS.md candidate is produced by rules-documents; src/a.ts is dirty AND a task anchor.
    const ids = result.candidates.map((c) => c.id);
    assert.ok(ids.some((id) => id.includes(":AGENTS.md")));
    assert.ok(ids.some((id) => id.includes(":src/a.ts")));
    validateCandidates(result.candidates);
  });
});

test("index: an adapter that throws still leaves the rest of the fan-out intact", { skip: workspaceSkip }, () => {
  withTempRepo("throw", (repo) => {
    writeFileSync(path.join(repo, "AGENTS.md"), "agents\n");
    // Force graphResolve to think a graph is present by seeding a tiny store.
    seedStore(repo, { manifest: { generatedAt: null }, nodes: [] });
    const result = produceCandidatesWithTelemetry("", { repoRoot: repo });
    // AGENTS.md should still surface from rules-documents even if graph-resolve misbehaves.
    const ids = result.candidates.map((c) => c.id);
    assert.ok(ids.some((id) => id.includes(":AGENTS.md")));
  });
});
