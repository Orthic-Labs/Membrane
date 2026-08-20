// D45: local explorer — loopback-only, session-token auth, read-only,
// evidence-linked endpoints, and no giant-graph default.

import assert from "node:assert/strict";
import { cpSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { startExplorerServer } from "../src/lib/http-server.mjs";
import { createSessionToken, verifySessionToken } from "../src/lib/session-token.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";
import { RootRegistry } from "../src/lib/application/root-registry.mjs";

function apiGet(url, token) {
  return fetch(url, { headers: { authorization: `Bearer ${token}` } });
}

function baseUrl(serverUrl) {
  return serverUrl.split("#")[0].replace(/\/$/, "");
}

test("session tokens are unguessable and timing-safe verified", () => {
  const token = createSessionToken();
  assert.equal(token.length, 43);
  assert.notEqual(token, createSessionToken());
  assert.equal(verifySessionToken(token, token), true);
  assert.equal(verifySessionToken(token, "wrong"), false);
  assert.equal(verifySessionToken(token, ""), false);
});

test("explorer serves service endpoints with token auth", async () => {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-explorer-"));
  cpSync(join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce"), repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  const registry = new RootRegistry([{ root: repo, repoId: "repo-1" }]);
  const service = createBlueprintApplicationService({ rootRegistry: registry, allowEmbeddedRoot: false });
  const server = await startExplorerServer({ service, repoId: "repo-1", serveAsset: (req, res) => res.end("ui") });
  try {
    const token = server.url.split("#token=")[1];
    const base = baseUrl(server.url);
    // 401 without token.
    const unauth = await fetch(`${base}/api/status`);
    assert.equal(unauth.status, 401);
    // Status with token.
    const status = await apiGet(`${base}/api/status`, token);
    assert.equal(status.status, 200);
    const payload = await status.json();
    assert.ok(payload.repository);
    // Search.
    const search = await apiGet(`${base}/api/search?q=placeOrder&limit=5`, token);
    assert.equal(search.status, 200);
    const searchPayload = await search.json();
    assert.ok(searchPayload.results.length > 0);
    // Doc-truth.
    const docs = await apiGet(`${base}/api/doc-truth`, token);
    assert.equal(docs.status, 200);
    // Unknown route.
    const unknown = await apiGet(`${base}/api/nope`, token);
    assert.equal(unknown.status, 404);
    // Non-GET rejected.
    const post = await fetch(`${base}/api/status`, { method: "POST", headers: { authorization: `Bearer ${token}` } });
    assert.equal(post.status, 405);
  } finally {
    await server.close();
    rmSync(repo, { recursive: true, force: true });
  }
});

test("explorer binds loopback only", async () => {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-explorer-loop-"));
  cpSync(join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce"), repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  const registry = new RootRegistry([{ root: repo, repoId: "repo-1" }]);
  const service = createBlueprintApplicationService({ rootRegistry: registry, allowEmbeddedRoot: false });
  const server = await startExplorerServer({ service, repoId: "repo-1", serveAsset: () => {} });
  try {
    const url = new URL(server.url);
    assert.equal(url.hostname, "127.0.0.1");
    assert.ok(url.port);
  } finally {
    await server.close();
    rmSync(repo, { recursive: true, force: true });
  }
});
