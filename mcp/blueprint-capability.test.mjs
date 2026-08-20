import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { TOOLS, blueprintCapability, blueprintCommand, sanitizeBlueprintPayload } from "./server.mjs";
const auth = { repository: "repo", caller: { root: "/repo", repositoryId: "repo", scopeId: "scope" } };

test("Blueprint operations use bounded argv", () => {
  const schema = TOOLS.find((tool) => tool.name === "membrane_blueprint").inputSchema;
  assert.deepEqual(Object.keys(schema.properties).filter((key) => ["query", "from", "to"].includes(key)), []);
  assert.equal(schema.properties.node.minLength, 1); assert.equal(schema.oneOf.length, 2);
  for (const operation of ["architecture", "symbol", "reference", "references", "impact", "changes"]) {
    const { command } = blueprintCommand({ ...auth, operation, ...(operation === "architecture" || operation === "changes" ? {} : { node: "safe_node", depth: 1 }), limit: 1, budget: 1 });
    assert.ok(command.includes("--json")); assert.ok(command.includes("--limit"));
    assert.equal(command.filter((arg) => arg === "--json").length, 1);
    assert.ok(!command.includes(auth.repository) && !command.includes(auth.caller.root));
  }
  for (const args of [{ operation: "symbol", node: "bad value" }, { operation: "impact", node: "x", limit: 0 }, { operation: "changes", query: "x", budget: 10001 }, { operation: "reference", node: "x", depth: 6 }, { operation: "architecture", node: "x" }, { operation: "impact", node: "x", from: "a" }]) assert.throws(() => blueprintCommand({ ...auth, ...args }));
  assert.throws(() => blueprintCommand({ operation: "architecture" }), /auth envelope/);
  const snap = blueprintCommand({ ...auth, operation: "snapshot_get", name: "base" });
  assert.deepEqual(snap.command.slice(0, 4), ["snapshot", "get", "base", "--json"]);
  assert.throws(() => blueprintCommand({ ...auth, operation: "changes_since", name: "bad name" }), /snapshot name/);
});

test("Blueprint payload is content-free and hash-bound", () => {
  for (const invalid of [null, [], 42, "object"]) assert.throws(() => sanitizeBlueprintPayload(invalid, "architecture"), /blueprint_unavailable/);
  const output = sanitizeBlueprintPayload({ freshness: { generationId: "gen-1", barrierResult: "caught_up", receiptId: "receipt-1", secret: "raw graph" }, results: [{ id: "src/main.rs:Thing", name: "Thing", kind: "function", path: "src/main.rs", sourceHash: `sha256:${"a".repeat(64)}`, text: "raw source", extra: "leak" }, { id: "bad id with space", text: "drop" }] }, "symbol");
  assert.equal(output.items.length, 1); assert.equal(output.items[0].sourceHash, `sha256:${"a".repeat(64)}`); assert.doesNotMatch(JSON.stringify(output), /raw source|raw graph|leak|drop/);
  assert.equal(output.items[0].resolver, "blueprint graph resolve --node src/main.rs:Thing");
  assert.deepEqual(sanitizeBlueprintPayload({ freshness: { generationId: "gen-1" }, results: [{ id: "x" }] }, "symbol").items, []);
  const changes = sanitizeBlueprintPayload({ generationId: "gen-1", manifestDigest: `sha256:${"b".repeat(64)}`, sourceObservation: { commit: "a".repeat(40), clean: true }, freshness: { generationId: "gen-1", barrierResult: "caught_up", receiptId: "receipt-1" } }, "changes");
  assert.equal(changes.changes.sourceObservation.clean, true);
  assert.equal(changes.resolver, "blueprint graph manifest --json");
  for (const invalid of [{ generationId: "gen-1" }, { generationId: "gen-1", manifestDigest: `sha256:${"b".repeat(64)}`, sourceObservation: { commit: "not-a-hash", clean: true } }, { generationId: "gen-1", manifestDigest: `sha256:${"b".repeat(64)}`, sourceObservation: { commit: "a".repeat(40), clean: true } }]) assert.throws(() => sanitizeBlueprintPayload(invalid, "changes"), /blueprint_unavailable/);
});

test("Blueprint capability uses IPC only & returns typed unavailable results", async () => {
  await assert.rejects(
    blueprintCapability({ root: "/repo" }, { ...auth, operation: "architecture" }, undefined, { request: async () => { throw new Error("SECRET_RAW_STDERR"); } }),
    (error) => error.message === "blueprint_unavailable" && !error.message.includes("SECRET_RAW_STDERR"),
  );
  await assert.rejects(blueprintCapability({ root: "/repo" }, { ...auth, operation: "changes" }, undefined, { request: async () => ({}) }), /blueprint_unavailable/);
  const source = await readFile(new URL("./server.mjs", import.meta.url), "utf8");
  const blueprintSection = source.slice(source.indexOf("export async function blueprintBatchCapability"), source.indexOf("async function callTool"));
  assert.doesNotMatch(blueprintSection, /BLUEPRINT_CLI|graph", "batch|process\.execPath/);
});

test("snapshot payloads are identity-only and bounded", () => {
  const hash = `xxh128:${"a".repeat(32)}`;
  const identity = { generationId: hash, manifestDigest: hash, sourceObservation: { head: "b".repeat(40), dirty: false } };
  const got = sanitizeBlueprintPayload({ name: "base", ...identity, leaves: [{ path: "src/a.ts", digest: hash }], ignored: "raw" }, "snapshot_get");
  assert.equal(got.leaves[0].path, "src/a.ts"); assert.doesNotMatch(JSON.stringify(got), /ignored|raw/);
  const delta = sanitizeBlueprintPayload({ base: identity, head: identity, changes: [{ path: "src/a.ts", kind: "modified" }], receipt: { total: 1, limit: 10, truncated: false } }, "changes_since");
  assert.equal(delta.receipt.total, 1);
  assert.throws(() => sanitizeBlueprintPayload({ base: identity, head: { ...identity, sourceObservation: { head: "b".repeat(40), dirty: true } }, changes: [], receipt: { total: 0, limit: 10, truncated: false } }, "changes_since"), /blueprint_unavailable/);
});
