import assert from "node:assert/strict";
import test from "node:test";

import { ProjectionCache, buildProjectionDependencyDag, invalidatedProjections, projectionFingerprint } from "../src/graph/dependency-dag.mjs";

function dag(overrides = {}) {
  return buildProjectionDependencyDag({ sourceHash: "s1", providerDigest: "p1", configDigest: "c1", schemaVersion: 20, generationId: "g1", ...overrides });
}

test("projection DAG explicitly binds source provider config schema and generation parents", () => {
  const value = dag();
  assert.ok(value.nodes.some((node) => node.id === "parent:source"));
  assert.ok(value.nodes.some((node) => node.id === "projection:orientation"));
  assert.ok(value.edges.some((edge) => edge.from === "parent:config" && edge.to === "projection:contracts"));
  assert.ok(value.edges.some((edge) => edge.from === "parent:generation" && edge.to === "projection:bm25"));
});

test("invalidation closure is projection-specific and deterministic", () => {
  assert.deepEqual(invalidatedProjections(dag(), ["config"]), ["contracts", "conventions", "orientation", "processes"]);
  assert.ok(invalidatedProjections(dag(), ["source"]).includes("bm25"));
  assert.ok(!invalidatedProjections(dag(), ["config"]).includes("bm25"));
});

test("projection cache invalidates when any declared parent changes", () => {
  const cache = new ProjectionCache();
  let builds = 0;
  const first = cache.getOrBuild("bm25", dag(), () => ({ build: ++builds }));
  const second = cache.getOrBuild("bm25", dag(), () => ({ build: ++builds }));
  assert.equal(first.cache, "miss");
  assert.equal(second.cache, "hit");
  assert.equal(second.value.build, 1);
  const changed = cache.getOrBuild("bm25", dag({ sourceHash: "s2", generationId: "g2" }), () => ({ build: ++builds }));
  assert.equal(changed.cache, "invalidated");
  assert.equal(changed.value.build, 2);
});

test("fingerprint ignores undeclared parent dimensions for a projection", () => {
  const a = projectionFingerprint(dag({ configDigest: "c1" }), "bm25");
  const b = projectionFingerprint(dag({ configDigest: "c2" }), "bm25");
  assert.equal(a, b);
  assert.notEqual(projectionFingerprint(dag({ configDigest: "c1" }), "contracts"), projectionFingerprint(dag({ configDigest: "c2" }), "contracts"));
});
