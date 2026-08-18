import assert from "node:assert/strict";
import test from "node:test";
import { executeCodeBatch } from "../../schemas/registry/code/mbr402-batch.mjs";

const admission = (item) => ({ ok: item.allow !== false, code: "denied", repositoryRoot: item.root, authority: "read-only", generationId: item.generation, sourceHash: item.hash });
test("MBR-402 calls provider once and emits request-order receipts", async () => {
  let calls = 0;
  const out = await executeCodeBatch({ items: [
    { id: "b", root: "/r", generation: "g", hash: "h" },
    { id: "a", root: "/r", generation: "g", hash: "h", allow: false },
  ], authorize: admission, provider: async (items) => { calls++; return [{ id: "b", repositoryRoot: "/r", authority: "read-only", generationId: "g", sourceHash: "h", data: { ok: true } }]; } });
  assert.equal(calls, 1); assert.deepEqual(out.map((row) => row.status), ["success", "terminal"]); assert.equal(out[1].code, "denied");
});
test("MBR-402 fails closed on foreign or stale provider rows", async () => {
  const out = await executeCodeBatch({ items: [{ id: "x", root: "/r", generation: "g", hash: "h" }], authorize: admission, provider: async () => [{ id: "x", repositoryRoot: "/other", authority: "read-only", generationId: "g", sourceHash: "h" }] });
  assert.equal(out[0].code, "provider_mismatch");
});
test("MBR-402 grouped MCP-shaped batch strips auth and withholds denied/stale siblings", async () => {
  let calls = 0; let sent;
  const out = await executeCodeBatch({ items: [
    { id: "accepted", operation: "architecture", repository: "/repo", caller: { root: "/repo", repositoryId: "r", scopeId: "s" }, root: "/repo", generation: "g", hash: "h" },
    { id: "denied", operation: "architecture", repository: "/repo", caller: { root: "/repo", repositoryId: "r", scopeId: "s" }, root: "/repo", generation: "g", hash: "h", allow: false },
    { id: "stale", operation: "architecture", repository: "/repo", caller: { root: "/repo", repositoryId: "r", scopeId: "s" }, root: "/repo", generation: "old", hash: "h" },
  ], authorize: (item) => item.generation !== "g" ? ({ ok: false, code: "stale_generation" }) : ({ ok: item.allow !== false, code: "denied", repositoryRoot: item.root, authority: "read-only", generationId: item.generation, sourceHash: item.hash }), provider: async (items) => {
    calls += 1; sent = items; return [{ id: "accepted", repositoryRoot: "/repo", authority: "read-only", generationId: "g", sourceHash: "h", data: { ok: true } }];
  } });
  assert.equal(calls, 1); assert.deepEqual(sent.map((item) => item.id), ["accepted"]); assert.equal(sent[0].caller, undefined); assert.equal(sent[0].repository, undefined);
  assert.deepEqual(out.map((row) => row.status), ["success", "terminal", "terminal"]); assert.equal(out[1].code, "denied"); assert.equal(out[2].code, "stale_generation");
});
