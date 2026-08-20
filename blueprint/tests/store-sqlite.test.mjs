import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  SCHEMA_VERSION,
  openStore,
  closeStore,
  migrate,
  getSchemaVersion,
  bulkInsertGeneration,
  countRows,
  getFile,
  getSymbol,
  listSymbolsByPath,
  listEdges,
  blastRadius,
  traversalNeighbors,
  upsertVectors,
  searchSimilar,
  searchGenerationSymbols,
  countVectors,
  saveGeneration,
  loadGeneration,
  hydrateNodesByIds,
} from "../src/graph/store-sqlite.mjs";
import { computeGenerationId } from "../src/graph/generation-identity.mjs";

function sampleGeneration() {
  return {
    manifest: { generationId: "gen-1" },
    nodes: [
      fileNode("a.ts", "h-a"),
      fileNode("b.ts", "h-b"),
      fileNode("c.ts", "h-c"),
      fileNode("d.ts", "h-d"),
      symNode("a.ts", "foo", ["Function"]),
      symNode("a.ts", "Widget.render", ["Method"]),
    ],
    edges: [
      importEdge("b.ts", "a.ts"),
      importEdge("c.ts", "b.ts"),
      importEdge("d.ts", "c.ts"),
      { id: "edge:CONTAINS:file:a.ts->symbol:a.ts::foo", kind: "CONTAINS", source: "file:a.ts", target: "symbol:a.ts::foo", confidence: 1, resolved: true, evidence: [] },
    ],
    fileReports: [
      { path: "a.ts", language: "typescript", provider: "cortex-treesitter", parseStatus: "ok", errorNodeCount: 0 },
      { path: "b.ts", language: "typescript", provider: "cortex-treesitter", parseStatus: "ok", errorNodeCount: 0 },
    ],
  };
}

function fileNode(path, hash) {
  return { id: `file:${path}`, kind: "file", labels: ["File"], name: path, qualifiedName: path, path, confidence: 1, evidence: [{ path, startLine: 1, endLine: 3, contentHash: hash }] };
}
function symNode(path, qualifiedName, labels) {
  return { id: `symbol:${path}::${qualifiedName}`, kind: "symbol", labels, name: qualifiedName.split(".").at(-1), qualifiedName, path, confidence: 1, evidence: [{ path, startLine: 1, endLine: 2, contentHash: "h" }] };
}
function importEdge(fromPath, toPath) {
  return { id: `edge:IMPORTS:file:${fromPath}->file:${toPath}`, kind: "IMPORTS", source: `file:${fromPath}`, target: `file:${toPath}`, confidence: 1, resolved: true, specifier: `./${toPath}`, evidence: [] };
}

