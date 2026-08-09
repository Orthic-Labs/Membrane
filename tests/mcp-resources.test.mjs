// D31: MCP resources/prompts, compatibility matrix, and host configs.

import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { buildGraphGeneration } from "../graph/static-provider.mjs";

import { RESOURCE_URIS, resourceForUri } from "../mcp/resources.mjs";
import { PROMPTS, promptByName } from "../mcp/prompts.mjs";
import { TOOL_EFFECTS } from "../mcp/effects.mjs";
import { MCP_COMPATIBILITY, HOSTS, mcpConfigForHost } from "../mcp/compatibility.mjs";
import { HOOK_POLICY_MODES, HOOK_POLICIES, policyBehavior } from "../lib/init/host-configs.mjs";

const ROOT = join(import.meta.dirname, "..");
const SERVER = join(ROOT, "scripts/cortex-mcp.mjs");
const FIXTURE = join(ROOT, "evals", "fixture-repos", "typescript-commerce");
const AUDIT_DIR = join(ROOT, ".audit", "cx-b1");

function ensureAuditDir() {
  mkdirSync(AUDIT_DIR, { recursive: true });
}

async function startServer({ repo }) {
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [SERVER, "--root", repo],
    cwd: repo,
    stderr: "pipe",
  });
  const client = new Client({ name: "cortex-resources-acceptance", version: "1.0.0" }, { capabilities: {} });
  await client.connect(transport);
  return { client, transport };
}

test("resources are URI-addressable and paginated", () => {
  assert.ok(RESOURCE_URIS.length >= 6);
  for (const uri of RESOURCE_URIS) {
    const resource = resourceForUri(uri, { limit: 50 });
    assert.equal(resource.uri, uri);
    assert.ok(resource.schemaVersion);
  }
  const claims = resourceForUri("cortex://claims", { limit: 10 });
  assert.equal(claims.pagination.limit, 10);
  const unknown = resourceForUri("cortex://nope");
  assert.equal(unknown.error.code, "resource_not_found");
});

test("prompts reference tools, not embedded prose", () => {
  assert.ok(PROMPTS.length >= 5);
  for (const prompt of PROMPTS) {
    assert.ok(prompt.toolRefs.length > 0, `${prompt.name} must reference tools`);
    for (const ref of prompt.toolRefs) {
      assert.match(ref, /^cortex_/);
    }
  }
  assert.ok(promptByName("debug"));
  assert.equal(promptByName("nope"), null);
});

test("compatibility matrix covers current and legacy SDK majors", () => {
  assert.ok(MCP_COMPATIBILITY.some((entry) => entry.major === 1 && entry.status === "supported"));
  assert.ok(MCP_COMPATIBILITY.some((entry) => entry.major === 0 && entry.status === "legacy"));
});

// CX-B1 S2 acceptance: the live server (spawned over stdio) must register
// every data resource plus the AX P14 effects resource, and every prompt from
// mcp/prompts.mjs. Counts come from the handshake, never from source reads.
test("live server registers >=8 resources and all 6 prompts, effects resource readable", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-mcp-res-"));
  ensureAuditDir();
  let client;
  try {
    cpSync(FIXTURE, repo, { recursive: true });
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    ({ client } = await startServer({ repo }));
    const resources = await client.listResources();
    const prompts = await client.listPrompts();
    const tools = await client.listTools();
    assert.ok(resources.resources.length >= 8, `expected >=8 resources, got ${resources.resources.length}`);
    assert.equal(prompts.prompts.length, 6);
    assert.equal(tools.tools.length, 6);

    const liveUris = new Set(resources.resources.map((resource) => resource.uri));
    for (const uri of RESOURCE_URIS) {
      assert.ok(liveUris.has(uri), `resource ${uri} must be registered on the live server`);
    }
    assert.ok(liveUris.has("cortex://effects"), "cortex://effects must be registered");
    for (const resource of resources.resources) {
      assert.ok(resource.name, `resource ${resource.uri} must carry a name`);
      assert.match(resource.uri, /^cortex:\/\//, `resource uri ${resource.uri} must use the cortex:// scheme`);
    }

    const livePromptNames = new Set(prompts.prompts.map((prompt) => prompt.name));
    for (const prompt of PROMPTS) {
      assert.ok(livePromptNames.has(prompt.name), `prompt ${prompt.name} must be registered on the live server`);
    }

    // Read every registered data resource over the transport: registration
    // must be functional, not merely listed.
    for (const uri of RESOURCE_URIS) {
      const read = await client.readResource({ uri });
      assert.ok(Array.isArray(read.contents) && read.contents.length > 0, `resource ${uri} must be readable`);
      assert.equal(read.contents[0].uri, uri, `resource ${uri} must echo its uri`);
      assert.ok(typeof read.contents[0].text === "string" && read.contents[0].text.length > 0, `resource ${uri} must return text content`);
    }

    // AX P14: the effects resource must enumerate the six registered tools.
    const effects = await client.readResource({ uri: "cortex://effects" });
    const effectsText = effects.contents.find((block) => block.uri === "cortex://effects")?.text ?? effects.contents[0]?.text ?? "";
    const effectsPayload = JSON.parse(effectsText);
    const effectKeys = Object.keys(effectsPayload.tools ?? effectsPayload).sort();
    assert.deepEqual(effectKeys, Object.keys(TOOL_EFFECTS).sort());

    writeFileSync(join(AUDIT_DIR, "handshake.json"), JSON.stringify({
      tools: tools.tools.length,
      resources: resources.resources.length,
      prompts: prompts.prompts.length,
      toolNames: tools.tools.map((tool) => tool.name).sort(),
      resourceUris: [...liveUris].sort(),
      promptNames: [...livePromptNames].sort(),
      effectsResource: true,
      transport: "stdio",
      server: "scripts/cortex-mcp.mjs",
    }, null, 2));
  } finally {
    if (client) await client.close().catch(() => {});
    rmSync(repo, { recursive: true, force: true });
  }
});

test("host configs generate for all seven hosts", () => {
  assert.equal(HOSTS.length, 7);
  for (const host of HOSTS) {
    const config = mcpConfigForHost(host, { root: "/repo" });
    assert.ok(config, `missing config for ${host}`);
    assert.ok(config.path);
    assert.ok(config.value);
  }
});

test("hook policies have explicit fail-open/fail-closed and recovery", () => {
  assert.deepEqual(HOOK_POLICY_MODES, ["advisory", "orient-before-read", "task-grants"]);
  assert.equal(HOOK_POLICIES.advisory.failClosed, false);
  assert.equal(HOOK_POLICIES["orient-before-read"].failClosed, true);
  assert.equal(HOOK_POLICIES["task-grants"].failClosed, true);
  assert.match(policyBehavior("orient-before-read").recoveryCommand, /cortex orient/);
  assert.match(policyBehavior("task-grants").recoveryCommand, /cortex grant issue/);
  assert.equal(policyBehavior("advisory").recoveryCommand, null);
});
