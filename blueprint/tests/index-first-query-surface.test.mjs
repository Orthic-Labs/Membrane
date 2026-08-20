import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  buildGraphGeneration,
  graphArchitecture,
  graphImpact,
  graphNeighbors,
  graphPath,
  graphStatus,
} from "../src/graph/static-provider.mjs";
import { openStore, closeStore, loadGeneration } from "../src/graph/store-sqlite.mjs";
import { indexedImpact, indexedNeighbors, indexedPath } from "../src/graph/traverse-store.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BLUEPRINT = path.resolve(HERE, "..");
const FIXTURE = path.join(BLUEPRINT, "evals/fixture-repos/typescript-commerce");
const CLI = path.join(BLUEPRINT, "scripts/blueprint.mjs");
const STATIC_PROVIDER = path.join(BLUEPRINT, "src/graph/static-provider.mjs");

function copyFixture() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "blueprint-index-first-"));
  fs.cpSync(FIXTURE, dir, { recursive: true });
  return dir;
}

function runJson(repo, args) {
  const result = spawnSync(process.execPath, [CLI, ...args, "--json"], { cwd: repo, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  return JSON.parse(result.stdout);
}

test("query command handlers cannot call whole-generation readers", () => {
  const source = fs.readFileSync(CLI, "utf8");
  for (const command of ["search", "candidates", "planner-status", "neighbors", "path", "architecture", "impact", "mermaid"]) {
    const start = source.indexOf(`if (subcommand === "${command}")`);
    const next = source.indexOf("if (subcommand ===", start + 1);
    const dispatchEnd = source.indexOf("\n  usage();", start);
    const end = next === -1 ? dispatchEnd : Math.min(next, dispatchEnd);
    const block = source.slice(start, end === -1 ? source.length : end);
    assert.doesNotMatch(block, /readFreshGraph\(|readGeneration\(/, `${command} must remain indexed`);
  }
  const providerSource = fs.readFileSync(STATIC_PROVIDER, "utf8");
  const statusStart = providerSource.indexOf("export function graphStatus");
  const statusEnd = providerSource.indexOf("export function readGeneration", statusStart);
  assert.doesNotMatch(providerSource.slice(statusStart, statusEnd), /loadGeneration\(/, "freshness gate must not materialise the graph");
});

test("indexed traversal is deep-equal to full-load traversal for neighbors, path, and impact", () => {
  const repo = copyFixture();
  const outDir = path.join(repo, ".agent");
  try {
    const generation = buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const db = openStore(path.join(outDir, "graph/graph.db"));
    try {
      const nodeId = "symbol:src/service.ts::OrderService.placeOrder";
      assert.deepEqual(
        indexedNeighbors(db, { nodeId, direction: "both", depth: 1 }),
        graphNeighbors(generation, { nodeId, direction: "both", depth: 1 }),
      );
      assert.deepEqual(
        indexedPath(db, {
          from: "symbol:src/routes.ts::registerOrderRoute",
          to: "symbol:src/store.ts::OrderStore.save",
          maxDepth: 5,
        }),
        graphPath(generation, {
          from: "symbol:src/routes.ts::registerOrderRoute",
          to: "symbol:src/store.ts::OrderStore.save",
          maxDepth: 5,
        }),
      );
      assert.deepEqual(
        indexedImpact(db, { nodeId: "symbol:src/store.ts::OrderStore.save", depth: 3 }),
        graphImpact(generation, { nodeId: "symbol:src/store.ts::OrderStore.save", depth: 3 }),
      );
    } finally {
      closeStore(db);
    }
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("freshness status reads the envelope and file hashes without changing the graph contract", () => {
  const repo = copyFixture();
  const outDir = path.join(repo, ".agent");
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const before = graphStatus(repo, ".agent");
    assert.equal(before.state, "fresh");
    assert.equal(before.capabilities.dirtyOverlayFileCount, 0);
    fs.appendFileSync(path.join(repo, "src/service.ts"), "\nexport const dirty = true;\n");
    const after = graphStatus(repo, ".agent");
    assert.equal(after.state, "stale");
    const db = openStore(path.join(outDir, "graph/graph.db"));
    try {
      assert.equal(loadGeneration(db).manifest.generationId, before.manifest.generationId);
    } finally {
      closeStore(db);
    }
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("v2 structural payloads are bounded and tabular/JSON encodings carry the same rows", () => {
  const repo = copyFixture();
  try {
    const build = spawnSync(process.execPath, [CLI, "graph", "build", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
    assert.equal(build.status, 0, build.stderr || build.stdout);
    const json = runJson(repo, ["graph", "neighbors", "--node", "symbol:src/service.ts::OrderService.placeOrder", "--depth", "2", "--budget", "120"]);
    assert.equal(json.schemaVersion, 2);
    assert.equal(json.sourceState, "clean");
    assert.match(json.generationId, /^xxh128:/);
    assert.equal(json.budget, 120);
    assert.equal(json.truncated, true);
    assert.ok(json.nodes.length + json.edges.length > 0);
    assert.ok(JSON.stringify(json).length < 8192);

    const tabular = spawnSync(process.execPath, [CLI, "graph", "neighbors", "--node", "symbol:src/service.ts::OrderService.placeOrder", "--depth", "2", "--budget", "120"], { cwd: repo, encoding: "utf8" });
    assert.equal(tabular.status, 0, tabular.stderr || tabular.stdout);
    assert.match(tabular.stdout, /nodes\[id kind path startLine endLine\]/);
    assert.match(tabular.stdout, /edges\[id kind source target confidenceTier\]/);
    for (const row of [...json.nodes, ...json.edges]) assert.match(tabular.stdout, new RegExp(row.id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});