test("openStore creates schema and migrate() is idempotent", () => {
  const db = openStore(":memory:");
  try {
    assert.equal(getSchemaVersion(db), SCHEMA_VERSION);
    const before = countRows(db);
    assert.deepEqual(before, { files: 0, symbols: 0, annotations: 0, edges: 0 });
    // Re-running migrate must not throw and must not change the version.
    const version = migrate(db);
    assert.equal(version, SCHEMA_VERSION);
    const search = db.prepare("SELECT sql FROM sqlite_master WHERE name='symbol_search'").get();
    assert.match(search.sql, /VIRTUAL TABLE symbol_search USING fts5\(id UNINDEXED, generation_id UNINDEXED, name, qualified_name, path\)/);
    const terms = db.prepare("SELECT sql FROM sqlite_master WHERE name='symbol_terms'").get();
    assert.match(terms.sql, /CREATE TABLE symbol_terms/);
    assert.ok(db.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='node_provider'").get());
  } finally {
    closeStore(db);
  }
});

test("bulk insert in a transaction round-trips files/symbols/edges exactly", () => {
  const db = openStore(":memory:");
  try {
    const generation = sampleGeneration();
    const result = bulkInsertGeneration(db, generation);
    assert.equal(result.generationId, "gen-1");
    assert.equal(result.fileCount, 4);
    assert.equal(result.symbolCount, 2);
    assert.equal(result.edgeCount, 4);

    const counts = countRows(db);
    assert.deepEqual(counts, { files: 4, symbols: 2, annotations: 0, edges: 4 });

    const fileRow = getFile(db, "a.ts");
    assert.equal(fileRow.content_hash, "h-a");
    assert.equal(fileRow.language, "typescript");
    assert.equal(fileRow.parse_status, "ok");
    assert.equal(fileRow.generation_id, "gen-1");

    const symbolRow = getSymbol(db, "symbol:a.ts::foo");
    assert.deepEqual(symbolRow.labels, ["Function"]);
    assert.equal(symbolRow.qualifiedName, "foo");
    assert.equal(symbolRow.confidence, 1);
    assert.equal(symbolRow.generationId, "gen-1");

    const bySymbolPath = listSymbolsByPath(db, "a.ts");
    assert.equal(bySymbolPath.length, 2);

    const matched = searchGenerationSymbols(db, "gen-1", ["widget"], 4);
    assert.equal(matched.length, 1);
    assert.equal(matched[0].qualifiedName, "Widget.render");
    assert.equal(matched[0].generationId, "gen-1");
    assert.match(matched[0].evidence, /a\.ts/);
    const fallback = searchGenerationSymbols(db, "gen-1", ["no_such_symbol"], 4);
    assert.equal(fallback.length, 2, "valid generation gets deterministic indexed fallback");
    assert.ok(db.prepare("SELECT 1 FROM symbol_terms WHERE generation_id='gen-1' AND token='*' LIMIT 1").get());

    const importEdges = listEdges(db, { kind: "IMPORTS" });
    assert.equal(importEdges.length, 3);
    assert.ok(importEdges.every((e) => e.generationId === "gen-1"));
  } finally {
    closeStore(db);
  }
});

test("annotation and dense-node migrations upgrade prior schemas", () => {
  const beforeAnnotations = 14;
  const annotationsVersion = 15;
  const db = openStore(":memory:", { upToVersion: beforeAnnotations });
  try {
    assert.equal(getSchemaVersion(db), beforeAnnotations);
    assert.equal(db.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='annotation_nodes'").get(), undefined);
    assert.equal(migrate(db, { upToVersion: annotationsVersion }), annotationsVersion);
    assert.ok(db.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='annotation_nodes'").get());
    assert.equal(db.prepare("PRAGMA table_info(files)").all().some((column) => column.name === "node_ordinal"), false);
    assert.equal(migrate(db), SCHEMA_VERSION);
    assert.ok(db.prepare("PRAGMA table_info(files)").all().some((column) => column.name === "node_ordinal"));
    assert.ok(db.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='provider_ranks'").get());
  } finally {
    closeStore(db);
  }
});

test("annotation nodes round-trip full payload in ordinal order outside symbols", () => {
  const db = openStore(":memory:");
  const annotations = [
    { id: "comment:a.ts:1", kind: "comment", labels: ["Comment"], name: null, qualifiedName: null, path: "a.ts", confidence: 0.5, evidence: null, text: null, provider: null, factProvider: null },
    { id: "comment:a.ts:2", kind: "comment", labels: ["Comment"], name: null, qualifiedName: null, path: "a.ts", confidence: 1, evidence: [{ path: "a.ts", startLine: 4, endLine: 4, contentHash: "h-a" }], text: "// retained", provider: { id: "cortex-treesitter" } },
  ];
  const generation = { ...sampleGeneration(), manifest: { generationId: "gen-comments" }, nodes: [...sampleGeneration().nodes, ...annotations] };
  try {
    const summary = saveGeneration(db, generation, { populateState: true });
    assert.deepEqual({ files: summary.fileCount, symbols: summary.symbolCount, annotations: summary.annotationCount }, { files: 4, symbols: 2, annotations: 2 });
    assert.deepEqual(countRows(db), { files: 4, symbols: 2, annotations: 2, edges: 4 });
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM symbols WHERE id LIKE 'comment:%'").get().n, 0);
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM fact_owner WHERE fact_id LIKE 'comment:%'").get().n, 2);
    assert.equal(loadGeneration(db).manifest.generationId, "gen-comments");
    assert.deepEqual(loadGeneration(db).nodes, generation.nodes);
    assert.deepEqual(hydrateNodesByIds(db, annotations.map((node) => node.id).reverse()), annotations);
    saveGeneration(db, { ...generation, nodes: sampleGeneration().nodes }, { populateState: true });
    assert.equal(countRows(db).annotations, 0);
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM fact_owner WHERE fact_id LIKE 'comment:%'").get().n, 0);
  } finally {
    closeStore(db);
  }
});

test("interleaved symbols and annotations retain node order and generation identity", () => {
  const db = openStore(":memory:");
  const first = symNode("a.ts", "first", ["Function"]);
  const comment = { id: "comment:a.ts:1", kind: "comment", labels: ["Comment"], name: null, qualifiedName: null, path: "a.ts", confidence: 1, evidence: [], text: "// between" };
  const second = symNode("a.ts", "second", ["Function"]);
  const nodes = [fileNode("a.ts", "h-a"), first, comment, second];
  const sourceHash = "source-order";
  const generation = { manifest: { generationId: computeGenerationId(nodes, [], sourceHash) }, nodes, edges: [] };
  try {
    saveGeneration(db, generation);
    const loaded = loadGeneration(db);
    assert.deepEqual(loaded.nodes, nodes);
    assert.deepEqual(hydrateNodesByIds(db, nodes.map((node) => node.id).reverse()), nodes);
    assert.equal(computeGenerationId(loaded.nodes, loaded.edges, sourceHash), generation.manifest.generationId);
  } finally {
    closeStore(db);
  }
});

test("save/load/hydrate preserve arbitrary dense interleave across every node table", () => {
  const db = openStore(":memory:");
  const a = { ...fileNode("a.ts", "h-a"), factProvider: { id: "lexical", version: "test" } };
  const b = { ...fileNode("b.ts", "h-b"), factProvider: { id: "custom", version: "test" } };
  const symbol = { ...symNode("a.ts", "alpha", ["Function"]), factProvider: { id: "lexical", version: "test" } };
  const comment = { id: "comment:a.ts:1", kind: "comment", labels: ["Comment"], name: null, qualifiedName: null, path: "a.ts", confidence: 1, evidence: [], text: "// exact", factProvider: { id: "treesitter", version: "test" } };
  const nodes = [comment, b, symbol, a];
  try {
    saveGeneration(db, { manifest: { generationId: "gen-dense-union" }, nodes, edges: [] });
    assert.deepEqual(loadGeneration(db).nodes, nodes);
    assert.deepEqual(hydrateNodesByIds(db, [...nodes].reverse().map((node) => node.id)), nodes);
    assert.deepEqual(db.prepare("SELECT node_ordinal FROM files UNION ALL SELECT node_ordinal FROM symbols UNION ALL SELECT node_ordinal FROM annotation_nodes ORDER BY node_ordinal").all().map((row) => row.node_ordinal), [0, 1, 2, 3]);
    assert.deepEqual(db.prepare("SELECT provider_id, rank FROM provider_ranks ORDER BY rank").all().map((row) => ({ ...row })), [{ provider_id: "lexical", rank: 0 }, { provider_id: "treesitter", rank: 1 }, { provider_id: "doctruth", rank: 2 }, { provider_id: "custom", rank: 3 }]);
    assert.deepEqual(db.prepare("SELECT node_id, provider_id, source_path FROM node_provider ORDER BY node_id").all().map((row) => ({ ...row })), [
      { node_id: "comment:a.ts:1", provider_id: "treesitter", source_path: "a.ts" },
      { node_id: "file:a.ts", provider_id: "lexical", source_path: "a.ts" },
      { node_id: "file:b.ts", provider_id: "custom", source_path: "b.ts" },
      { node_id: "symbol:a.ts::alpha", provider_id: "lexical", source_path: "a.ts" },
    ]);
    saveGeneration(db, { manifest: { generationId: "gen-provider-removed" }, nodes: [a], edges: [] });
    assert.deepEqual(db.prepare("SELECT provider_id, rank FROM provider_ranks ORDER BY rank").all().map((row) => ({ ...row })), [{ provider_id: "lexical", rank: 0 }, { provider_id: "treesitter", rank: 1 }, { provider_id: "doctruth", rank: 2 }]);
  } finally {
    closeStore(db);
  }
});

test("bulkInsertGeneration rejects append before mutating the single-current store", () => {
  const db = openStore(":memory:");
  try {
    bulkInsertGeneration(db, sampleGeneration(), { generationId: "gen-1" });
    assert.equal(countRows(db).files, 4);

    // A second generation with the SAME row ids should replace, not accumulate.
    const gen2 = sampleGeneration();
    bulkInsertGeneration(db, gen2, { generationId: "gen-2", mode: "replace" });
    const counts = countRows(db);
    assert.equal(counts.files, 4, "replace mode must not accumulate duplicate rows across generations");
    assert.equal(getFile(db, "a.ts").generation_id, "gen-2");

    const before = db.prepare("SELECT path, generation_id FROM files ORDER BY path").all();
    const extra = { nodes: [fileNode("e.ts", "h-e")], edges: [], fileReports: [] };
    assert.throws(() => bulkInsertGeneration(db, extra, { generationId: "gen-2", mode: "append" }), (error) => error.code === "store_append_unsupported");
    assert.deepEqual(db.prepare("SELECT path, generation_id FROM files ORDER BY path").all(), before);
  } finally {
    closeStore(db);
  }
});

test("saveGeneration rejects append and cross-table duplicate node IDs atomically", () => {
  const db = openStore(":memory:");
  try {
    saveGeneration(db, sampleGeneration());
    const before = JSON.stringify(loadGeneration(db));
    assert.throws(() => saveGeneration(db, { nodes: [fileNode("z.ts", "z")], edges: [] }, { mode: "append" }), (error) => error.code === "store_append_unsupported");
    const duplicate = { id: "same", kind: "comment", labels: [], name: null, qualifiedName: null, path: "a.ts", confidence: 1, evidence: [] };
    assert.throws(() => saveGeneration(db, { manifest: { generationId: "bad" }, nodes: [{ ...fileNode("a.ts", "h"), id: "same" }, duplicate], edges: [] }), (error) => error.code === "duplicate_node_id");
    assert.throws(() => saveGeneration(db, { manifest: { generationId: "bad-path" }, nodes: [fileNode("same.ts", "1"), { ...fileNode("same.ts", "2"), id: "file:other" }], edges: [] }), (error) => error.code === "path_normalization_collision");
    assert.equal(JSON.stringify(loadGeneration(db)), before);
  } finally { closeStore(db); }
});

test("indexes exist on edges(source), edges(target), edges(kind)", () => {
  const db = openStore(":memory:");
  try {
    const rows = db.prepare("SELECT name, tbl_name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'edges'").all();
    const names = rows.map((r) => r.name);
    assert.ok(names.some((n) => n.includes("source")));
    assert.ok(names.some((n) => n.includes("target")));
    assert.ok(names.some((n) => n.includes("kind")));
  } finally {
    closeStore(db);
  }
});

test("compound indexes for indexed traversal exist after migration 13", () => {
  const db = openStore(":memory:");
  try {
    const rows = db.prepare("SELECT name, sql FROM sqlite_master WHERE type = 'index' AND tbl_name = 'edges'").all();
    const names = rows.map((r) => r.name);
    assert.ok(names.includes("idx_edges_gen_source_kind"), "expected (generation_id, source, kind) compound index");
    assert.ok(names.includes("idx_edges_gen_target_kind"), "expected (generation_id, target, kind) compound index");
    const sourceIndex = rows.find((r) => r.name === "idx_edges_gen_source_kind");
    assert.match(sourceIndex.sql, /generation_id.*source.*kind/i);
  } finally {
    closeStore(db);
  }
});

test("traversalNeighbors expands the frontier in SQL, bounded by generation_id and depth", () => {
  const db = openStore(":memory:");
  try {
    bulkInsertGeneration(db, sampleGeneration());
    // sampleGeneration is gen-1: edges are
    //   b.ts IMPORTS a.ts    (source=b.ts, target=a.ts)
    //   c.ts IMPORTS b.ts    (source=c.ts, target=b.ts)
    //   d.ts IMPORTS c.ts    (source=d.ts, target=c.ts)
    //   file:a.ts CONTAINS symbol:a.ts::foo
    //
    // Direction "out" walks edges where edge.source is in the frontier ->
    // edge.target is new. Direction "in" walks edges where edge.target is in
    // the frontier -> edge.source is new.
    //
    // So from a.ts, going "out" via IMPORTS reaches only a.ts (no outgoing
    // IMPORTS edges). Going "in" via IMPORTS from a.ts at maxDepth=1 reaches
    // the seed + b.ts (the 1-hop in-neighbor); maxDepth=2 reaches c.ts;
    // maxDepth=3 reaches d.ts.
    const in0 = traversalNeighbors(db, { seedIds: ["file:a.ts"], maxDepth: 0, direction: "in", generationId: "gen-1", kinds: ["IMPORTS"] });
    assert.deepEqual(in0.seenNodes, ["file:a.ts"]);
    const in1 = traversalNeighbors(db, { seedIds: ["file:a.ts"], maxDepth: 1, direction: "in", generationId: "gen-1", kinds: ["IMPORTS"] });
    assert.deepEqual(in1.seenNodes.sort(), ["file:a.ts", "file:b.ts"]);
    assert.equal(in1.depths.get("file:b.ts"), 1);
    const in3 = traversalNeighbors(db, { seedIds: ["file:a.ts"], maxDepth: 3, direction: "in", generationId: "gen-1", kinds: ["IMPORTS"] });
    assert.deepEqual(in3.seenNodes.sort(), ["file:a.ts", "file:b.ts", "file:c.ts", "file:d.ts"]);
    assert.equal(in3.depths.get("file:d.ts"), 3);
    // Direction "both" from d.ts at depth 1 reaches c.ts only.
    const both1 = traversalNeighbors(db, { seedIds: ["file:d.ts"], maxDepth: 1, direction: "both", generationId: "gen-1", kinds: ["IMPORTS"] });
    assert.deepEqual(both1.seenNodes.sort(), ["file:c.ts", "file:d.ts"]);
    // No generation_id -> early return (not a crash).
    assert.deepEqual(traversalNeighbors(db, { seedIds: ["file:a.ts"], maxDepth: 1 }).seenNodes, []);
  } finally {
    closeStore(db);
  }
});

test("traversalNeighbors honours the kind filter", () => {
  const db = openStore(":memory:");
  try {
    // Build a generation with a mix of edge kinds so the kind filter has
    // something to do. generationId is explicit so the test does not rely on
    // the bulkInsertGeneration "unknown" default.
    const generation = {
      manifest: { generationId: "gen-kinds" },
      nodes: [
        fileNode("a.ts", "h-a"),
        fileNode("b.ts", "h-b"),
        symNode("a.ts", "foo", ["Function"]),
      ],
      edges: [
        importEdge("b.ts", "a.ts"),
        { id: "edge:CONTAINS:file:a.ts->symbol:a.ts::foo", kind: "CONTAINS", source: "file:a.ts", target: "symbol:a.ts::foo", confidence: 1, resolved: true, evidence: [] },
      ],
      fileReports: [],
    };
    bulkInsertGeneration(db, generation);
    const onlyImports = traversalNeighbors(db, {
      seedIds: ["file:a.ts"],
      maxDepth: 2,
      direction: "both",
      generationId: "gen-kinds",
      kinds: ["IMPORTS"],
    });
    // With kinds=["IMPORTS"], the CONTAINS edge to the symbol must NOT be in
    // the visited set; the symbol must not be visited either.
    assert.ok(!onlyImports.seenNodes.includes("symbol:a.ts::foo"));
    const both = traversalNeighbors(db, {
      seedIds: ["file:a.ts"],
      maxDepth: 2,
      direction: "both",
      generationId: "gen-kinds",
    });
    assert.ok(both.seenNodes.includes("symbol:a.ts::foo"));
  } finally {
    closeStore(db);
  }
});

// ---------------------------------------------------------------------------
// Recursive-CTE blast radius — hand-built graph with a KNOWN expected answer.
//
// Chain: a.ts <- b.ts <- c.ts <- d.ts   (arrow = "imports", so an edge is
// stored as IMPORTS source=importer target=imported). Changing a.ts should
// transitively affect b.ts (depth 1), c.ts (depth 2), d.ts (depth 3) — i.e.
// walking "who points at this" backward from a.ts along IMPORTS edges.
// ---------------------------------------------------------------------------

test("blastRadius returns the exact transitive dependent set with correct depths", () => {
  const db = openStore(":memory:");
  try {
    bulkInsertGeneration(db, sampleGeneration());
    const radius = blastRadius(db, { changedNodeIds: ["file:a.ts"], maxDepth: 5 });
    const byId = new Map(radius.map((r) => [r.nodeId, r]));

    assert.equal(byId.size, 4, "expected a.ts (seed) + b.ts + c.ts + d.ts, nothing else");
    assert.equal(byId.get("file:a.ts").depth, 0);
    assert.equal(byId.get("file:a.ts").isSeed, true);
    assert.equal(byId.get("file:b.ts").depth, 1);
    assert.equal(byId.get("file:b.ts").isSeed, false);
    assert.equal(byId.get("file:c.ts").depth, 2);
    assert.equal(byId.get("file:d.ts").depth, 3);
  } finally {
    closeStore(db);
  }
});

test("blastRadius respects maxDepth (bounded traversal, not full closure)", () => {
  const db = openStore(":memory:");
  try {
    bulkInsertGeneration(db, sampleGeneration());
    const radius = blastRadius(db, { changedNodeIds: ["file:a.ts"], maxDepth: 1 });
    const ids = radius.map((r) => r.nodeId).sort();
    assert.deepEqual(ids, ["file:a.ts", "file:b.ts"]);
  } finally {
    closeStore(db);
  }
});

test("blastRadius handles diamond dependencies with correct MIN depth per node (not double-counted)", () => {
  const db = openStore(":memory:");
  try {
    // diamond: root <- left <- top, root <- right <- top
    const generation = {
      nodes: [fileNode("root.ts", "h1"), fileNode("left.ts", "h2"), fileNode("right.ts", "h3"), fileNode("top.ts", "h4")],
      edges: [
        importEdge("left.ts", "root.ts"),
        importEdge("right.ts", "root.ts"),
        importEdge("top.ts", "left.ts"),
        importEdge("top.ts", "right.ts"),
      ],
      fileReports: [],
    };
    bulkInsertGeneration(db, generation);
    const radius = blastRadius(db, { changedNodeIds: ["file:root.ts"], maxDepth: 10 });
    assert.equal(radius.length, 4, "top.ts must appear exactly once despite two paths reaching it");
    const top = radius.find((r) => r.nodeId === "file:top.ts");
    assert.equal(top.depth, 2, "top.ts is reachable at depth 2 via either path; MIN must win");
  } finally {
    closeStore(db);
  }
});

test("blastRadius on an empty changed set returns an empty array, not an error", () => {
  const db = openStore(":memory:");
  try {
    bulkInsertGeneration(db, sampleGeneration());
    assert.deepEqual(blastRadius(db, { changedNodeIds: [], maxDepth: 5 }), []);
  } finally {
    closeStore(db);
  }
});

test("blastRadius on a node with no dependents returns just the seed at depth 0", () => {
  const db = openStore(":memory:");
  try {
    bulkInsertGeneration(db, sampleGeneration());
    const radius = blastRadius(db, { changedNodeIds: ["file:d.ts"], maxDepth: 5 });
    assert.equal(radius.length, 1);
    assert.equal(radius[0].nodeId, "file:d.ts");
    assert.equal(radius[0].depth, 0);
  } finally {
    closeStore(db);
  }
});

test("store is regenerable: a fresh openStore on a new file has zero rows until populated", () => {
  const dir = mkdtempSync(join(tmpdir(), "cortex-store-sqlite-"));
  const dbPath = join(dir, "graph.sqlite");
  try {
    const db = openStore(dbPath);
    assert.deepEqual(countRows(db), { files: 0, symbols: 0, annotations: 0, edges: 0 });
    bulkInsertGeneration(db, sampleGeneration());
    assert.equal(countRows(db).files, 4);
    closeStore(db);

    // Re-opening the same path picks up the persisted rows — proves it is a
    // real regenerable cache file, not an in-memory-only illusion.
    const reopened = openStore(dbPath);
    assert.equal(countRows(reopened).files, 4);
    closeStore(reopened);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// --- optional embedding layer -------------------------------------------------

test("searchSimilar on an empty vectors table returns [] rather than throwing", () => {
  const db = openStore(":memory:");
  try {
    migrate(db);
    assert.equal(countVectors(db), 0);
    const out = searchSimilar(db, [1, 0, 0]);
    assert.deepEqual(out.results, []);
    assert.equal(out.scanned, 0);
  } finally {
    closeStore(db);
  }
});

test("vectors round-trip through the BLOB exactly", () => {
  const db = openStore(":memory:");
  try {
    migrate(db);
    const v = Float32Array.from([0.5, -0.25, 0.125, 1]);
    upsertVectors(db, "gen-1", [{ nodeId: "symbol:a", vector: v }], { model: "test-model" });
    assert.equal(countVectors(db), 1);
    // An identical vector must score ~1.0, which only holds if the bytes — and the
    // BLOB's byteOffset — survived intact.
    const { results } = searchSimilar(db, v);
    assert.equal(results.length, 1);
    assert.equal(results[0].nodeId, "symbol:a");
    assert.ok(Math.abs(results[0].score - 1) < 1e-6, `expected ~1.0, got ${results[0].score}`);
    assert.equal(results[0].model, "test-model");
  } finally {
    closeStore(db);
  }
});

test("searchSimilar ranks by cosine with a known answer: identical > orthogonal > opposite", () => {
  const db = openStore(":memory:");
  try {
    migrate(db);
    upsertVectors(db, "gen-1", [
      { nodeId: "symbol:same", vector: [1, 0, 0] },
      { nodeId: "symbol:orthogonal", vector: [0, 1, 0] },
      { nodeId: "symbol:opposite", vector: [-1, 0, 0] },
    ]);
    const { results } = searchSimilar(db, [1, 0, 0]);
    assert.deepEqual(results.map((r) => r.nodeId), ["symbol:same", "symbol:orthogonal", "symbol:opposite"]);
    assert.ok(Math.abs(results[0].score - 1) < 1e-6);
    assert.ok(Math.abs(results[1].score - 0) < 1e-6);
    assert.ok(Math.abs(results[2].score + 1) < 1e-6);
  } finally {
    closeStore(db);
  }
});

test("a stored vector whose dim differs from the query is skipped, not wrongly compared", () => {
  const db = openStore(":memory:");
  try {
    migrate(db);
    upsertVectors(db, "gen-1", [
      { nodeId: "symbol:ok", vector: [1, 0, 0] },
      { nodeId: "symbol:wrongdim", vector: [1, 0, 0, 0, 0] },
    ]);
    const out = searchSimilar(db, [1, 0, 0]);
    assert.equal(out.dimMismatches, 1);
    assert.deepEqual(out.results.map((r) => r.nodeId), ["symbol:ok"]);
  } finally {
    closeStore(db);
  }
});

test("limit and minScore are both respected", () => {
  const db = openStore(":memory:");
  try {
    migrate(db);
    upsertVectors(db, "gen-1", [
      { nodeId: "symbol:a", vector: [1, 0] },
      { nodeId: "symbol:b", vector: [0.9, 0.1] },
      { nodeId: "symbol:c", vector: [0, 1] },
      { nodeId: "symbol:d", vector: [-1, 0] },
    ]);
    assert.equal(searchSimilar(db, [1, 0], { limit: 2 }).results.length, 2);
    const filtered = searchSimilar(db, [1, 0], { minScore: 0.5 }).results;
    assert.ok(filtered.every((r) => r.score >= 0.5), "minScore must exclude lower matches");
    assert.ok(!filtered.some((r) => r.nodeId === "symbol:d"), "opposite vector must be excluded");
  } finally {
    closeStore(db);
  }
});

test("upsertVectors replaces an existing node_id rather than duplicating it", () => {
  const db = openStore(":memory:");
  try {
    migrate(db);
    upsertVectors(db, "gen-1", [{ nodeId: "symbol:a", vector: [1, 0] }]);
    upsertVectors(db, "gen-2", [{ nodeId: "symbol:a", vector: [0, 1] }]);
    assert.equal(countVectors(db), 1);
    const { results } = searchSimilar(db, [0, 1]);
    assert.ok(Math.abs(results[0].score - 1) < 1e-6, "the newer vector must be the one stored");
  } finally {
    closeStore(db);
  }
});
