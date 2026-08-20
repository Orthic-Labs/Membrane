import assert from "node:assert/strict";
import { cp, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { verifyMcpRegistry } from "../../scripts/release/verify-mcp-registry.mjs";

const source = await readFile(new URL("../../server.json", import.meta.url), "utf8");

async function fixture(change) {
  const directory = await mkdtemp(join(tmpdir(), "membrane-mcp-registry-"));
  await cp(new URL("../../dist/npm", import.meta.url), join(directory, "dist/npm"), { recursive: true });
  const server = JSON.parse(source); change(server);
  await writeFile(join(directory, "server.json"), JSON.stringify(server));
  return directory;
}

test("source metadata binds official MCP identity and pinned install route", async () => {
  const result = await verifyMcpRegistry({ directory: new URL("../..", import.meta.url).pathname, requirePublished: false });
  assert.equal(result.name, "io.github.orthic-labs/membrane");
  assert.equal(result.install, "npm:@membrane/membrane@0.0.0-source");
});

test("unverified namespace or artifacts fail closed for installation", async () => {
  await assert.rejects(() => verifyMcpRegistry({ directory: new URL("../..", import.meta.url).pathname }), /unpublished/);
});

test("identity and native artifact tampering are rejected", async () => {
  const directory = await fixture(server => { server.nativeArtifacts["darwin-x64"].package = "@membrane/other"; });
  await assert.rejects(() => verifyMcpRegistry({ directory, requirePublished: false }), /native package map mismatch/);
});

test("publication flags cannot substitute for namespace and artifact proof", async () => {
  const directory = await fixture(server => { server.publication.namespaceStatus = "published"; server.publication.artifactStatus = "published"; });
  await assert.rejects(() => verifyMcpRegistry({ directory }), /unpublished|artifact|published namespace receipt/);
});
