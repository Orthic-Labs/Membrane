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
  child.stdout.on("data", (chunk) => { output += chunk; });
  child.stdin.end(messages.map(JSON.stringify).join("\n") + "\n");
  await new Promise((resolve, reject) => { child.once("error", reject); child.once("close", resolve); });
  return output.trim().split("\n").filter(Boolean).map(JSON.parse);
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
    MEMRIGHT_BIN: fileURLToPath(new URL("../../tools/bin/memright", import.meta.url)),
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
    assert.equal(proposal.status, "persisted");
    assert.equal(proposal.durable, true);
    assert.equal(proposal.lifecycleReceipt.status, "persisted");
    await stopMemright(resident);
    const restarted = await startMemright(env.MEMRIGHT_BIN, store, port, env);
    active = restarted;
    const second = await rpc([request(2, "membrane_feedback", { ...common, receiptId: proposal.durableId, outcome: "used" })], env);
    assert.equal(second[0].result.isError, false, second[0].result.content[0].text);
    const feedback = JSON.parse(second[0].result.content[0].text);
    assert.equal(feedback.status, "persisted");
    assert.equal(feedback.lifecycleReceipt.status, "persisted");
    assert.ok(readFileSync(store).byteLength > 0);
  } finally {
    await stopMemright(active);
    await rm(root, { recursive: true, force: true });
  }
});
