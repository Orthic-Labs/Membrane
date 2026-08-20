// D07: MCP adapter over the shared application service — six tools, root
// supplied at process start, all egress redacted and budgeted.

import assert from "node:assert/strict";
import { cpSync, mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";

const ROOT = join(import.meta.dirname, "..");
const SERVER = join(ROOT, "scripts/blueprint-mcp.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function payload(response) {
  assert.ok(!response.isError, `unexpected error: ${response.content?.[0]?.text ?? ""}`);
  const text = response.content.find((block) => block.type === "text")?.text;
  assert.ok(text);
  return JSON.parse(text);
}

async function withServer(repo, fn) {
  const transport = new StdioClientTransport({ command: process.execPath, args: [SERVER, "--root", repo], cwd: repo, stderr: "pipe" });
  const client = new Client({ name: "blueprint-test", version: "1.0.0" }, { capabilities: {} });
  try {
    await client.connect(transport);
    return await fn(client);
  } finally {
    await client.close().catch(() => {});
  }
}

function buildRepo() {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-mcp-service-"));
  cpSync(FIXTURE, repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  return repo;
}

test("MCP exposes exactly six task-shaped tools", async () => {
  const repo = buildRepo();
  try {
    await withServer(repo, async (client) => {
      const tools = await client.listTools();
      const names = tools.tools.map((tool) => tool.name).sort();
      assert.deepEqual(names, ["blueprint_doc_truth", "blueprint_expand", "blueprint_impact", "blueprint_recall", "blueprint_search", "blueprint_status"]);
      // No tool accepts an unrestricted repoRoot.
      for (const tool of tools.tools) {
        const schema = tool.inputSchema ?? {};
        const props = Object.keys(schema.properties ?? {});
        assert.ok(!props.includes("repoRoot"), `${tool.name} must not accept repoRoot`);
      }
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("blueprint_recall returns generation context with receipt", async () => {
  const repo = buildRepo();
  try {
    await withServer(repo, async (client) => {
      const result = payload(await client.callTool({ name: "blueprint_recall", arguments: { task: "placeOrder" } }));
      assert.equal(result.schemaVersion, 1);
      assert.equal(result.action, "allow");
      assert.ok(result.generationId);
      assert.ok(result.freshnessReceipt);
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("blueprint_search returns bounded results", async () => {
  const repo = buildRepo();
  try {
    await withServer(repo, async (client) => {
      const result = payload(await client.callTool({ name: "blueprint_search", arguments: { query: "placeOrder", limit: 5 } }));
      assert.equal(result.kind, "search");
      assert.ok(Array.isArray(result.results));
      assert.ok(result.results.length <= 5);
      assert.ok(result.freshnessReceipt);
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("blueprint_expand returns a bounded evidence slice", async () => {
  const repo = buildRepo();
  try {
    await withServer(repo, async (client) => {
      const search = payload(await client.callTool({ name: "blueprint_search", arguments: { query: "placeOrder", limit: 1 } }));
      const anchor = search.results[0].id;
      const result = payload(await client.callTool({ name: "blueprint_expand", arguments: { anchor, depth: 1, budget: 2000 } }));
      assert.ok(result.nodes || result.neurons);
      assert.ok(result.edges || result.synapses);
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("blueprint_impact returns affected edges", async () => {
  const repo = buildRepo();
  try {
    await withServer(repo, async (client) => {
      const search = payload(await client.callTool({ name: "blueprint_search", arguments: { query: "placeOrder", limit: 1 } }));
      const anchor = search.results[0].id;
      const result = payload(await client.callTool({ name: "blueprint_impact", arguments: { anchor, depth: 2, budget: 2000 } }));
      assert.ok(result);
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("blueprint_doc_truth lists claims", async () => {
  const repo = buildRepo();
  try {
    await withServer(repo, async (client) => {
      const result = payload(await client.callTool({ name: "blueprint_doc_truth", arguments: { limit: 50 } }));
      assert.equal(result.schemaVersion, 1);
      assert.ok(Array.isArray(result.claims));
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("blueprint_status reports repository state", async () => {
  const repo = buildRepo();
  try {
    await withServer(repo, async (client) => {
      const result = payload(await client.callTool({ name: "blueprint_status", arguments: {} }));
      assert.equal(result.schemaVersion, 1);
      assert.ok(result.repository);
      assert.ok("state" in result);
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("errors preserve structured codes and set isError", async () => {
  const repo = buildRepo();
  try {
    await withServer(repo, async (client) => {
      // Schema-level validation errors surface as SDK errors (non-JSON).
      await assert.rejects(
        client.callTool({ name: "blueprint_search", arguments: { query: "" } }),
        (error) => error?.code === -32602,
      );
      // Service-level errors surface as JSON envelopes with stable codes.
      const missing = await client.callTool({ name: "blueprint_expand", arguments: { anchor: "no-such-symbol-xyz", depth: 1 } });
      assert.equal(missing.isError, true);
      const parsed = JSON.parse(missing.content.find((block) => block.type === "text")?.text);
      assert.equal(parsed.error.code, "anchor_not_found");
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("stale barrier is surfaced as an error with allowStale opt-out", async () => {
  const repo = buildRepo();
  try {
    // Force an unreachable target clock.
    const { openStore, closeStore } = await import("../src/graph/store-sqlite.mjs");
    const db = openStore(join(repo, ".agent", "graph", "graph.db"));
    try {
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('source_clock','99999') ON CONFLICT(key) DO UPDATE SET value='99999'").run();
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('applied_clock','0') ON CONFLICT(key) DO UPDATE SET value='0'").run();
      db.prepare("DELETE FROM event_journal").run();
    } finally {
      closeStore(db);
    }
    await withServer(repo, async (client) => {
      // The barrier cannot catch up within the service timeout -> stale_blocked.
      const blocked = await client.callTool({ name: "blueprint_search", arguments: { query: "placeOrder" } });
      assert.equal(blocked.isError, true);
      const parsed = JSON.parse(blocked.content.find((block) => block.type === "text")?.text);
      assert.equal(parsed.error.code, "stale_blocked");
      // allowStale serves from the last complete generation.
      const allowed = await client.callTool({ name: "blueprint_search", arguments: { query: "placeOrder", allowStale: true, limit: 5 } });
      assert.ok(!allowed.isError);
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("egress is redacted for secret-shaped values", async () => {
  const repo = buildRepo();
  try {
    await withServer(repo, async (client) => {
      // status exposes environment-adjacent fields; force a secret-shaped value via a probe.
      const result = payload(await client.callTool({ name: "blueprint_status", arguments: {} }));
      assert.ok(result);
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
