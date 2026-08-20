// Frozen P0 recovery equivalence: current service cancellation, publication
// rollback, and snapshot poison handling must remain typed and recoverable.

import assert from "node:assert/strict";
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import corpus from "../evals/equivalence/recovery-corpus-v1.json" with { type: "json" };
import { createCortexApplicationService } from "../src/lib/application/service.mjs";
import { adoptFileAtomically } from "../src/graph/atomic-store-adoption.mjs";
import { closeStore, openStore, openStoreReadOnly, readManifestEnvelope } from "../src/graph/store-sqlite.mjs";
import { createSnapshot } from "../src/graph/snapshots.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";

const FIXTURE = join(import.meta.dirname, "..", "evals", "fixture-repos", "typescript-commerce");

function caseFor(id) {
  const found = corpus.cases.find((entry) => entry.id === id);
  assert.ok(found, `recovery corpus case missing: ${id}`);
  return found;
}

function builtRepo() {
  const repo = mkdtempSync(join(tmpdir(), "cortex-recovery-equivalence-"));
  cpSync(FIXTURE, repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  return repo;
}

test("cancelled service request is typed and leaves an independent reader usable", async () => {
  const scenario = caseFor("cancelled-service-request-is-typed-and-isolated");
  const repo = builtRepo();
  try {
    const service = createCortexApplicationService();
    const controller = new AbortController();
    controller.abort();
    await assert.rejects(
      service.search({ repoRoot: repo, query: "placeOrder" }, { signal: controller.signal }),
      { code: scenario.expectedCode },
    );
    const healthy = await service.search({ repoRoot: repo, query: "placeOrder", limit: 5 });
    assert.ok(healthy.results.length > 0);
    assert.ok(healthy.generationId);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("failed publication restores prior validated database", () => {
  const scenario = caseFor("failed-publication-restores-prior-validated-db");
  const repo = builtRepo();
  const graphDir = join(repo, ".agent", "graph");
  const target = join(graphDir, "graph.db");
  const source = join(graphDir, "graph.db.rebuild-fixture");
  try {
    const before = readFileSync(target);
    writeFileSync(source, before);
    assert.throws(() => adoptFileAtomically(source, target, () => {
      throw Object.assign(new Error("simulated interrupted publication validation"), { code: scenario.expectedCode });
    }), { code: scenario.expectedCode });
    assert.equal(existsSync(source), false);
    const db = openStoreReadOnly(target);
    try {
      assert.deepEqual(readManifestEnvelope(db)?.generationId ? readFileSync(target) : null, before);
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("poisoned snapshot envelope returns a typed error and valid envelope recovers", () => {
  const scenario = caseFor("poisoned-snapshot-envelope-is-typed-and-recoverable");
  const db = openStore(":memory:");
  try {
    db.prepare("INSERT INTO generation(key,value) VALUES (?,?)").run("manifest", JSON.stringify({ complete: true, generationId: "g1", manifestDigest: "m1" }));
    db.prepare("INSERT INTO generation(key,value) VALUES (?,?)").run("sourceObservation", JSON.stringify({ head: "h1", dirty: false }));
    db.prepare("INSERT INTO generation_leaf(path,kind,digest) VALUES (?,?,?)").run("a.ts", "file", "d1");
    db.prepare("UPDATE generation SET value=? WHERE key='sourceObservation'").run("{not-json");
    assert.throws(() => createSnapshot(db, "poisoned", "/tmp", { current: { head: "h1", dirty: false } }), { code: scenario.expectedCode });
    db.prepare("UPDATE generation SET value=? WHERE key='sourceObservation'").run(JSON.stringify({ head: "h1", dirty: false }));
    assert.equal(createSnapshot(db, "recovered", "/tmp", { current: { head: "h1", dirty: false } }).idempotent, false);
  } finally { closeStore(db); }
});
