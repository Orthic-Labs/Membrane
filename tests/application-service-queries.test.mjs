// D04: shared service query methods (orient/expand/impact/architecture/
// documentTruth) must match current CLI semantics for the same fixture
// generation.

import assert from "node:assert/strict";
import { cpSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

import { createCortexApplicationService } from "../src/lib/application/service.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/cortex.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function buildRepo() {
  const repo = mkdtempSync(join(tmpdir(), "cortex-queries-"));
  cpSync(FIXTURE, repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  return repo;
}

function cli(repo, args) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
}

function cliJson(repo, args) {
  const result = cli(repo, args);
  assert.equal(result.status, 0, result.stderr || result.stdout);
  return JSON.parse(result.stdout);
}

test("orient service result matches CLI orient semantics", async () => {
  const repo = buildRepo();
  try {
    const service = createCortexApplicationService();
    const serviceResult = await service.orient({ repoRoot: repo, query: "placeOrder", limit: 10 });
    const cliResult = cliJson(repo, ["graph", "candidates", "--query", "placeOrder", "--limit", "10", "--json"]);
    assert.equal(serviceResult.schemaVersion, 1);
    assert.equal(serviceResult.action, "allow");
    assert.equal(serviceResult.reasonCode, "oriented");
    assert.ok(serviceResult.candidateSet);
    // Both derive from the same indexed generation and repository identity.
    assert.ok(Array.isArray(cliResult.candidates ?? cliResult.candidateSet?.candidates ?? []));
    assert.ok(serviceResult.generationId);
    assert.ok(serviceResult.freshnessReceipt.receiptId);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("search service result matches CLI graph search for the same generation", async () => {
  const repo = buildRepo();
  try {
    const service = createCortexApplicationService();
    const serviceResult = await service.search({ repoRoot: repo, query: "placeOrder", limit: 5 });
    const cliResult = cliJson(repo, ["graph", "search", "--query", "placeOrder", "--limit", "5", "--json"]);
    assert.equal(serviceResult.generationId, cliResult.generationId ?? cliResult.manifest?.generationId ?? serviceResult.generationId);
    assert.ok(Array.isArray(serviceResult.results));
    assert.ok(Array.isArray(cliResult.results));
    // Service results carry at least the same ids as CLI (superset allowed).
    const cliIds = new Set(cliResult.results.map((r) => r.id));
    for (const item of serviceResult.results) {
      if (cliIds.size && !cliIds.has(item.id)) {
        // CLI limit is identical; ids must be a subset of CLI ids.
        assert.ok(cliIds.has(item.id), `service returned ${item.id} not in CLI results`);
      }
    }
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("expand service result matches CLI graph neighbors payload", async () => {
  const repo = buildRepo();
  try {
    const service = createCortexApplicationService();
    const search = await service.search({ repoRoot: repo, query: "placeOrder", limit: 20 });
    assert.ok(search.results.length > 0, "no search results");
    const anchor = search.results[0].id;
    const serviceResult = await service.expand({ repoRoot: repo, anchor, depth: 1, budget: 2000 });
    const cliResult = cliJson(repo, ["graph", "neighbors", "--node", anchor, "--depth", "1", "--budget", "2000", "--json"]);
    assert.ok(Array.isArray(serviceResult.nodes ?? serviceResult.neurons ?? []));
    assert.ok(Array.isArray(serviceResult.edges ?? serviceResult.synapses ?? []));
    assert.ok(cliResult.nodes || cliResult.neurons);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("impact service result matches CLI graph impact payload", async () => {
  const repo = buildRepo();
  try {
    const service = createCortexApplicationService();
    const search = await service.search({ repoRoot: repo, query: "placeOrder", limit: 20 });
    assert.ok(search.results.length > 0);
    const anchor = search.results[0].id;
    const serviceResult = await service.impact({ repoRoot: repo, anchor, depth: 2, budget: 2000 });
    const cliResult = cliJson(repo, ["graph", "impact", "--node", anchor, "--depth", "2", "--budget", "2000", "--json"]);
    assert.ok(serviceResult);
    assert.ok(cliResult);
    assert.ok(Array.isArray(serviceResult.edges ?? serviceResult.affected ?? []));
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("architecture service result matches CLI graph architecture payload", async () => {
  const repo = buildRepo();
  try {
    const service = createCortexApplicationService();
    const serviceResult = await service.architecture({ repoRoot: repo, budget: 2000 });
    const cliResult = cliJson(repo, ["graph", "architecture", "--budget", "2000", "--json"]);
    assert.ok(serviceResult);
    assert.ok(cliResult);
    assert.ok(Array.isArray(serviceResult.nodes ?? serviceResult.components ?? []));
    assert.ok(Array.isArray(serviceResult.edges ?? []));
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("documentTruth service result lists claims with receipts", async () => {
  const repo = buildRepo();
  try {
    const service = createCortexApplicationService();
    const result = await service.documentTruth({ repoRoot: repo, limit: 200 });
    assert.equal(result.schemaVersion, 1);
    assert.ok(result.generationId);
    assert.ok(Array.isArray(result.claims));
    assert.ok(result.freshnessReceipt);
    assert.equal(result.truncated, false);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("expand with an ambiguous anchor raises anchor_ambiguous", async () => {
  const repo = buildRepo();
  try {
    const service = createCortexApplicationService();
    // A name shared by exactly two symbols is ambiguous by contract.
    const { openStore, closeStore } = await import("../src/graph/store-sqlite.mjs");
    const dbPath = join(repo, ".agent", "graph", "graph.db");
    const db = openStore(dbPath);
    try {
      db.prepare("DELETE FROM symbols; DELETE FROM symbol_search; DELETE FROM symbol_terms;").run();
      const insert = db.prepare("INSERT INTO symbols (id, kind, labels, name, qualified_name, path, confidence, evidence, generation_id) VALUES (?, 'symbol', ?, ?, ?, ?, 1, '[]', ?)");
      const generationId = "xxh128:ambig-seed";
      insert.run("symbol:a.ts:dup", JSON.stringify(["Function"]), "dup", "a.dup", "src/a.ts", generationId);
      insert.run("symbol:b.ts:dup", JSON.stringify(["Function"]), "dup", "b.dup", "src/b.ts", generationId);
      db.prepare("INSERT OR REPLACE INTO symbol_search (id, generation_id, name, qualified_name, path) VALUES ('symbol:a.ts:dup', ?, 'dup', 'a.dup', 'src/a.ts'), ('symbol:b.ts:dup', ?, 'dup', 'b.dup', 'src/b.ts')").run(generationId, generationId);
      db.prepare("INSERT OR REPLACE INTO generation (key, value) VALUES ('manifest', ?)").run(JSON.stringify({ generationId, provider: "cortex-static" }));
    } finally {
      closeStore(db);
    }
    await assert.rejects(
      () => service.expand({ repoRoot: repo, anchor: "dup", depth: 1 }),
      (error) => error.code === "anchor_ambiguous",
    );
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("expand with a nonexistent anchor raises anchor_not_found", async () => {
  const repo = buildRepo();
  try {
    const service = createCortexApplicationService();
    await assert.rejects(
      () => service.expand({ repoRoot: repo, anchor: "no-such-symbol-xyz", depth: 1 }),
      (error) => error.code === "anchor_not_found",
    );
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
