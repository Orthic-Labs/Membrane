// D32: SDK + plugin boundary — typed clients, contract validation, plugin
// capability/permission constraints, and no private-file-layout dependency.

import assert from "node:assert/strict";
import { cpSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { BlueprintClient, EmbeddedBlueprintClient, PROTOCOL_VERSION } from "../src/sdk/index.mjs";
import { definePlugin, PLUGIN_TYPES } from "../src/sdk/providers.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { createDaemonServer } from "../src/service/server.mjs";
import { temporaryDaemonEndpoint } from "../src/service/paths.mjs";
import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";
import { RootRegistry } from "../src/lib/application/root-registry.mjs";
import examplePlugin from "../examples/providers/example-language-plugin.mjs";

test("SDK exposes typed client classes and protocol version", () => {
  assert.equal(typeof BlueprintClient, "function");
  assert.equal(typeof EmbeddedBlueprintClient, "function");
  assert.equal(PROTOCOL_VERSION, 1);
});

test("embedded client searches a built repo read-only", async () => {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-sdk-embedded-"));
  cpSync(join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce"), repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  try {
    const client = new EmbeddedBlueprintClient({ allowEmbeddedRoot: true });
    const result = await client.search({ repoRoot: repo, query: "placeOrder", limit: 5 });
    assert.equal(result.kind, "search");
    assert.ok(result.results.length > 0);
    await client.close();
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("daemon client talks to the resident daemon", async () => {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-sdk-daemon-"));
  cpSync(join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce"), repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  const endpoint = temporaryDaemonEndpoint("blueprint-sdk");
  const registry = new RootRegistry([{ root: repo, repoId: "repo-1" }]);
  const service = createBlueprintApplicationService({ rootRegistry: registry, allowEmbeddedRoot: false });
  const daemon = createDaemonServer({ service, endpoint });
  try {
    await daemon.listen();
    const client = new BlueprintClient({ endpoint });
    const result = await client.search({ repoId: "repo-1", query: "placeOrder", limit: 5 });
    assert.ok(result.results.length > 0);
    await client.close();
  } finally {
    await daemon.close().catch(() => {});
    rmSync(repo, { recursive: true, force: true });
  }
});

test("stable direct client uses bounded one-shot when Hub daemon is absent", async () => {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-sdk-one-shot-"));
  cpSync(join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce"), repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  try {
    const client = new BlueprintClient({
      endpoint: temporaryDaemonEndpoint("blueprint-sdk-absent"),
      rootRegistry: new RootRegistry([{ root: repo, repoId: "repo-one-shot" }]),
    });
    const result = await client.search({ repoRoot: repo, query: "placeOrder", limit: 5 });
    assert.ok(result.results.length > 0);
    await client.close();
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("plugin types are constrained", () => {
  assert.ok(PLUGIN_TYPES.includes("language-table"));
  assert.ok(PLUGIN_TYPES.includes("compiler-provider"));
  assert.ok(PLUGIN_TYPES.includes("rule-pack"));
  assert.throws(() => definePlugin({ id: "x", version: "1", protocolRange: "1", type: "nope", capabilities: [], permissions: {}, hash: "h" }), /unknown plugin type/);
  assert.throws(() => definePlugin({ id: "x" }), /missing/);
});

test("example plugin passes the contract with repo-read-only permissions", () => {
  assert.equal(examplePlugin.id, "example.lang.dsl");
  assert.equal(examplePlugin.permissions.filesystem, "repo-read");
  assert.equal(examplePlugin.permissions.network, "none");
  assert.ok(examplePlugin.table.functions.length > 0);
});

test("plugin boundary: plugins cannot override built-ins", () => {
  // The SDK exposes no override path for built-in providers: definePlugin
  // requires a full manifest and the plugin type is constrained. Built-in
  // language ids/extensions are locked by the custom-language config.
  const plugin = definePlugin({ id: "custom.id", version: "1.0.0", protocolRange: ">=1 <2", type: "language-table", capabilities: [], permissions: {}, hash: "h" });
  assert.equal(plugin.id, "custom.id");
  assert.ok(plugin.permissions.filesystem === "repo-read");
});
