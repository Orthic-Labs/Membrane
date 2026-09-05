import assert from "node:assert/strict";
import test from "node:test";
import { openStore } from "../src/graph/store-sqlite.mjs";
import { migrateNullableDocTruthConfidence } from "../src/graph/confidence-migration.mjs";
import { projectDocumentTruth } from "../src/graph/doc-truth-projection.mjs";

const edge = (kind, confidenceClass = "DETERMINISTIC_EXTRACTION", confidence = null) => ({
  kind, source: "doc:a", target: "file:src/a.ts", confidenceClass, confidence,
  reason: `${kind} reason`, evidenceDocPath: "docs/a.md", evidenceDocLine: 4,
  evidenceDocSha1: "doc-hash", evidenceCodePath: "src/a.ts", evidenceCodeNodeId: "file:src/a.ts",
  evidenceCodeContentHash: "code-hash",
});
const claim = (id, edges) => ({ id, documentId: "doc:a", source: "docs/a.md", line: 4, status: "implemented", sha1: "doc-hash", edges });

test("doc truth preserves declaration and observed evidence with deterministic null confidence", () => {
  const projection = projectDocumentTruth({ claims: [claim("c1", [edge("supports")])], generationId: "g1", freshness: "fresh" });
  assert.equal(projection.claims[0].grounding, "direct");
  assert.equal(projection.claims[0].declared.status, "implemented");
  assert.equal(projection.claims[0].observed[0].confidence, null);
  assert.equal(projection.claims[0].mismatch.present, false);
  assert.deepEqual(projection.claims[0].citations.map((c) => c.kind), ["document", "code"]);
});

test("doc truth emits contradicted, ambiguous, unsupported and stale rather than guessing", () => {
  assert.equal(projectDocumentTruth({ claims: [claim("c", [edge("contradicts")])], freshness: "fresh" }).claims[0].grounding, "contradicted");
  assert.equal(projectDocumentTruth({ claims: [claim("c", [edge("supports"), edge("contradicts")])], freshness: "fresh" }).claims[0].grounding, "ambiguous");
  assert.equal(projectDocumentTruth({ claims: [claim("c", [])], freshness: "fresh" }).claims[0].grounding, "unsupported");
  assert.equal(projectDocumentTruth({ claims: [claim("c", [edge("supports")])], freshness: "changed_since_generation" }).claims[0].grounding, "stale");
});

test("heuristic doc relation may retain inferential confidence", () => {
  const projected = projectDocumentTruth({ claims: [claim("c", [edge("supersedes", "HEURISTIC_BRIDGE", 0.9)])], freshness: "fresh" });
  assert.equal(projected.claims[0].grounding, "indirect");
  assert.equal(projected.claims[0].observed[0].confidence, 0.9);
});

test("schema 20 makes claim_code_edges confidence nullable without changing other columns", () => {
  const db = openStore(":memory:", { upToVersion: 19 });
  try {
    assert.equal(db.prepare("PRAGMA table_info(claim_code_edges)").all().find((c) => c.name === "confidence").notnull, 1);
    migrateNullableDocTruthConfidence(db);
    const columns = db.prepare("PRAGMA table_info(claim_code_edges)").all();
    assert.equal(columns.find((c) => c.name === "confidence").notnull, 0);
    assert.ok(columns.some((c) => c.name === "evidence_code_content_hash"));
  } finally { db.close(); }
});
