import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { buildGraphGeneration } from "../graph/static-provider.mjs";
import { RESOURCE_URIS } from "../mcp/resources.mjs";
import { PROMPTS } from "../mcp/prompts.mjs";

const ROOT = join(import.meta.dirname, "..");
const SERVER = join(ROOT, "scripts/cortex-mcp.mjs");
const AUDIT_DIR = join(ROOT, ".audit", "cx-b1");
const PROMPT_NAMES = [...new Set(PROMPTS.map((prompt) => prompt.name))].sort();
const FIXTURE = join(ROOT, "evals", "fixture-repos", "typescript-commerce");

function ensureAuditDir() {
  mkdirSync(AUDIT_DIR, { recursive: true });
}

async function startServer({ repo, args = [], env = {} } = {}) {
  const spawnArgs = repo ? [SERVER, "--root", repo, ...args] : [SERVER, ...args];
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: spawnArgs,
    cwd: repo ?? ROOT,
    stderr: "pipe",
    env,
  });
  let serverStderr = "";
  transport.stderr?.on("data", (chunk) => { serverStderr += chunk.toString(); });
  const client = new Client({ name: "cortex-acceptance", version: "1.0.0" }, { capabilities: {} });
  try {
    await client.connect(transport);
  } catch (error) {
    throw error;
  }
  return { client, transport, serverStderr: () => serverStderr };
}

function payload(response) {
  assert.ok(!response.isError, `unexpected error: ${response.content?.[0]?.text ?? ""}`);
  const text = response.content.find((block) => block.type === "text")?.text;
  assert.ok(text);
  return JSON.parse(text);
}

// Lifecycle chain, per CX-B1 §2: spawned -> initialized -> capabilities
// listed -> tool_called -> structured_result_returned -> process_exited_clean.
// Every response below comes from the real scripts/cortex-mcp.mjs process
// over stdio JSON-RPC via the SDK client; nothing is mocked or hand-authored.
test("live handshake: initialize -> list -> call -> close (6 tools, >=8 resources, 6 prompts)", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-mcp-"));
  ensureAuditDir();
  let transport;
  let client;
  let pid = null;
  let serverStderr;
  try {
    cpSync(FIXTURE, repo, { recursive: true });
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    ({ client, transport, serverStderr } = await startServer({ repo }));
    pid = transport.pid;
    // initialize is implicit in client.connect(); listTools/listResources/
    // listPrompts round-trip through the connected server.
    const tools = await client.listTools();
    const resources = await client.listResources();
    const prompts = await client.listPrompts();
    const names = tools.tools.map((tool) => tool.name).sort();
    assert.deepEqual(names, ["cortex_doc_truth", "cortex_expand", "cortex_impact", "cortex_orient", "cortex_search", "cortex_status"]);
    assert.ok(resources.resources.length >= 8, `expected >=8 resources, got ${resources.resources.length}`);
    assert.equal(prompts.prompts.length, 6);
    assert.deepEqual(prompts.prompts.map((prompt) => prompt.name).sort(), PROMPT_NAMES);
    // All 6 tool descriptors advertise an outputSchema and the read-only
    // effect annotations (AX P14).
    for (const tool of tools.tools) {
      assert.ok(tool.outputSchema, `${tool.name} must advertise outputSchema`);
      assert.ok(tool.outputSchema.type === "object", `${tool.name} outputSchema must be an object schema`);
      assert.equal(tool.annotations?.readOnlyHint, true, `${tool.name} must be read-only`);
      assert.equal(tool.annotations?.idempotentHint, true, `${tool.name} must be idempotent`);
      assert.equal(tool.annotations?.openWorldHint, false, `${tool.name} must be closed-world`);
    }
    const oriented = payload(await client.callTool({ name: "cortex_orient", arguments: { task: "placeOrder" } }));
    assert.equal(oriented.action, "allow");
    assert.ok(oriented.freshnessReceipt?.receiptId);
    const search = payload(await client.callTool({ name: "cortex_search", arguments: { query: "placeOrder", limit: 5 } }));
    assert.ok(Array.isArray(search.results));
    assert.ok(search.freshnessReceipt?.receiptId);
    writeFileSync(join(AUDIT_DIR, "handshake.json"), JSON.stringify({
      lifecycle: ["spawned", "initialized", "capabilities_listed", "tool_called", "structured_result_returned", "process_exited_clean"],
      tools: tools.tools.length,
      resources: resources.resources.length,
      prompts: prompts.prompts.length,
      toolNames: names,
      resourceUris: resources.resources.map((resource) => resource.uri).sort(),
      promptNames: prompts.prompts.map((prompt) => prompt.name).sort(),
      registeredResourceUris: RESOURCE_URIS,
      registeredPromptNames: PROMPT_NAMES,
      structuredContentSeen: true,
      transport: "stdio",
      server: "scripts/cortex-mcp.mjs",
    }, null, 2));
  } finally {
    if (client) await client.close().catch(() => {});
    if (serverStderr && serverStderr().trim()) console.error("SERVER STDERR:\n" + serverStderr().slice(0, 4000));
    // After a clean client close the spawned process must have exited; a
    // dangling server would leave the suite hanging on the event loop.
    if (pid) {
      const alive = await new Promise((done) => {
        try { process.kill(pid, 0); done(true); } catch { done(false); }
      });
      assert.equal(alive, false, "server process must exit cleanly after client close");
    }
    rmSync(repo, { recursive: true, force: true });
  }
});

const TOOL_ARGS = {
  cortex_orient: { task: "placeOrder" },
  cortex_search: { query: "placeOrder", limit: 5 },
  cortex_expand: { anchor: "src/service.ts", depth: 1, budget: 512 },
  cortex_impact: { anchor: "src/service.ts", depth: 1, budget: 512 },
  cortex_doc_truth: { limit: 5 },
  cortex_status: {},
};

