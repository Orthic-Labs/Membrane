import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { ProjectionCache, buildProjectionDependencyDag, invalidatedProjections, projectionFingerprint } from "../src/graph/dependency-dag.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";

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

test("a real config change invalidates the projection cache through the declared config parent", () => {
  // BPT-020 declares `config` as a dependency parent, but nothing set
  // `configDigest`: the application service read it and no producer wrote it,
  // so the parent was dead and only a synthetic unit test exercised it. This
  // drives the real build path end to end.
  const root = mkdtempSync(join(tmpdir(), "blueprint-config-dag-"));
  try {
    mkdirSync(join(root, "src"), { recursive: true });
    writeFileSync(join(root, "src/a.js"), "export const a = 1;\n");
    writeFileSync(join(root, "tsconfig.json"), JSON.stringify({ compilerOptions: { baseUrl: "." } }));
    const before = buildGraphGeneration(root, { outDir: ".agent", persist: false });
    const beforeDigest = before.augmentation.providers.configDigest;
    assert.ok(beforeDigest, "the build must publish a config digest, not leave the parent null");

    writeFileSync(join(root, "tsconfig.json"), JSON.stringify({ compilerOptions: { baseUrl: "./src" } }));
    const after = buildGraphGeneration(root, { outDir: ".agent", persist: false });
    const afterDigest = after.augmentation.providers.configDigest;
    assert.notEqual(afterDigest, beforeDigest, "editing a consumed config file must change the digest");

    // The digest is content-addressed, not timestamped: rebuilding the original
    // config reproduces the original digest rather than drifting.
    writeFileSync(join(root, "tsconfig.json"), JSON.stringify({ compilerOptions: { baseUrl: "." } }));
    assert.equal(buildGraphGeneration(root, { outDir: ".agent", persist: false }).augmentation.providers.configDigest, beforeDigest);

    // Invalidation is scoped by the DECLARED parents, not blanket-applied.
    // `conventions` declares `config`; `bm25` deliberately does not, because it
    // is built from graph nodes rather than resolution configuration. Both
    // halves are asserted so this proves the DAG is explicit, not just eager.
    const dagFor = (configDigest) => buildProjectionDependencyDag({ sourceHash: "s1", providerDigest: "p1", configDigest, schemaVersion: 1, generationId: "g1" });
    const counted = (cache, name) => {
      let builds = 0;
      const build = () => { builds += 1; return { built: builds }; };
      cache.getOrBuild(name, dagFor(beforeDigest), build);
      cache.getOrBuild(name, dagFor(beforeDigest), build);
      const afterWarm = builds;
      cache.getOrBuild(name, dagFor(afterDigest), build);
      return { afterWarm, total: builds };
    };
    const conventions = counted(new ProjectionCache(), "conventions");
    assert.equal(conventions.afterWarm, 1, "an unchanged config reuses the cached projection");
    assert.equal(conventions.total, 2, "a changed config digest invalidates a config-dependent projection");
    const bm25 = counted(new ProjectionCache(), "bm25");
    assert.equal(bm25.total, 1, "a projection that does not declare `config` is not invalidated by it");
    const invalidated = invalidatedProjections(dagFor(beforeDigest), ["config"]);
    assert.ok(invalidated.includes("conventions"));
    assert.ok(!invalidated.includes("bm25"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
