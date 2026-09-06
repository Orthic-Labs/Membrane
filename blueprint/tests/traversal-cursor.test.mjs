import assert from "node:assert/strict";
import test from "node:test";
import { openStore, closeStore, saveGeneration } from "../src/graph/store-sqlite.mjs";
import { boundedNeighbors, boundedImpact, boundedPath, boundedArchitecture, indexedNeighbors, indexedPath } from "../src/graph/traverse-store.mjs";
import { selectTraversalPolicy } from "../src/graph/traversal-policy.mjs";

function fixture(generationId = "gen-cursors") {
  const nodes = Array.from({ length: 24 }, (_, i) => ({
    id: `symbol:src/n${i}.ts::work`, kind: "symbol", name: "work", qualifiedName: "work", path: `src/n${i}.ts`,
    labels: ["Symbol"], confidence: null, evidence: [{ path: `src/n${i}.ts`, startLine: 1, endLine: 1, contentHash: `h${i}` }],
  }));
  const edges = nodes.slice(1).map((node, i) => ({ id: `edge:${i}`, kind: "CALLS", source: node.id,
    target: nodes[i < 10 ? 0 : i].id, confidence: null, confidenceTier: "EXACT_RESOLUTION", evidence: [{ path: node.path }] }));
  return { schemaVersion: 1, provider: { id: "blueprint-static" }, repoRoot: "/repo/cursors", nodes, edges,
    manifest: { generationId, complete: true, counts: { nodes: nodes.length, edges: edges.length } } };
}
const modes = [
  ["neighbors", boundedNeighbors, { nodeId: "symbol:src/n0.ts::work", direction: "in", depth: 2 }, "nodes"],
  ["impact", boundedImpact, { nodeId: "symbol:src/n0.ts::work", depth: 2 }, "impacted"],
  ["path", boundedPath, { from: "symbol:src/n23.ts::work", to: "symbol:src/n10.ts::work", maxDepth: 15 }, "path"],
  ["architecture", boundedArchitecture, {}, "examples"],
];
const invalid = (error) => error.code === "cursor_invalid";

for (const [name, query, parameters, field] of modes) {
  test(`${name} continuation visits all rows exactly once and advances`, () => {
    const db = openStore(":memory:");
    try {
      saveGeneration(db, fixture());
      const full = query(db, { ...parameters, budget: 100000 });
      const rows = [], edges = [], seen = new Set();
      let cursor;
      for (let page = 0; page < 100; page += 1) {
        const result = query(db, { ...parameters, budget: 100, cursor });
        assert.equal(result.generationId, "gen-cursors");
        rows.push(...result[field]); edges.push(...(result.edges ?? []));
        if (!result.truncated) { assert.equal(result.continuationCursor, null); break; }
        cursor = result.continuationCursor;
        assert.ok(cursor && !seen.has(cursor), "continuation must make forward progress");
        seen.add(cursor);
      }
      assert.ok(seen.size > 0, "fixture must actually paginate");
      assert.deepEqual(rows, full[field]);
      assert.deepEqual(edges, full.edges ?? []);
      assert.equal(new Set(rows.map((row) => row.id)).size, rows.length);
    } finally { closeStore(db); }
  });

  test(`${name} rejects a cursor from different generation, parameters or malformed position`, () => {
    const db = openStore(":memory:");
    try {
      saveGeneration(db, fixture());
      const options = { ...parameters, budget: 100 };
      const cursor = query(db, options).continuationCursor;
      assert.ok(cursor);
      assert.throws(() => query(db, { ...options, budget: 101, cursor }), invalid);
      assert.throws(() => query(db, { ...options, cursor: "not-valid-json" }), invalid);
      const decoded = JSON.parse(Buffer.from(cursor, "base64url").toString());
      for (const position of [-1, 0.5, "1", 999999]) {
        const bad = Buffer.from(JSON.stringify({ ...decoded, node: position })).toString("base64url");
        assert.throws(() => query(db, { ...options, cursor: bad }), invalid);
      }
      const other = { ...decoded, kind: "other" };
      assert.throws(() => query(db, { ...options, cursor: Buffer.from(JSON.stringify(other)).toString("base64url") }), invalid);
      saveGeneration(db, fixture("next"));
      assert.throws(() => query(db, { ...options, cursor }), invalid);
      assert.throws(() => query(db, { ...options, cursor, freshness: { generationId: "gen-cursors" } }), invalid,
        "a caller-supplied old generation cannot hide the actual store generation");
    } finally { closeStore(db); }
  });
}

