import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { applyFileDelta } from "../src/graph/delta-store.mjs";
import { queryGraph } from "../src/graph/static-provider.mjs";
import { compilerSemanticFact, heuristicFact } from "../src/graph/provenance.mjs";
import { pythonScipProvider } from "../src/providers/compilers/python-scip.mjs";
import {
  SCHEMA_VERSION, closeStore, getGenerationEnvelope, getSchemaVersion, getSymbol,
  hydrateEdgesByIds, hydrateNodesByIds, listEdgeCore, listEdges, listSymbolsByPath,
  loadGeneration, migrationBackupPath, openStore, saveGeneration,
} from "../src/graph/store-sqlite.mjs";

const provider = { id: "scip-python", version: "confidence-fixture" };
function evidence(path, contentHash = "a-before") {
  return [{ path, startLine: 1, endLine: 2, contentHash }];
}
function file(path) {
  return compilerSemanticFact({
    id: `file:${path}`, kind: "file", labels: ["File"], name: path,
    qualifiedName: path, path, provider: provider.id, evidence: evidence(path),
  });
}
function symbol(path, name) {
  return compilerSemanticFact({
    id: `symbol:${path}::${name}`, kind: "symbol", labels: ["Function"], name,
    qualifiedName: name, path, provider: provider.id, precisionTier: "COMPILER", evidence: evidence(path),
  });
}
function fixture() {
  const nodes = [file("a.py"), file("b.py"), symbol("a.py", "answer"), symbol("b.py", "keep")];
  const edge = {
    id: "edge:reference", kind: "REFERENCES", source: "file:a.py", target: "symbol:b.py::keep",
    resolved: true, confidenceTier: "EXACT_RESOLUTION", provider: provider.id,
    precisionTier: "COMPILER", evidence: evidence("a.py"),
  };
  return {
    schemaVersion: 1, provider,
    manifest: { generationId: "fixture-before", complete: true, repo: { sourceHash: "source-before" } },
    nodes,
    edges: [compilerSemanticFact(edge),
      compilerSemanticFact({ ...edge, id: "edge:unresolved", target: null, resolved: false, confidenceTier: "UNRESOLVED" }, { resolved: false }),
      heuristicFact({ ...edge, id: "edge:heuristic", confidenceTier: "CROSS_FILE_HEURISTIC" }, 0.78)],
  };
}
function expectConfidence(facts) {
  for (const fact of facts) {
    assert.equal(fact.confidence, fact.provenance === "HEURISTIC_BRIDGE" ? 0.78 : null, fact.id);
    assert.ok(fact.provenance, `${fact.id}: provenance survives the adapter`);
  }
}

test("full publication and indexed hydration retain null and categorical provenance", () => {
  const db = openStore(":memory:");
  try {
    const generation = fixture();
    saveGeneration(db, generation, { populateState: true });
    const loaded = loadGeneration(db);
    expectConfidence([...loaded.nodes, ...loaded.edges]);
    expectConfidence(hydrateNodesByIds(db, generation.nodes.map((node) => node.id)));
    expectConfidence(hydrateEdgesByIds(db, generation.edges.map((edge) => edge.id)));
    expectConfidence(listSymbolsByPath(db, "a.py"));
    expectConfidence(listEdges(db));
    expectConfidence([getSymbol(db, "symbol:a.py::answer")]);
    assert.equal(listEdgeCore(db).find((edge) => edge.id === "edge:reference").confidence, null);
    assert.equal(listEdgeCore(db).find((edge) => edge.id === "edge:heuristic").confidence, 0.78);
    assert.equal(getGenerationEnvelope(db, "manifest").generationId, "fixture-before");
    assert.deepEqual(db.prepare("PRAGMA foreign_key_check").all(), []);
  } finally { closeStore(db); }
});

test("real SCIP adapter output retains compiler null confidence through SQLite", async () => {
  const repoRoot = join(import.meta.dirname, "fixtures/compiler-adapters/python");
  const collected = await pythonScipProvider.collect({ repoRoot });
  assert.equal(collected.index.state, "ok");
  assert.ok(collected.nodes.length > 0 && collected.edges.length > 0);
  const db = openStore(":memory:");
  try {
    saveGeneration(db, { ...fixture(), nodes: collected.nodes, edges: collected.edges });
    const loaded = loadGeneration(db);
    expectConfidence([...loaded.nodes, ...loaded.edges]);
    for (const edge of listEdges(db)) {
      assert.equal(edge.precisionTier, "COMPILER");
      assert.equal(edge.provider, "scip-python");
    }
  } finally { closeStore(db); }
});

