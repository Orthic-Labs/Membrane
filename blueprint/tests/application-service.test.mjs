// D03: shared application service for status, search, and resolve.
// Service reads must not materialize the full generation; error codes must be
// stable and schema-valid.

import assert from "node:assert/strict";
import { mkdtempSync, rmSync, cpSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { createCortexApplicationService } from "../src/lib/application/service.mjs";
import { seedStore, readEnvelope, writeEnvelope, mutateManifest } from "./_store-helpers.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { CortexError } from "../src/lib/application/errors.mjs";
import { syncToCurrentSource } from "../src/graph/barrier.mjs";

const FIXTURE = join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce");

function tempRepo() {
  const repo = mkdtempSync(join(tmpdir(), "cortex-service-"));
  cpSync(FIXTURE, repo, { recursive: true });
  return repo;
}

function builtRepo() {
  const repo = tempRepo();
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  return repo;
}

async function expectCortexError(promise, code) {
  try {
    await promise;
  } catch (error) {
    assert.ok(error instanceof CortexError, `expected CortexError, got ${error.constructor?.name ?? error}`);
    assert.equal(error.code, code);
    return error;
  }
  assert.fail(`expected error code ${code}`);
}

test("status works on a built graph and reports repository identity", async () => {
  const repo = builtRepo();
  try {
    const service = createCortexApplicationService();
    const result = await service.status({ repoRoot: repo });
    assert.equal(result.schemaVersion, 1);
    assert.ok(result.repository);
    assert.ok(typeof result.state === "string");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("missing graph raises graph_missing for search", async () => {
  const repo = tempRepo();
  try {
    const service = createCortexApplicationService();
    await expectCortexError(service.search({ repoRoot: repo, query: "placeOrder" }), "graph_missing");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("search returns generation-pinned results on a fresh graph", async () => {
  const repo = builtRepo();
  try {
    const service = createCortexApplicationService();
    const result = await service.search({ repoRoot: repo, query: "placeOrder", limit: 5 });
    assert.equal(result.schemaVersion, 1);
    assert.equal(result.kind, "search");
    assert.ok(result.generationId);
    assert.ok(Array.isArray(result.results));
    assert.ok(result.freshnessReceipt);
    assert.equal(result.freshnessReceipt.barrierResult, "caught_up");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("stale barrier rejection raises stale_blocked unless allowStale", async () => {
  const repo = builtRepo();
  try {
    // Force an unreachable target clock so the barrier times out -> stale_blocked.
    const { openStore, closeStore } = await import("../src/graph/store-sqlite.mjs");
    const dbPath = join(repo, ".agent", "graph", "graph.db");
    const db = openStore(dbPath);
    try {
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('source_clock','99999') ON CONFLICT(key) DO UPDATE SET value='99999'").run();
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('applied_clock','0') ON CONFLICT(key) DO UPDATE SET value='0'").run();
      db.prepare("DELETE FROM event_journal").run();
    } finally {
      closeStore(db);
    }
    const service = createCortexApplicationService();
    await expectCortexError(service.search({ repoRoot: repo, query: "placeOrder", timeoutMs: 80 }), "stale_blocked");
    // allowStale bypasses the barrier rejection.
    const result = await service.search({ repoRoot: repo, query: "placeOrder", allowStale: true, timeoutMs: 80, limit: 5 });
    assert.ok(Array.isArray(result.results));
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("freshness work aborts with request_cancelled", async () => {
  const repo = builtRepo();
  try {
    const { openStore, closeStore } = await import("../src/graph/store-sqlite.mjs");
    const db = openStore(join(repo, ".agent", "graph", "graph.db"));
    try {
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('source_clock','99999') ON CONFLICT(key) DO UPDATE SET value=excluded.value").run();
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('applied_clock','0') ON CONFLICT(key) DO UPDATE SET value=excluded.value").run();
      db.prepare("DELETE FROM event_journal").run();
    } finally { closeStore(db); }
    const controller = new AbortController();
    controller.abort();
    const service = createCortexApplicationService();
    await expectCortexError(service.search({ repoRoot: repo, query: "placeOrder", timeoutMs: 100 }, { signal: controller.signal }), "request_cancelled");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("freshness cancellation waits for active reconciliation before returning", async () => {
  const repo = builtRepo();
  const { openStore, closeStore } = await import("../src/graph/store-sqlite.mjs");
  const db = openStore(join(repo, ".agent", "graph", "graph.db"));
  let release;
  let settled = false;
  try {
    const controller = new AbortController();
    const work = syncToCurrentSource(db, repo, {
      signal: controller.signal,
      timeoutMs: 100,
      reconcileFn: () => new Promise((resolve) => { release = resolve; }),
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    controller.abort();
    work.finally(() => { settled = true; }).catch(() => {});
    await new Promise((resolve) => setTimeout(resolve, 10));
    assert.equal(settled, false);
    release({ ok: true });
    await assert.rejects(work, { code: "request_cancelled" });
  } finally {
    closeStore(db);
    rmSync(repo, { recursive: true, force: true });
  }
});

test("generation pin mismatch raises generation_mismatch", async () => {
  const repo = builtRepo();
  try {
    const manifest = readEnvelope(repo, "manifest");
    const current = manifest.generationId;
    const service = createCortexApplicationService();
    await expectCortexError(service.search({ repoRoot: repo, query: "placeOrder", generation: "xxh128:not-the-current" }), "generation_mismatch");
    const ok = await service.search({ repoRoot: repo, query: "placeOrder", generation: current, limit: 5 });
    assert.ok(Array.isArray(ok.results));
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("resolve returns a node with generation and receipt", async () => {
  const repo = builtRepo();
  try {
    const service = createCortexApplicationService();
    const search = await service.search({ repoRoot: repo, query: "placeOrder", limit: 20 });
    assert.ok(search.results.length > 0, "expected at least one result");
    const nodeId = search.results[0].id;
    const resolved = await service.resolve({ repoRoot: repo, nodeId });
    assert.ok(resolved);
    assert.equal(resolved.generationId, search.generationId);
    assert.ok(resolved.freshnessReceipt);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("resolve of an unknown node raises node_not_found", async () => {
  const repo = builtRepo();
  try {
    const service = createCortexApplicationService();
    await expectCortexError(service.resolve({ repoRoot: repo, nodeId: "symbol:does-not-exist" }), "node_not_found");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("search on a seeded generation without building works read-only", async () => {
  const repo = tempRepo();
  try {
    seedStore(repo, {
      manifest: { generationId: "xxh128:seed-1", provider: "cortex-static" },
      provider: { id: "cortex-static", version: "0.2.0", precisionTier: "LEXICAL" },
      nodes: [
        {
          id: "symbol:src/service.ts:placeOrder",
          kind: "symbol",
          path: "src/service.ts",
          name: "placeOrder",
          qualifiedName: "fixture.OrderService.placeOrder",
          labels: ["Method"],
          confidence: 1,
          evidence: [],
          span: null,
        },
      ],
      edges: [],
      anchors: [],
    });
    const service = createCortexApplicationService();
    const result = await service.search({ repoRoot: repo, query: "placeOrder", limit: 5 });
    assert.ok(Array.isArray(result.results));
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
