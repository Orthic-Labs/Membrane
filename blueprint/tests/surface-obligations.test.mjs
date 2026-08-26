import assert from "node:assert/strict";
import { cpSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import Ajv2020 from "ajv/dist/2020.js";
import { readFileSync } from "node:fs";

import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";
import { RootRegistry } from "../src/lib/application/root-registry.mjs";
import { buildGraphGeneration, graphFlowInventory } from "../src/graph/static-provider.mjs";
import { closeStore, listEdgeCore, openStoreReadOnly } from "../src/graph/store-sqlite.mjs";
import { METHODS } from "../src/service/protocol.mjs";

const FIXTURE = join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce");

function builtRepo() {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-surfaces-"));
  cpSync(FIXTURE, repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  return repo;
}

function connectedPair(repo) {
  const db = openStoreReadOnly(join(repo, ".agent", "graph", "graph.db"));
  try {
    const edge = listEdgeCore(db).find((item) => item.target);
    assert.ok(edge, "fixture must have one resolved edge");
    return { from: edge.source, to: edge.target };
  } finally {
    closeStore(db);
  }
}

function connectedChain(repo) {
  const db = openStoreReadOnly(join(repo, ".agent", "graph", "graph.db"));
  try {
    const edges = listEdgeCore(db).filter((item) => item.target);
    for (const first of edges) {
      const second = edges.find((item) => item.source === first.target && item.target && item.target !== first.source);
      if (second) return { from: first.source, middle: first.target, to: second.target };
    }
    assert.fail("fixture must have a two-edge directed path");
  } finally {
    closeStore(db);
  }
}

test("path is a public protocol method and uses bounded application traversal", async () => {
  const repo = builtRepo();
  try {
    assert.ok(METHODS.includes("path"));
    const result = await createBlueprintApplicationService({ allowEmbeddedRoot: true }).path({
      repoRoot: repo,
      ...connectedPair(repo),
      maxDepth: 2,
      budget: 2000,
    });
    assert.equal(result.kind, "path");
    assert.equal(result.found, true);
    assert.ok(result.path.length >= 2);
    assert.ok(result.generationId);
    assert.equal(result.truncated, false);
    await assert.rejects(
      createBlueprintApplicationService({ allowEmbeddedRoot: true }).path({ repoRoot: repo, from: result.from, to: result.to, maxDepth: 13 }),
      (error) => error.code === "path_bounds_invalid",
    );
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("path cursors fail closed and bind generation, endpoints, and traversal bounds", async () => {
  const repo = builtRepo();
  try {
    const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
    const chain = connectedChain(repo);
    const input = { repoRoot: repo, from: chain.from, to: chain.to, maxDepth: 2, budget: 128 };
    const first = await service.path(input);
    assert.equal(first.truncated, true);
    assert.ok(first.continuationCursor);
    const second = await service.path({ ...input, cursor: first.continuationCursor });
    assert.equal(second.found, true);

    await assert.rejects(service.path({ ...input, cursor: "not-a-cursor" }), (error) => error.code === "cursor_invalid");
    const decoded = JSON.parse(Buffer.from(first.continuationCursor, "base64url").toString("utf8"));
    const cursorWith = (changes) => Buffer.from(JSON.stringify({ ...decoded, ...changes })).toString("base64url");
    await assert.rejects(
      service.path({ ...input, cursor: cursorWith({ generationId: "wrong-generation" }) }),
      (error) => error.code === "cursor_invalid",
    );
    await assert.rejects(
      service.path({ ...input, to: chain.middle, cursor: first.continuationCursor }),
      (error) => error.code === "cursor_invalid",
    );
    await assert.rejects(
      service.path({ ...input, maxDepth: 3, cursor: first.continuationCursor }),
      (error) => error.code === "cursor_invalid",
    );
    await assert.rejects(
      service.path({ ...input, cursor: cursorWith({ node: 100000 }) }),
      (error) => error.code === "cursor_invalid",
    );
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("flow pages use canonical full-path order and unique full-path digest ids", () => {
  const node = (id) => ({ id, kind: "symbol", evidence: [{ path: `${id}.mjs`, startLine: 1, endLine: 1 }] });
  const nodes = ["a", "b", "c", "d", "x"].map(node);
  const edge = (source, target) => ({ id: `edge:${source}->${target}`, kind: "CALLS", source, target });
  const generation = {
    provider: { id: "test" },
    manifest: { generationId: "generation:test", generatedAt: "test" },
    nodes,
    edges: [edge("a", "c"), edge("c", "d"), edge("a", "b"), { ...edge("a", "b"), id: "edge:a=>b:parallel" }, edge("b", "x"), edge("x", "d")],
  };
  const result = graphFlowInventory(generation, { maxFlows: 10 });
  assert.deepEqual(result.flows.map((flow) => flow.path.map((item) => item.id)), [
    ["a", "b", "x", "d"],
    ["a", "c", "d"],
  ]);
  assert.equal(new Set(result.flows.map((flow) => flow.id)).size, 2);
  for (const flow of result.flows) assert.match(flow.id, /^flow:sha256:[a-f0-9]{64}$/);
});

test("architecture flows have deterministic bounded pages and generation-bound cursors", async () => {
  const repo = builtRepo();
  try {
    const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
    const first = await service.architecture({ repoRoot: repo, view: "flows", maxFlows: 1 });
    assert.equal(first.schemaVersion, 2);
    assert.equal(first.kind, "architecture");
    assert.equal(first.view, "flows");
    assert.equal(first.ordering, "entry.id,path[].id");
    assert.deepEqual(first.bounds, { maxFlows: 1, maxDepth: 12 });
    assert.ok(first.flows.length <= 1);
    const schema = JSON.parse(readFileSync(join(import.meta.dirname, "..", "schemas", "architecture-flow-view-v1.schema.json"), "utf8"));
    assert.equal(new Ajv2020().compile(schema)(first), true);
    if (first.continuationCursor) {
      const second = await service.architecture({ repoRoot: repo, view: "flows", maxFlows: 1, cursor: first.continuationCursor });
      assert.notEqual(second.flows[0]?.id, first.flows[0]?.id);
    }
    await assert.rejects(
      service.architecture({ repoRoot: repo, view: "flows", cursor: Buffer.from(JSON.stringify({ view: "flows", generationId: "wrong", offset: 0 })).toString("base64url") }),
      (error) => error.code === "cursor_invalid",
    );
    await assert.rejects(
      service.architecture({ repoRoot: repo, view: "flows", cursor: Buffer.from(JSON.stringify({ view: "flows", generationId: first.generationId, offset: 10001 })).toString("base64url") }),
      (error) => error.code === "cursor_invalid",
    );
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("Hub-hosted and bounded one-shot surfaces agree and generation mismatch fails closed", async () => {
  const repo = builtRepo();
  try {
    const rootRegistry = new RootRegistry([{ root: repo }]);
    const direct = createBlueprintApplicationService({ freshnessOwnership: "one_shot", rootRegistry });
    const hosted = createBlueprintApplicationService({ freshnessOwnership: "resident", rootRegistry });
    // A copied fixture is not a VCS checkout, so the resident observer honestly
    // reports unavailable freshness. allowStale lets us compare the pinned
    // generation payload while preserving that receipt distinction.
    const input = { repoRoot: repo, view: "flows", maxFlows: 20, allowStale: true };
    const directResult = await direct.architecture(input);
    const hostedResult = await hosted.architecture(input);
    assert.deepEqual(hostedResult.flows, directResult.flows);
    assert.equal(hostedResult.generationId, directResult.generationId);
    for (const service of [direct, hosted]) {
      await assert.rejects(
        service.architecture({ ...input, generation: "xxh128:not-the-served-generation" }),
        (error) => error.code === "generation_mismatch",
      );
    }
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("complete audit projection remains exact and untruncated", async () => {
  const repo = builtRepo();
  try {
    const { spawnSync } = await import("node:child_process");
    const cli = join(import.meta.dirname, "..", "scripts", "blueprint.mjs");
    const result = spawnSync(process.execPath, [cli, "graph", "audit-projection"], { cwd: repo, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const projection = JSON.parse(result.stdout);
    assert.equal(projection.schema, "membrane.blueprint-packet.v1");
    assert.equal(projection.fileCount, projection.files.length);
    assert.deepEqual(projection.files, [...projection.files].sort());
    assert.equal(Object.hasOwn(projection, "truncated"), false);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
