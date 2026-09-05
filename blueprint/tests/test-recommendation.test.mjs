import assert from "node:assert/strict";
import test from "node:test";
import { openStore, bulkInsertGeneration } from "../src/graph/store-sqlite.mjs";
import { recommendTestsForImpact } from "../src/graph/test-recommendation.mjs";

const ev = (path) => [{ path, startLine: 1, endLine: 2, contentHash: `${path}-hash` }];

test("test recommendations use first-class TESTS evidence and report uncovered impact", () => {
  const db = openStore(":memory:");
  try {
    bulkInsertGeneration(db, {
      nodes: [
        { id: "test:a", kind: "symbol", labels: ["Test"], name: "testA", qualifiedName: "testA", path: "tests/a.test.ts", confidence: null, evidence: ev("tests/a.test.ts") },
        { id: "prod:a", kind: "symbol", labels: ["Function"], name: "a", qualifiedName: "a", path: "src/a.ts", confidence: null, evidence: ev("src/a.ts") },
        { id: "prod:b", kind: "symbol", labels: ["Function"], name: "b", qualifiedName: "b", path: "src/b.ts", confidence: null, evidence: ev("src/b.ts") },
      ],
      edges: [{ id: "tests:a", kind: "TESTS", source: "test:a", target: "prod:a", confidence: null, confidenceTier: "EXACT_RESOLUTION", evidence: ev("tests/a.test.ts") }],
      manifest: { generationId: "g-tests" },
      provider: { id: "blueprint-static" },
    });
    const result = recommendTestsForImpact(db, { generationId: "g-tests", impactedIds: ["prod:a", "prod:b"] });
    assert.equal(result.recommendations.length, 1);
    assert.equal(result.recommendations[0].testId, "test:a");
    assert.deepEqual(result.recommendations[0].coveredTargets, ["prod:a"]);
    assert.deepEqual(result.uncoveredImpact, ["prod:b"]);
    assert.deepEqual(result.coverage, { impacted: 2, covered: 1, ratio: 0.5 });
    assert.equal(result.minimality, "not_proven");
    assert.ok(result.recommendations[0].evidence.length > 0);
  } finally { db.close(); }
});

test("absence of TESTS evidence is an omission, never a claim that no tests exist", () => {
  const db = openStore(":memory:");
  try {
    bulkInsertGeneration(db, {
      nodes: [{ id: "prod:a", kind: "symbol", labels: ["Function"], name: "a", qualifiedName: "a", path: "src/a.ts", confidence: null, evidence: ev("src/a.ts") }],
      edges: [], manifest: { generationId: "g-empty" }, provider: { id: "blueprint-static" },
    });
    const result = recommendTestsForImpact(db, { generationId: "g-empty", impactedIds: ["prod:a"] });
    assert.deepEqual(result.recommendations, []);
    assert.deepEqual(result.uncoveredImpact, ["prod:a"]);
    assert.ok(result.omissions.some((row) => row.reason === "no_static_test_reachability_evidence"));
    assert.equal(result.minimality, "not_proven");
  } finally { db.close(); }
});
