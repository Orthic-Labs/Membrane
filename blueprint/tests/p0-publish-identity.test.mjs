import assert from "node:assert/strict";
import { mkdtempSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  buildGraphGeneration,
  augmentGenerationWithTreeSitter,
  finalizeGenerationIdentity,
} from "../src/graph/static-provider.mjs";
import { computeGenerationId } from "../src/graph/generation-identity.mjs";
import { validatePortableManifest } from "../src/graph/portable-manifest.mjs";
import {
  openStore,
  closeStore,
  saveGeneration,
  loadGeneration,
  getGenerationEnvelope,
  countRows,
} from "../src/graph/store-sqlite.mjs";

const FIXTURE = join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce");

test("saveGeneration is atomic: row failure rolls back envelope writes and stale keys", () => {
  const db = openStore(":memory:");
  try {
    const good = {
      schemaVersion: 1,
      manifest: { generationId: "gen-good", complete: true, counts: { nodes: 1, edges: 0 } },
      provider: { id: "blueprint-static" },
      nodes: [{
        id: "file:a.ts",
        kind: "file",
        labels: ["File"],
        name: "a.ts",
        qualifiedName: "a.ts",
        path: "a.ts",
        confidence: 1,
        evidence: [{ path: "a.ts", startLine: 1, endLine: 1, contentHash: "h1" }],
      }],
      edges: [],
      repoRoot: "/tmp",
    };
    saveGeneration(db, { ...good, augmentation: { treesitter: { parsedFiles: 1 } } });
    assert.ok(getGenerationEnvelope(db).augmentation);
    assert.equal(getGenerationEnvelope(db).manifest.generationId, "gen-good");

    const bad = {
      ...good,
      manifest: { generationId: "gen-bad", complete: true },
      nodes: [{
        id: "file:a.ts",
        kind: "file",
        labels: ["File"],
        name: "a.ts",
        qualifiedName: "a.ts",
        path: undefined,
        confidence: 1,
        evidence: [{ path: "a.ts", startLine: 1, endLine: 1, contentHash: "h1" }],
      }],
    };
    assert.throws(() => saveGeneration(db, bad));
    assert.equal(getGenerationEnvelope(db).manifest.generationId, "gen-good");
    assert.ok(getGenerationEnvelope(db).augmentation);
    assert.equal(countRows(db).files, 1);

    saveGeneration(db, good);
    assert.equal(getGenerationEnvelope(db).augmentation, undefined);
  } finally {
    closeStore(db);
  }
});

test("saveGeneration preserves prior generation for row, envelope, and stale-key failures", () => {
  const db = openStore(":memory:");
  const base = {
    schemaVersion: 1,
    manifest: { generationId: "gen-prior", complete: true, counts: { nodes: 1, edges: 0 } },
    provider: { id: "blueprint-static" },
    nodes: [{
      id: "file:prior.ts", kind: "file", labels: ["File"], name: "prior.ts", qualifiedName: "prior.ts",
      path: "prior.ts", confidence: 1, evidence: [{ path: "prior.ts", startLine: 1, endLine: 1, contentHash: "prior" }],
    }],
    edges: [],
    repoRoot: "/tmp",
  };
  const next = {
    ...base,
    manifest: { ...base.manifest, generationId: "gen-next" },
    nodes: [{ ...base.nodes[0], id: "file:next.ts", path: "next.ts", name: "next.ts", qualifiedName: "next.ts" }],
  };
  const injections = [
    ["first row insert", (sql) => sql.includes("INSERT INTO files")],
    ["first envelope put", (sql) => sql.includes("INSERT INTO generation")],
    ["stale-key delete", (sql) => sql.includes("DELETE FROM generation WHERE key NOT IN")],
  ];
  try {
    saveGeneration(db, base);
    for (const [label, predicate] of injections) {
      const original = db.prepare.bind(db);
      let injected = false;
      db.prepare = (sql, ...params) => {
        if (!injected && predicate(String(sql))) {
          injected = true;
          throw new Error(`injected ${label}`);
        }
        return original(sql, ...params);
      };
      assert.throws(() => saveGeneration(db, next), new RegExp(`injected ${label}`));
      db.prepare = original;
      const loaded = loadGeneration(db);
      assert.equal(loaded.manifest.generationId, "gen-prior", label);
      assert.equal(loaded.nodes[0].path, "prior.ts", label);
    }
  } finally {
    closeStore(db);
  }
});

test("finalizeGenerationIdentity recomputes generationId and manifestDigest after augmentation", async () => {
  const generation = buildGraphGeneration(FIXTURE);
  const lexicalId = generation.manifest.generationId;
  const beforeDigest = generation.manifest.manifestDigest;
  await augmentGenerationWithTreeSitter(generation, FIXTURE, {});
  finalizeGenerationIdentity(generation);
  const postId = generation.manifest.generationId;
  assert.notEqual(postId, lexicalId, "augmented body must change generationId");
  assert.match(postId, /^xxh128:/);
  assert.match(generation.manifest.manifestDigest, /^sha256:[a-f0-9]{64}$/);
  assert.notEqual(generation.manifest.manifestDigest, beforeDigest);
  const expected = computeGenerationId(
    generation.nodes,
    generation.edges,
    generation.manifest.repo.sourceHash,
  );
  assert.equal(postId, expected);
});

test("buildGraphGeneration with outDir does not publish until persist is requested", () => {
  const dir = mkdtempSync(join(tmpdir(), "bp-no-publish-"));
  try {
    buildGraphGeneration(dir, { outDir: ".agent" });
    assert.equal(existsSync(join(dir, ".agent/graph/graph.db")), false);
    buildGraphGeneration(dir, { outDir: ".agent", persist: true });
    assert.equal(existsSync(join(dir, ".agent/graph/graph.db")), true);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("validatePortableManifest rejects absolute and parent-traversal artifact paths", () => {
  const errors = validatePortableManifest({
    schemaVersion: 1,
    repo: { sourceHash: "xxh128:abc" },
    artifacts: {
      graph: "/Users/adrian/graph.db",
      map: "../outside/map.json",
    },
    entrypoint: ".agent/",
    humanDocs: ["docs/product.md"],
  });
  assert.ok(errors.some((error) => error.includes("artifacts.graph")));
  assert.ok(errors.some((error) => error.includes("artifacts.map")));
});
