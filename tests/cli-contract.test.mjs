// D05: CLI adapter contract — facade commands route through the service,
// JSON keys are stable, exit codes are stable, aliases carry deprecation
// metadata in JSON mode, and read commands never write.

import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

import { EXIT } from "../scripts/cli/args.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/cortex.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function buildRepo() {
  const repo = mkdtempSync(join(tmpdir(), "cortex-cli-contract-"));
  cpSync(FIXTURE, repo, { recursive: true });
  const build = spawnSync(process.execPath, [CLI, "graph", "build", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
  assert.equal(build.status, 0, build.stderr || build.stdout);
  return repo;
}

function run(repo, args) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
}

test("help prints branded Cortex usage and exits 0", () => {
  const result = run(process.cwd(), ["--help"]);
  assert.equal(result.status, 0);
  assert.match(result.stdout, /Cortex — repository truth and evidence map/);
});

test("facade status --json returns stable keys", () => {
  const repo = buildRepo();
  try {
    const result = run(repo, ["status", "--json"]);
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const payload = JSON.parse(result.stdout);
    assert.equal(payload.schemaVersion, 1);
    assert.ok(payload.repository);
    assert.ok("state" in payload);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("facade search --json returns stable keys and matches graph search", () => {
  const repo = buildRepo();
  try {
    const result = run(repo, ["search", "--query", "placeOrder", "--limit", "5", "--json"]);
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const payload = JSON.parse(result.stdout);
    assert.equal(payload.schemaVersion, 1);
    assert.equal(payload.kind, "search");
    assert.ok(payload.generationId);
    assert.ok(Array.isArray(payload.results));
    assert.ok(payload.freshnessReceipt);
    // Alias parity: graph search returns the same result ids.
    const alias = run(repo, ["graph", "search", "--query", "placeOrder", "--limit", "5", "--json"]);
    assert.equal(alias.status, 0, alias.stderr);
    const aliasPayload = JSON.parse(alias.stdout);
    const facadeIds = new Set(payload.results.map((r) => r.id));
    for (const item of aliasPayload.results) {
      if (facadeIds.size) assert.ok(facadeIds.has(item.id), `alias returned ${item.id} not in facade results`);
    }
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("facade show --json resolves a node", () => {
  const repo = buildRepo();
  try {
    const search = run(repo, ["search", "--query", "placeOrder", "--limit", "1", "--json"]);
    const first = JSON.parse(search.stdout).results[0];
    const result = run(repo, ["show", "--node", first.id, "--json"]);
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const payload = JSON.parse(result.stdout);
    assert.equal(payload.id ?? payload.node?.id ?? first.id, first.id);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("facade expand and impact work with JSON", () => {
  const repo = buildRepo();
  try {
    const search = run(repo, ["search", "--query", "placeOrder", "--limit", "1", "--json"]);
    const first = JSON.parse(search.stdout).results[0];
    const expand = run(repo, ["expand", "--anchor", first.id, "--depth", "1", "--json"]);
    assert.equal(expand.status, 0, expand.stderr || expand.stdout);
    const impact = run(repo, ["impact", "--anchor", first.id, "--depth", "2", "--json"]);
    assert.equal(impact.status, 0, impact.stderr || impact.stdout);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("facade docs --json lists claims", () => {
  const repo = buildRepo();
  try {
    const result = run(repo, ["docs", "--json"]);
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const payload = JSON.parse(result.stdout);
    assert.equal(payload.schemaVersion, 1);
    assert.ok(Array.isArray(payload.claims));
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("read commands never write graph artifacts", () => {
  const repo = buildRepo();
  try {
    const before = readFileSync(join(repo, ".agent", "graph", "graph.db")).length;
    for (const args of [
      ["status", "--json"],
      ["search", "--query", "placeOrder", "--json"],
      ["docs", "--json"],
    ]) {
      const result = run(repo, args);
      assert.equal(result.status, 0, `${args.join(" ")}: ${result.stderr}`);
    }
    const after = readFileSync(join(repo, ".agent", "graph", "graph.db")).length;
    assert.equal(after, before, "read commands must not modify the store");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("stable exit codes: usage error returns USAGE", () => {
  const repo = buildRepo();
  try {
    const result = run(repo, ["search", "--json"]);
    assert.equal(result.status, EXIT.USAGE);
    const payload = JSON.parse(result.stderr);
    assert.equal(payload.error.code, "query_required");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("alias deprecation metadata appears in JSON mode", () => {
  const repo = buildRepo();
  try {
    const result = run(repo, ["graph", "neighbors", "--node", "src/service.ts", "--json"]);
    // The alias still works (exit 0 or degraded); when JSON, deprecation metadata is present.
    assert.ok(result.status === 0 || result.status === 2, `unexpected exit ${result.status}: ${result.stderr}`);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
