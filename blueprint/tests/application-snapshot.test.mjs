import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { openStore, closeStore, saveGeneration, loadGeneration, getGenerationEnvelope } from "../src/graph/store-sqlite.mjs";
import { indexedResolve, boundedNeighbors } from "../src/graph/traverse-store.mjs";

// The source remains fixed here: this test isolates database snapshot coherence
// from the independent source-freshness gate. A second writer atomically changes
// rows/envelope while a reader session is held open over WAL.
test("application session keeps rows and envelope pinned across concurrent publication", async () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-read-snapshot-"));
  let session;
  let writer;
  try {
    writeFileSync(join(root, "work.ts"), "export function work() { return 1; }\n");
    buildGraphGeneration(root, { outDir: ".agent", persist: true });
    const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
    session = await service.openFreshnessSession({ repoRoot: root, allowStale: true });
    const before = loadGeneration(session.db);
    const id = "symbol:work.ts::work";
    const oldName = before.nodes.find((node) => node.id === id).name;
    writer = openStore(join(root, ".agent", "graph", "graph.db"));
    const next = { ...before,
      manifest: { ...before.manifest, generationId: "gen-next-snapshot" },
      nodes: before.nodes.map((node) => node.id === id ? { ...node, name: "replacement" } : node),
    };
    saveGeneration(writer, next);
    const oldResult = await service.resolve({ repoRoot: root, nodeId: id }, { session });
    assert.equal(oldResult.generationId, before.manifest.generationId);
    assert.equal(oldResult.node.name, oldName);
    assert.equal(getGenerationEnvelope(session.db).manifest.generationId, before.manifest.generationId);
    assert.equal(getGenerationEnvelope(writer).manifest.generationId, "gen-next-snapshot");
    session.close();
    session.close();
    assert.equal(session.closed, true);
    await assert.rejects(service.resolve({ repoRoot: root, nodeId: id }, { session }), (error) => error.code === "session_closed");
    const fresh = await service.resolve({ repoRoot: root, nodeId: id, allowStale: true });
    assert.equal(fresh.generationId, "gen-next-snapshot");
    assert.equal(fresh.node.name, "replacement");
  } finally {
    session?.close();
    if (writer) closeStore(writer);
    rmSync(root, { recursive: true, force: true });
  }
});

test("resolve does not label unknown, stale or dirty source state fresh", () => {
  const db = openStore(":memory:");
  try {
    const node = { id: "file:a.ts", kind: "file", path: "a.ts", name: "a.ts", qualifiedName: "a.ts", labels: ["File"], evidence: [] };
    saveGeneration(db, { schemaVersion: 1, provider: { id: "blueprint-static" },
      manifest: { generationId: "fresh-flags", complete: true, counts: { nodes: 1, edges: 0 } }, nodes: [node], edges: [] });
    for (const sourceState of [undefined, null, "unknown", "stale", "dirty_overlay", "unavailable"]) {
      assert.equal(indexedResolve(db, node.id, { sourceState }).node.fresh, false);
    }
    assert.equal(indexedResolve(db, node.id, { sourceState: "clean" }).node.fresh, true);
    assert.equal(boundedNeighbors(db, { nodeId: node.id }).sourceState, "unknown");
  } finally { closeStore(db); }
});
