import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { closeStore, openStore, readManifestEnvelope } from "../src/graph/store-sqlite.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CORTEX = path.resolve(HERE, "..");
const CLI = path.join(CORTEX, "scripts/cortex.mjs");
const FIXTURE = path.join(CORTEX, "evals/fixture-repos/typescript-commerce");

test("regular cortex build writes graph and flow artifacts beside bootstrap map", () => {
  const repo = path.join(os.tmpdir(), `cortex-live-build-${process.pid}-${Date.now()}`);
  fs.cpSync(FIXTURE, repo, { recursive: true });
  try {
    assert.equal(spawnSync("git", ["init", "--quiet"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["config", "user.email", "test@example.invalid"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["config", "user.name", "Test"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["add", "-A"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["commit", "--quiet", "-m", "fixture"], { cwd: repo }).status, 0);
    const baseCommit = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repo, encoding: "utf8" }).stdout.trim();
    const result = spawnSync(process.execPath, [CLI, "build", "--out", ".agent", "--check"], { cwd: repo, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    assert.match(result.stdout, /phase1_maintenance=complete/);
    assert.match(result.stdout, /full_cortex=not_requested/);
    assert.doesNotMatch(result.stdout, /next=phase2/);
    assert.ok(fs.existsSync(path.join(repo, ".agent/map.json")), "map.json must exist");
    assert.ok(fs.existsSync(path.join(repo, ".agent/graph/graph.db")), "graph store must exist");
    assert.ok(fs.existsSync(path.join(repo, ".agent/flows.json")), "flows.json must exist");
    assert.ok(fs.existsSync(path.join(repo, ".agent/manifest.json")), ".agent/manifest.json must exist");
    assert.equal(fs.existsSync(path.join(repo, ".agent/START-HERE.md")), false, "START-HERE.md must NOT exist (retired)");
    const manifest = JSON.parse(fs.readFileSync(path.join(repo, ".agent/manifest.json"), "utf8"));
    assert.equal(manifest.entrypoint, ".agent/");
    // Tree-sitter is the SELECTED provider once it parses anything in the repo
    // (it cleared every qualification gate the lexical incumbent has). The
    // lexical layer stays as the fallback for the extensions it cannot parse.
    assert.equal(manifest.generation.provider ?? manifest.generation.toolVersions?.cortex ?? null, "cortex-treesitter");
    assert.equal(manifest.generation.baseCommit, baseCommit);
    const graph = openStore(path.join(repo, ".agent/graph/graph.db"));
    try {
      const envelope = readManifestEnvelope(graph);
      assert.deepEqual(envelope.sourceObservation, {
        head: baseCommit,
        dirty: false,
        statusDigest: envelope.sourceObservation?.statusDigest,
      });
    } finally {
      closeStore(graph);
    }
    const map = JSON.parse(fs.readFileSync(path.join(repo, ".agent/map.json"), "utf8"));
    assert.equal(map.entrypoint, ".agent/manifest.json");

    // A dirty tree no longer defers the build — the graph is a local index, not
    // a committed artifact — so the build succeeds and indexes what is on disk.
    fs.appendFileSync(path.join(repo, "src/service.ts"), "\n// dirty overlay\n", "utf8");
    const dirtyBuild = spawnSync(process.execPath, [CLI, "build", "--out", ".agent", "--check"], { cwd: repo, encoding: "utf8" });
    assert.equal(dirtyBuild.status, 0, dirtyBuild.stderr || dirtyBuild.stdout);
    assert.doesNotMatch(dirtyBuild.stderr ?? "", /graph_build_deferred_dirty/);
    // …and it says so honestly: a graph built from uncommitted content does not
    // correspond to any commit, so baseCommit is null and the state is recorded
    // as a dirty overlay rather than silently claiming the last commit.
    const dirtyManifest = JSON.parse(fs.readFileSync(path.join(repo, ".agent/manifest.json"), "utf8"));
    assert.equal(dirtyManifest.generation.baseCommit, null);
    assert.equal(dirtyManifest.generation.sourceState, "dirty_overlay");

    assert.equal(spawnSync("git", ["restore", "src/service.ts"], { cwd: repo }).status, 0);
    fs.rmSync(path.join(repo, ".agent/graph"), { recursive: true, force: true });
    const rerun = spawnSync(process.execPath, [CLI], { cwd: repo, encoding: "utf8" });
    assert.equal(rerun.status, 0, rerun.stderr || rerun.stdout);
    assert.match(rerun.stdout, /built .agent\/map.json/);
    assert.match(rerun.stdout, /phase1_ready/);
    assert.match(rerun.stdout, /full_cortex=incomplete/);
    assert.match(rerun.stdout, /next=phase2/);
    assert.ok(fs.existsSync(path.join(repo, ".agent/graph/graph.db")), "graph store must be rebuilt by bare command");
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("build stays fresh when tracked generated docs are rewritten", () => {
  const repo = path.join(os.tmpdir(), `cortex-generated-docs-${process.pid}-${Date.now()}`);
  fs.cpSync(FIXTURE, repo, { recursive: true });
  try {
    fs.renameSync(path.join(repo, "docs/ARCHITECTURE.md"), path.join(repo, "docs/design.md"));
    assert.equal(spawnSync("git", ["init", "--quiet"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["config", "user.email", "test@example.invalid"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["config", "user.name", "Test"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["add", "-A"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["commit", "--quiet", "-m", "fixture"], { cwd: repo }).status, 0);

    const first = spawnSync(process.execPath, [CLI, "build", "--out", ".agent"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(first.status, 0, first.stderr || first.stdout);
    assert.equal(spawnSync("git", [
      "add", "-f", ".agent", "docs/product.md", "docs/architecture.md",
    ], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["commit", "--quiet", "-m", "track generated docs"], { cwd: repo }).status, 0);

    fs.appendFileSync(path.join(repo, "src/service.ts"), "\nexport const changed = true;\n", "utf8");
    assert.equal(spawnSync("git", ["add", "src/service.ts"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["commit", "--quiet", "-m", "change source"], { cwd: repo }).status, 0);

    const second = spawnSync(process.execPath, [CLI, "build", "--out", ".agent"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(second.status, 0, second.stderr || second.stdout);
    const status = spawnSync(process.execPath, [CLI, "graph", "status", "--out", ".agent"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(status.status, 0, status.stderr || status.stdout);
    const manifest = JSON.parse(fs.readFileSync(path.join(repo, ".agent/manifest.json"), "utf8"));
    assert.equal(manifest.generation.sourceState, "clean");
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});