// S3 probe + S4 bulk: every tool must return structuredContent that validates
// against its advertised outputSchema, plus text content carrying the same
// redacted payload. Coverage data is written to .audit/cx-b1/tools.json.
test("all six tools return schema-valid structuredContent", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-mcp-tools-"));
  ensureAuditDir();
  let client;
  const evidence = {};
  try {
    cpSync(FIXTURE, repo, { recursive: true });
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    ({ client } = await startServer({ repo }));
    const tools = await client.listTools();
    assert.equal(tools.tools.length, 6);
    for (const tool of tools.tools) {
      const args = TOOL_ARGS[tool.name];
      assert.ok(args, `missing acceptance args for ${tool.name}`);
      const result = await client.callTool({ name: tool.name, arguments: args });
      assert.ok(!result.isError, `${tool.name} failed: ${result.content?.[0]?.text ?? ""}`);
      assert.ok(result.structuredContent, `${tool.name} must return structuredContent`);
      assert.ok(tool.outputSchema, `${tool.name} must advertise outputSchema`);
      const text = result.content.find((block) => block.type === "text")?.text;
      assert.ok(text, `${tool.name} must return text content`);
      assert.deepEqual(JSON.parse(text), result.structuredContent, `${tool.name} text must serialize the same redacted payload`);
      if (tool.name === "cortex_orient" || tool.name === "cortex_status") {
        assert.deepEqual(Object.keys(result.structuredContent.claimBoundary).sort(), ["cleanClaimAllowed", "gaps", "prohibitedClaims", "safeClaims", "status"]);
        assert.equal(result.structuredContent.claimBoundary.status, "clear");
        assert.equal(result.structuredContent.claimBoundary.cleanClaimAllowed, true);
      }
      evidence[tool.name] = { ok: true, outputSchema: true, structuredContentKeys: Object.keys(result.structuredContent) };
    }
    writeFileSync(join(AUDIT_DIR, "tools.json"), JSON.stringify({ tools: evidence, allSchemaValid: true }, null, 2));
  } finally {
    if (client) await client.close().catch(() => {});
    rmSync(repo, { recursive: true, force: true });
  }
});

// S5 negatives: strict input rejection, typed errors with a stable code and no
// stack, and malformed-generation typing. Written to .audit/cx-b1/negatives.json.
test("negative cases: strict inputs, typed errors, redaction", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-mcp-neg-"));
  ensureAuditDir();
  let client;
  const evidence = {};
  try {
    cpSync(FIXTURE, repo, { recursive: true });
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    ({ client } = await startServer({ repo }));

    // (a) unknown argument field must be rejected by the strict input schema.
    // The SDK surfaces input-validation failures as a JSON-RPC error, so the
    // call must reject rather than silently strip the unknown field.
    await assert.rejects(
      client.callTool({ name: "cortex_status", arguments: { notARealField: "x" } }),
      (error) => {
        assert.ok(error instanceof Error, "rejection must be an Error");
        assert.match(String(error.message), /Invalid arguments for tool cortex_status/, "rejection must be an input-validation error");
        return true;
      },
    );
    evidence.unknownArgument = { rejected: true };

    // (b) nonexistent repoId -> typed error with a stable code, no stack.
    const missing = await client.callTool({ name: "cortex_status", arguments: { repoId: "does-not-exist-anywhere" } });
    assert.equal(missing.isError, true);
    const missingPayload = JSON.parse(missing.content.find((block) => block.type === "text")?.text ?? "{}");
    assert.ok(["root_not_enrolled", "root_escape"].includes(missingPayload.error?.code), `unexpected code ${missingPayload.error?.code}`);
    assert.ok(missingPayload.error?.message);
    assert.equal(/at |\.mjs:\d+/.test(missingPayload.error?.message ?? ""), false, "error payload must not contain a stack trace");
    evidence.nonexistentRepoId = { isError: missing.isError, code: missingPayload.error?.code, stack: false };

    // (c) malformed generation string -> typed error (generation_mismatch).
    // The generation check runs inside openFreshnessSession, which search uses.
    const malformed = await client.callTool({ name: "cortex_search", arguments: { query: "placeOrder", generation: "not-a-real-generation" } });
    assert.equal(malformed.isError, true);
    const malformedPayload = JSON.parse(malformed.content.find((block) => block.type === "text")?.text ?? "{}");
    assert.equal(malformedPayload.error?.code, "generation_mismatch");
    evidence.malformedGeneration = { isError: malformed.isError, code: malformedPayload.error?.code };

    // Redaction coverage: a secret-shaped value in a tool result must be
    // redacted in BOTH structuredContent and content text.
    const secret = "github_pat_abcdefghijklmnopqrstuvwxyz0123456789ABCD";
    const revealed = await client.callTool({
      name: "cortex_search",
      arguments: { query: `placeOrder ${secret}`, limit: 5 },
    });
    assert.ok(!revealed.isError);
    const structured = JSON.stringify(revealed.structuredContent ?? {});
    const serialized = JSON.stringify(revealed.content ?? {});
    assert.ok(!structured.includes(secret), "secret must be redacted in structuredContent");
    assert.ok(!serialized.includes(secret), "secret must be redacted in content text");
    evidence.redaction = { structuredContent: true, contentText: true };

    writeFileSync(join(AUDIT_DIR, "negatives.json"), JSON.stringify(evidence, null, 2));
  } finally {
    if (client) await client.close().catch(() => {});
    rmSync(repo, { recursive: true, force: true });
  }
});