test("file delta preserves compiler nulls and leaves unrelated facts unchanged", () => {
  const db = openStore(":memory:");
  try {
    saveGeneration(db, fixture(), { populateState: true });
    const before = getSymbol(db, "symbol:b.py::keep");
    const result = applyFileDelta(db, {
      eventKind: "modify", path: "a.py", sourceClock: 1, contentDigest: "xxh128:a-after", provider,
      parsed: { nodes: [file("a.py"), symbol("a.py", "updated")], edges: fixture().edges, dependencies: [] },
    });
    assert.equal(result.applied, true);
    expectConfidence(hydrateNodesByIds(db, ["file:a.py", "symbol:a.py::updated"]));
    expectConfidence(listEdges(db));
    assert.equal(getSymbol(db, "symbol:a.py::answer"), null);
    assert.deepEqual(getSymbol(db, "symbol:b.py::keep"), before);
    assert.notEqual(getGenerationEnvelope(db, "manifest").generationId, "fixture-before");
  } finally { closeStore(db); }
});

test("failed replacement rolls back facts and the sealed generation together", () => {
  const db = openStore(":memory:");
  try {
    saveGeneration(db, fixture(), { populateState: true });
    const before = loadGeneration(db);
    const invalid = fixture();
    invalid.nodes.push({ ...symbol("a.py", "invalid"), name: null });
    assert.throws(() => saveGeneration(db, invalid), /NOT NULL/);
    assert.deepEqual(loadGeneration(db), before);
    assert.throws(() => applyFileDelta(db, {
      eventKind: "modify", path: "a.py", contentDigest: "xxh128:failure", provider,
      parsed: { nodes: [file("a.py"), symbol("a.py", "uncommitted")], edges: [], dependencies: [] },
    }, { beforeCommit() { throw new Error("injected delta failure"); } }), /injected delta failure/);
    assert.deepEqual(loadGeneration(db), before);
  } finally { closeStore(db); }
});

test("v18 upgrade preserves sealed bytes, row order, indexes and a recoverable backup", () => {
  const dir = mkdtempSync(join(tmpdir(), "blueprint-confidence-upgrade-"));
  const path = join(dir, "graph.db");
  let db;
  try {
    db = openStore(path, { upToVersion: 18 });
    const legacy = fixture();
    legacy.nodes = legacy.nodes.map((node) => ({ ...node, confidence: 1 }));
    legacy.edges = legacy.edges.map((edge) => ({ ...edge, confidence: edge.provenance === "HEURISTIC_BRIDGE" ? 0.78 : 1 }));
    saveGeneration(db, legacy, { populateState: true });
    const before = JSON.stringify(loadGeneration(db));
    const ordinals = db.prepare("SELECT rowid,id,node_ordinal FROM symbols ORDER BY rowid").all();
    const indexes = db.prepare("SELECT name,sql FROM sqlite_master WHERE type='index' AND tbl_name IN ('symbols','edges') ORDER BY name").all();
    closeStore(db); db = null;
    db = openStore(path);
    assert.equal(getSchemaVersion(db), SCHEMA_VERSION);
    assert.ok(existsSync(migrationBackupPath(path, 18)));
    assert.equal(JSON.stringify(loadGeneration(db)), before, "schema-only upgrade does not reinterpret historical facts");
    assert.deepEqual(db.prepare("SELECT rowid,id,node_ordinal FROM symbols ORDER BY rowid").all(), ordinals);
    assert.deepEqual(db.prepare("SELECT name,sql FROM sqlite_master WHERE type='index' AND tbl_name IN ('symbols','edges') ORDER BY name").all(), indexes);
    assert.equal(getSymbol(db, "symbol:a.py::answer").confidence, null, "public compatibility view drops legacy fake probability");
    assert.equal(listEdges(db).find((edge) => edge.id === "edge:heuristic").confidence, 0.78);
    assert.equal(getGenerationEnvelope(db, "manifest").generationId, "fixture-before");
    saveGeneration(db, fixture());
    expectConfidence([...loadGeneration(db).nodes, ...listEdges(db)]);
  } finally {
    if (db) closeStore(db);
    rmSync(dir, { recursive: true, force: true });
  }
});

test("public graph queries normalize tagged legacy probabilities without mutating sealed facts", () => {
  const generation = fixture();
  const target = generation.nodes.find((node) => node.name === "answer");
  target.confidence = 1;
  const before = JSON.stringify(generation);
  const result = queryGraph(generation, { query: "answer" }).find((node) => node.id === target.id);
  assert.ok(result);
  assert.equal(result.confidence, null);
  assert.equal(result.provenance, "AUTHORITATIVE_SEMANTIC");
  assert.equal(JSON.stringify(generation), before);
});

test("untagged V1 confidence and genuinely omitted defaults remain compatible", () => {
  const db = openStore(":memory:");
  try {
    const generation = fixture();
    for (const node of generation.nodes) { delete node.provenance; delete node.confidence; }
    generation.edges = generation.edges.map(({ provenance, ...edge }) => ({ ...edge, confidence: 0.4 }));
    saveGeneration(db, generation);
    assert.equal(getSymbol(db, "symbol:a.py::answer").confidence, 1);
    assert.ok(listEdges(db).every((edge) => edge.confidence === 0.4));
  } finally { closeStore(db); }
});