test("neighbor cursor cannot be replayed for another root or direction", () => {
  const db = openStore(":memory:");
  try {
    saveGeneration(db, fixture());
    const options = { nodeId: "symbol:src/n0.ts::work", direction: "in", depth: 2, budget: 100 };
    const cursor = boundedNeighbors(db, options).continuationCursor;
    assert.throws(() => boundedNeighbors(db, { ...options, cursor, direction: "both" }), invalid);
    assert.throws(() => boundedNeighbors(db, { ...options, cursor, nodeId: "symbol:src/n1.ts::work" }), invalid);
    assert.throws(() => boundedImpact(db, { ...options, cursor }), invalid);
  } finally { closeStore(db); }
});

// BPT-027: the RAW indexedNeighbors/indexedPath internal helpers must enforce
// their own ceiling in the no-generation ("rare — empty/in-memory") fallback
// branch, independent of the wrapping boundedPayload() token budget. These
// tests call the internal helpers DIRECTLY — never through boundedNeighbors
// / boundedPath — so a caller that bypasses the bounded wrapper still gets a
// bounded frontier walk.
function longChainFixture(length, generationId = "gen-chain") {
  const nodes = Array.from({ length }, (_, i) => ({
    id: `symbol:src/c${i}.ts::work`, kind: "symbol", name: "work", qualifiedName: "work", path: `src/c${i}.ts`,
    labels: ["Symbol"], confidence: null, evidence: [{ path: `src/c${i}.ts`, startLine: 1, endLine: 1, contentHash: `h${i}` }],
  }));
  const edges = nodes.slice(1).map((node, i) => ({
    id: `edge:${i}`, kind: "CALLS", source: nodes[i].id, target: node.id,
    confidence: null, confidenceTier: "EXACT_RESOLUTION", evidence: [{ path: node.path }],
  }));
  return { schemaVersion: 1, provider: { id: "blueprint-static" }, repoRoot: "/repo/chain", nodes, edges,
    manifest: { generationId, complete: true, counts: { nodes: nodes.length, edges: edges.length } } };
}

// Force the no-generation-envelope fallback path even though rows exist: drop
// `generationId` from the persisted manifest so indexedMeta() still resolves
// (meta is non-null, provider intact) but `meta.manifest.generationId` is
// falsy, which is exactly the condition indexedNeighbors/indexedPath test to
// pick the JS frontier-scan fallback over the SQL-indexed frontier.
function dropManifestGenerationId(db) {
  const row = db.prepare("SELECT value FROM generation WHERE key = 'manifest'").get();
  const manifest = JSON.parse(row.value);
  delete manifest.generationId;
  db.prepare("UPDATE generation SET value = ? WHERE key = 'manifest'").run(JSON.stringify(manifest));
}

const chainLimits = selectTraversalPolicy(null, "explore.both", {});

test("indexedNeighbors fallback frontier (no generation envelope) is ceiling-bound, not unbounded", () => {
  const db = openStore(":memory:");
  try {
    const length = chainLimits.maxNodes + 200;
    saveGeneration(db, longChainFixture(length));
    dropManifestGenerationId(db);
    const result = indexedNeighbors(db, {
      nodeId: "symbol:src/c0.ts::work", direction: "out", depth: length,
    });
    assert.ok(result.nodes.length <= chainLimits.maxNodes,
      `expected fallback frontier to respect maxNodes=${chainLimits.maxNodes}, got ${result.nodes.length}`);
    assert.ok(result.edges.length <= chainLimits.maxEdges,
      `expected fallback frontier to respect maxEdges=${chainLimits.maxEdges}, got ${result.edges.length}`);
    assert.equal(result.truncated, true, "ceiling must be reported as truncated, not silently dropped");
    assert.ok(Array.isArray(result.omissions) && result.omissions.length > 0 && result.omissions[0].reason === "bound_reached",
      "ceiling must be reported as a typed omission");
  } finally { closeStore(db); }
});

test("indexedPath fallback frontier (no generation envelope) is ceiling-bound, not unbounded", () => {
  const db = openStore(":memory:");
  try {
    const length = chainLimits.maxNodes + 200;
    saveGeneration(db, longChainFixture(length));
    dropManifestGenerationId(db);
    const result = indexedPath(db, {
      from: "symbol:src/c0.ts::work", to: `symbol:src/c${length - 1}.ts::work`, maxDepth: length,
    });
    // The target is genuinely reachable but only by walking past the ceiling;
    // an unbounded fallback would find it, a bounded one must not.
    assert.equal(result.found, false);
    assert.ok(Array.isArray(result.omissions) && result.omissions.length > 0 && result.omissions[0].reason === "bound_reached",
      "ceiling exhaustion must be reported as a typed omission, matching the bounded surfaces' reporting shape");
  } finally { closeStore(db); }
});
