import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { mkdtemp, mkdir, realpath, rm, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const server = fileURLToPath(new URL("./server.mjs", import.meta.url));

async function rpc(messages, env) {
  const child = spawn(process.execPath, [server], { stdio: ["pipe", "pipe", "pipe"], windowsHide: true, env: { ...process.env, ...env } });
  let output = "";
  let stderr = "";
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  let responses;
  try {
    responses = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`MCP response timeout: ${stderr}`)), 30_000);
      child.once("error", reject);
      child.once("close", (code) => {
        if (output.trim().split("\n").filter(Boolean).length < messages.length) reject(new Error(`MCP exited ${code}: ${stderr}`));
      });
      child.stdout.on("data", (chunk) => {
        output += chunk;
        const lines = output.trim().split("\n").filter(Boolean);
        if (lines.length === messages.length) {
          clearTimeout(timer);
          try { resolve(lines.map(JSON.parse)); } catch (error) { reject(error); }
        }
      });
      child.stdin.end(messages.map(JSON.stringify).join("\n") + "\n");
    });
  } finally {
    child.kill();
    await Promise.race([once(child, "exit"), new Promise((resolve) => setTimeout(resolve, 1_000))]);
  }
  return responses;
}

async function freePort() {
  return new Promise((resolve, reject) => {
    const listener = createServer();
    listener.once("error", reject);
    listener.listen(0, "127.0.0.1", () => {
      const { port } = listener.address();
      listener.close((error) => error ? reject(error) : resolve(port));
    });
  });
}

async function startMemright(binary, db, port, env) {
  const child = spawn(binary, ["--db", db, "serve", "--port", String(port)], { stdio: ["ignore", "ignore", "pipe"], windowsHide: true, env });
  let stderr = "";
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/health`);
      if (response.ok) return child;
    } catch { /* startup race */ }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  child.kill();
  throw new Error(`isolated memright service did not start: ${stderr}`);
}

async function stopMemright(child) {
  if (child.exitCode !== null || child.killed) return;
  child.kill();
  await Promise.race([once(child, "exit"), new Promise((resolve) => setTimeout(resolve, 1_000))]);
}

test("C1 durable proposal/feedback returns readback receipts across MCP restart", async () => {
  const root = await mkdtemp(join(tmpdir(), "membrane-durable-"));
  const enrolled = join(root, "enrolled");
  await mkdir(enrolled);
  const canonical = await realpath(enrolled);
  const registry = join(root, "registry.json");
  const store = join(root, "memright-engine.db");
  const token = join(root, "api-token");
  await writeFile(registry, JSON.stringify({ schema_version: 1, bindings: { [canonical]: { repository_id: "repo-a", scope_id: "scope-a", provider_config: {}, grant_policy: { level: "write-proposed" } } } }));
  await writeFile(token, "test-token\n", { mode: 0o600 });
  const env = {
    MEMBRANE_PROJECT_REGISTRY: registry,
    MEMRIGHT_BIN: process.env.MEMRIGHT_TEST_BIN || fileURLToPath(new URL("../../tools/bin/memright", import.meta.url)),
    MEMRIGHT_DB: store,
    MEMRIGHT_API_TOKEN_FILE: token,
  };
  const port = await freePort();
  env.MEMRIGHT_PORT = String(port);
  const resident = await startMemright(env.MEMRIGHT_BIN, store, port, env);
  let active = resident;
  const request = (id, name, args) => ({ jsonrpc: "2.0", id, method: "tools/call", params: { name, arguments: args } });
  const common = { repository: enrolled, caller: { root: enrolled, repositoryId: "repo-a", scopeId: "scope-a" } };
  try {
    const first = await rpc([request(1, "membrane_knowledge_propose", { ...common, emission: { text: "durable proposal" } })], env);
    assert.equal(first[0].result.isError, false, first[0].result.content[0].text);
    const proposal = JSON.parse(first[0].result.content[0].text);
    assert.equal(proposal.status, "needs_review");
    assert.equal(proposal.durable, true);
    assert.equal(proposal.reviewState, "pending");
    assert.equal(proposal.lifecycleReceipt.status, "needs_review");
    const lifecycle = await rpc([
      request(2, "membrane_working_context", { ...common, operation: "save", context: { sessionId: "session-a", taskId: "task-a", items: [{ ref: "sha256:a" }], expiresAt: "2030-08-03T00:00:00Z", durable: true, authority: "A1", sourceRefs: ["source-a"] } }),
      request(3, "membrane_temporal_fact", { ...common, operation: "record", fact: { factId: "fact-a", subject: "repo", predicate: "owner", object: "adrian", observedAt: "2026-08-02T00:00:00Z", scopeId: "scope-a" }, singleValuedPredicates: ["owner"] }),
      request(4, "membrane_scratchpad", { ...common, operation: "save", scratchpad: { sessionId: "session-a", taskId: "task-a", items: [{ note: "ephemeral" }], expiresAt: "2030-08-03T00:00:00Z" } }),
    ], env);
    assert.ok(lifecycle.every((response) => response.result.isError === false), lifecycle.map((response) => response.result.content[0].text));
    const lifecycleById = Object.fromEntries(lifecycle.map((response) => [response.id, response]));
    assert.equal(JSON.parse(lifecycleById[2].result.content[0].text).context.durable, true);
    assert.equal(JSON.parse(lifecycleById[3].result.content[0].text).fact.fact_id, "fact-a");
    assert.equal(JSON.parse(lifecycleById[4].result.content[0].text).scratchpad.items[0].note, "ephemeral");
    await stopMemright(resident);
    const restarted = await startMemright(env.MEMRIGHT_BIN, store, port, env);
    active = restarted;
    const second = await rpc([
      request(6, "membrane_working_context", { ...common, operation: "load", sessionId: "session-a", taskId: "task-a", asOf: "2026-08-02T12:00:00Z" }),
      request(7, "membrane_temporal_fact", { ...common, operation: "query", scopeId: "scope-a", subject: "repo", predicate: "owner", asOf: "2026-08-02T12:00:00Z" }),
      request(8, "membrane_scratchpad", { ...common, operation: "load", sessionId: "session-a", taskId: "task-a", asOf: "2026-08-02T12:00:00Z" }),
      request(9, "membrane_feedback", { ...common, receiptId: proposal.durableId, outcome: "used" }),
    ], env);
    assert.ok(second.every((response) => response.result.isError === false), second.map((response) => response.result.content[0].text));
    const secondById = Object.fromEntries(second.map((response) => [response.id, response]));
    assert.equal(JSON.parse(secondById[6].result.content[0].text).contexts.length, 1);
    assert.equal(JSON.parse(secondById[7].result.content[0].text).facts[0].fact_id, "fact-a");
    assert.equal(JSON.parse(secondById[8].result.content[0].text).scratchpad, null);
    const feedback = JSON.parse(secondById[9].result.content[0].text);
    assert.equal(feedback.status, "persisted");
    assert.equal(feedback.lifecycleReceipt.status, "persisted");
    assert.ok(readFileSync(store).byteLength > 0);
  } finally {
    await stopMemright(active);
    await rm(root, { recursive: true, force: true });
  }
});
