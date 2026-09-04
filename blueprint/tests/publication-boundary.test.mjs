import assert from "node:assert/strict";
import test from "node:test";
import { openStore, closeStore, saveGeneration, loadGeneration, adoptRebuiltGeneration, bulkInsertGeneration } from "../src/graph/store-sqlite.mjs";
import { evaluatePublicationCandidate } from "../src/graph/publication-policy.mjs";
import { applyFileDelta } from "../src/graph/delta-store.mjs";

const file = (path) => ({ id: `file:${path}`, kind: "file", name: path, qualifiedName: path, path, labels: ["File"], confidence: null,
  evidence: [{ path, startLine: 1, endLine: 1, contentHash: `hash:${path}` }] });
function generation(id = "prior") {
  return { schemaVersion: 1, provider: { id: "blueprint-static" }, repoRoot: "/unused",
    manifest: { generationId: id, complete: true, counts: { nodes: 2, edges: 0 } },
    nodes: [file("a.ts"), file("b.ts")], edges: [] };
}
function snapshot(db) {
  return JSON.stringify({ generation: loadGeneration(db), watch: db.prepare("SELECT * FROM watch_state ORDER BY key").all(),
    files: db.prepare("SELECT * FROM files ORDER BY path").all(), owners: db.prepare("SELECT * FROM fact_owner ORDER BY fact_id").all() });
}

test("production save refuses partial replacements and preserves the complete graph and clocks", () => {
  const db = openStore(":memory:");
  try {
    saveGeneration(db, generation(), { populateState: true });
    db.prepare("INSERT INTO watch_state(key,value) VALUES('source_clock','9')").run();
    const before = snapshot(db);
    const candidates = [
      { ...generation("partial"), manifest: { ...generation().manifest, complete: false } },
      { ...generation("unknown"), manifest: { generationId: "unknown" } },
      { ...generation("truncated"), manifest: { ...generation().manifest, truncated: true } },
      { ...generation("wrong-count"), manifest: { ...generation().manifest, counts: { nodes: 99, edges: 0 } } },
      { ...generation("doc-truncated"), docTruth: { truncated: true } },
    ];
    for (const candidate of candidates) {
      assert.throws(() => saveGeneration(db, candidate, { populateState: true }), (error) => error.code === "publication_incomplete" && error.decision.action === "block");
      assert.equal(snapshot(db), before);
      assert.throws(() => adoptRebuiltGeneration(db, candidate, { populateState: true }), (error) => error.code === "publication_incomplete");
      assert.equal(snapshot(db), before, "failed adoption must not reset watcher state");
    }
    saveGeneration(db, generation("next"), { populateState: true });
    assert.equal(loadGeneration(db).manifest.generationId, "next");
  } finally { closeStore(db); }
});

test("the raw bulk writer cannot bypass the sealed-generation publication boundary", () => {
  const db = openStore(":memory:");
  try {
    saveGeneration(db, generation());
    const before = snapshot(db);
    assert.throws(() => bulkInsertGeneration(db, { nodes: [], edges: [] }), (error) => error.code === "store_sealed_replace_unsupported");
    assert.equal(snapshot(db), before);
  } finally { closeStore(db); }
});

test("an explicitly empty changed-path set protects every previous fact", () => {
  const prior = generation();
  const candidate = { ...generation("empty"), nodes: [], manifest: { generationId: "empty", complete: true, counts: { nodes: 0, edges: 0 } } };
  assert.equal(evaluatePublicationCandidate(candidate, { priorGeneration: prior, changedPaths: [] }).action, "block");
  assert.equal(evaluatePublicationCandidate(null, { priorGeneration: prior, changedPaths: [] }).reasonCode, "generation_shape_invalid");
  const db = openStore(":memory:");
  try {
    saveGeneration(db, prior);
    const before = snapshot(db);
    assert.throws(() => saveGeneration(db, candidate, { changedPaths: [] }), (error) => error.decision.problems.includes("unexpected_unrelated_fact_shrink"));
    assert.equal(snapshot(db), before);
    saveGeneration(db, candidate, { changedPaths: ["a.ts", "b.ts"] });
    assert.equal(loadGeneration(db).nodes.length, 0);
  } finally { closeStore(db); }
});

test("partial file extraction cannot delete complete facts or acknowledge its source clock", () => {
  const db = openStore(":memory:");
  try {
    saveGeneration(db, generation(), { populateState: true });
    const before = snapshot(db);
    assert.throws(() => applyFileDelta(db, {
      path: "a.ts", eventKind: "modify", contentDigest: "xxh128:changed", sourceClock: 1,
      parsed: { nodes: [file("a.ts")], edges: [], truncated: true },
    }), (error) => error.code === "publication_incomplete");
    assert.equal(snapshot(db), before);
  } finally { closeStore(db); }
});
