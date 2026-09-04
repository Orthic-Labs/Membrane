import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { openStore, openStoreReadOnly, closeStore, saveGeneration, searchGenerationSymbols } from "../src/graph/store-sqlite.mjs";
import { indexedQueryGeneration } from "../src/graph/traverse-store.mjs";
import { resolveSeeds } from "../src/graph/seed-resolver.mjs";
import { queryGraph } from "../src/graph/static-provider.mjs";
import { symbolAuthorityOrder } from "../src/graph/symbol-authority-order.mjs";

function fixture() {
  const node = (id, provenance, confidence) => ({
    id, kind: "symbol", labels: ["Symbol", "Function"],
    name: "work", qualifiedName: "work", path: `${id}.ts`,
    provenance, confidence,
    evidence: [{ path: `${id}.ts`, startLine: 1, endLine: 1, contentHash: id }],
  });
  const nodes = Array.from({ length: 40 }, (_, i) => node(`a-${String(i).padStart(2, "0")}`, "HEURISTIC_BRIDGE", 0.999));
  nodes.push(node("z-compiler", "AUTHORITATIVE_SEMANTIC", null));
  return { schemaVersion: 1, provider: { id: "blueprint-static" }, nodes, edges: [],
    manifest: { generationId: "authority-selection", complete: true, counts: { nodes: nodes.length, edges: 0 } } };
}

for (const readOnly of [false, true]) {
  test(`bounded selection retains authoritative null-confidence facts (${readOnly ? "read-only" : "writer"})`, () => {
    const root = mkdtempSync(join(tmpdir(), "blueprint-authority-pool-"));
    const path = join(root, "graph.db");
    let db = openStore(path);
    try {
      const generation = fixture();
      saveGeneration(db, generation);
      if (readOnly) { closeStore(db); db = openStoreReadOnly(path); }
      const rows = searchGenerationSymbols(db, generation.manifest.generationId, ["work"], 1);
      assert.equal(rows[0].id, "z-compiler", "SQL LIMIT must run after categorical authority");
      assert.equal(rows[0].confidence, null);
      const pool = indexedQueryGeneration(db, "work", { limit: 1 });
      assert.ok(pool.nodes.some((node) => node.id === "z-compiler"));
      const publicResults = queryGraph(pool, { query: "work", limit: 1 });
      assert.equal(publicResults[0].id, "z-compiler", "final lexical tie-break must not discard the retained fact");
      assert.equal(publicResults[0].confidence, null);
      const seeds = resolveSeeds(db, "work", { generationId: generation.manifest.generationId, maxSeeds: 1 });
      assert.equal(seeds.state, "ambiguous", "authority must not invent uniqueness among distinct entities");
      assert.ok(seeds.candidates.some((candidate) => candidate.id === "z-compiler"));
    } finally { closeStore(db); rmSync(root, { recursive: true, force: true }); }
  });
}

test("in-memory search and SQL search agree on authority without rewriting input", () => {
  const generation = fixture();
  const before = JSON.stringify(generation);
  assert.equal(queryGraph(generation, { query: "work", limit: 1 })[0].id, "z-compiler");
  assert.equal(JSON.stringify(generation), before);
});

test("symbol SQL authority uses the central classifier and rejects unsafe aliases", () => {
  const db = openStore(":memory:");
  try {
    assert.throws(() => symbolAuthorityOrder(db, "s; DROP TABLE symbols"), TypeError);
    const rank = symbolAuthorityOrder(db);
    const classify = (extra) => db.prepare(`SELECT ${rank} AS rank FROM (SELECT ? AS extra)`).get(extra).rank;
    assert.ok(classify(JSON.stringify({ provenance: "AUTHORITATIVE_SEMANTIC" })) < classify(JSON.stringify({ provenance: "HEURISTIC_BRIDGE" })));
    assert.ok(classify(JSON.stringify({ provider: "scip-python", resolved: false })) > classify(JSON.stringify({ provenance: "STRUCTURAL_RESOLVED" })));
    assert.equal(classify("not json"), classify(null));
  } finally { closeStore(db); }
});

test("legacy scalar order is compatible only after categorical authority", () => {
  const db = openStore(":memory:");
  try {
    const generation = fixture();
    const compiler = generation.nodes.at(-1);
    const legacy = generation.nodes.slice(0, 2).map(({ provenance, ...node }, index) => ({ ...node, confidence: index === 0 ? 0.1 : 0.9 }));
    generation.nodes = [legacy[0], legacy[1], compiler];
    generation.manifest.counts.nodes = generation.nodes.length;
    saveGeneration(db, generation);
    assert.deepEqual(searchGenerationSymbols(db, generation.manifest.generationId, ["work"], 3).map((row) => row.id),
      [compiler.id, legacy[1].id, legacy[0].id]);
    assert.equal(searchGenerationSymbols(db, generation.manifest.generationId, ["work"], 1)[0].confidence, null);
  } finally { closeStore(db); }
});
