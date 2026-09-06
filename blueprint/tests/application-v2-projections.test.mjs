import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";

function repo() {
  const root = mkdtempSync(join(tmpdir(), "blueprint-app-v2-"));
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(join(root, "src", "service.ts"), `export function placeOrder() { return 1; }\nexport function main() { return placeOrder(); }\n`);
  buildGraphGeneration(root, { outDir: ".agent", persist: true });
  return root;
}

test("existing search tool exposes BM25 and structural retrieval without a new MCP tool", async () => {
  const root = repo();
  try {
    const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
    const result = await service.search({ repoRoot: root, query: "place order", astPattern: { kind: "Function", name: "place" }, limit: 10 });
    assert.ok(result.retrieval?.bm25?.fingerprint);
    assert.ok(result.results.some((row) => row.name === "placeOrder"));
    assert.ok(result.retrieval.structural);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("architecture tool exposes orientation process contract and signature projections", async () => {
  const root = repo();
  try {
    const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
    const orientation = await service.architecture({ repoRoot: root, view: "orientation" });
    const processes = await service.architecture({ repoRoot: root, view: "processes" });
    const contracts = await service.architecture({ repoRoot: root, view: "contracts" });
    const signatures = await service.architecture({ repoRoot: root, view: "signatures", limit: 20 });
    assert.equal(orientation.kind, "cold-start-orientation");
    assert.equal(processes.kind, "process-projection");
    assert.ok(Array.isArray(contracts.contracts));
    assert.ok(signatures.signatures.some((row) => row.name === "placeOrder"));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("resolve can request an on-demand live semantic cross-check without replacing canonical identity", async () => {
  const root = repo();
  try {
    const service = createBlueprintApplicationService({
      allowEmbeddedRoot: true,
      liveVerifier: async ({ canonical }) => ({ provider: "lsp-test", entityId: canonical.id }),
    });
    const search = await service.search({ repoRoot: root, query: "placeOrder", limit: 5 });
    const symbol = search.results.find((row) => row.name === "placeOrder");
    const resolved = await service.resolve({ repoRoot: root, nodeId: symbol.id, verifySemantic: true });
    assert.equal(resolved.node.id, symbol.id);
    assert.equal(resolved.verification.state, "agreement");
  } finally { rmSync(root, { recursive: true, force: true }); }
});
