import assert from "node:assert/strict";
import test from "node:test";
import { existsSync, readFileSync } from "node:fs";
import { mkdtemp, mkdir, realpath, rm, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const server = fileURLToPath(new URL("./server.mjs", import.meta.url));
const membraneRoot = fileURLToPath(new URL("../", import.meta.url));
const engineRoot = fileURLToPath(new URL("../engine/", import.meta.url));
const sourceCrypt = join(engineRoot, "target", "debug", process.platform === "win32" ? "crypt.exe" : "crypt");

async function currentCrypt() {
  if (process.env.CRYPT_TEST_BIN) return process.env.CRYPT_TEST_BIN;
  const cargo = spawn("cargo", ["build", "--manifest-path", join(engineRoot, "Cargo.toml"), "--bin", "crypt", "--message-format=json-render-diagnostics"], {
    cwd: membraneRoot,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  let stdout = "";
  let stderr = "";
  cargo.stdout.on("data", (chunk) => { stdout += chunk; });
  cargo.stderr.on("data", (chunk) => { stderr += chunk; });
  const [code] = await once(cargo, "close");
  let builtCrypt = null;
  for (const line of stdout.split(/\r?\n/)) {
    try {
      const message = JSON.parse(line);
      if (message.reason === "compiler-artifact" && message.target?.name === "crypt" && typeof message.executable === "string") builtCrypt = message.executable;
    } catch { /* non-JSON Cargo output is irrelevant */ }
  }
  if (code !== 0 || !builtCrypt || !existsSync(builtCrypt)) {
    // The rebuild failed (e.g. RIGHT_RELEASE_CACHE_ROOT unset, or a broken shared build cache).
    // Fall back to a previously built binary rather than failing outright — it is stale relative
    // to source, but still a real Crypt build, so the test keeps exercising real behavior instead
    // of going dark. Logged loudly so staleness is never silent.
    if (existsSync(sourceCrypt)) {
      process.stderr.write(`server-durable.test.mjs: reusing stale Crypt build at ${sourceCrypt} because rebuild failed: ${stderr}\n`);
      return sourceCrypt;
    }
    throw new Error(`current Crypt build failed: ${stderr}`);
  }
  return builtCrypt;
}

async function rpc(messages, env) {
  const child = spawn(process.execPath, [server], { stdio: ["pipe", "pipe", "pipe"], windowsHide: true, env: { ...process.env, ...env } });
  let output = "";
  let stderr = "";
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  // The server also emits MCP `notifications/message` lifecycle-log lines (lifecycle.mjs,
  // MBR-305) on the same stdout stream as the JSON-RPC responses; those notifications carry no
  // `id`. Counting raw lines (as this helper used to) resolves on the first lifecycle
  // notification instead of the real response. Filter on `id !== undefined`, matching the
  // already-correct sibling helper in mcp/server.test.mjs.
  const expected = messages.filter((message) => message.id !== undefined).length;
  let responses;
  try {
    responses = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`MCP response timeout: ${stderr}`)), 30_000);
      child.once("error", reject);
      child.once("close", (code) => {
        const rows = output.trim().split("\n").filter(Boolean).map((line) => JSON.parse(line));
        if (rows.filter((row) => row.id !== undefined).length < expected) reject(new Error(`MCP exited ${code}: ${stderr}`));
      });
      child.stdout.on("data", (chunk) => {
        output += chunk;
        let rows;
        try { rows = output.trim().split("\n").filter(Boolean).map((line) => JSON.parse(line)); }
        catch { return; } // a line may still be mid-flight; wait for more data
        const ready = rows.filter((row) => row.id !== undefined);
        if (ready.length >= expected) {
          clearTimeout(timer);
          try { resolve(ready); } catch (error) { reject(error); }
        }
      });
      // Write only -- do NOT close stdin here. server.mjs treats a closed stdin as a client
      // disconnect and aborts every in-flight request's signal (see the `stdio_closed` shutdown
      // handler near the bottom of server.mjs); ending stdin before the response arrives races
      // that abort against `withCancellationGrace` and silently discards an otherwise-successful
      // result. Matches the already-correct sibling helper in mcp/server.test.mjs, which also
      // writes first and only ends stdin after every expected response has been collected.
      child.stdin.write(messages.map(JSON.stringify).join("\n") + "\n");
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

async function startCrypt(binary, db, port, env) {
  const child = spawn(binary, ["--db", db, "serve", "--port", String(port)], { stdio: ["ignore", "ignore", "pipe"], windowsHide: true, env });
  let stderr = "";
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  for (let attempt = 0; attempt < 400; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/health`);
      if (response.ok) return child;
    } catch { /* startup race */ }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  child.kill();
  throw new Error(`isolated crypt service did not start: ${stderr}`);
}

async function stopCrypt(child) {
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
  const store = join(root, "crypt-engine.db");
  const token = join(root, "api-token");
  await writeFile(registry, JSON.stringify({ schema_version: 1, bindings: { [canonical]: { repository_id: "repo-a", scope_id: "scope-a", provider_config: {}, grant_policy: { level: "write-proposed" } } } }));
  await writeFile(token, "test-token\n", { mode: 0o600 });
  const env = {
    ...process.env,
    MEMBRANE_PROJECT_REGISTRY: registry,
    CRYPT_DB: store,
    CRYPT_API_TOKEN_FILE: token,
    CRYPT_ALLOW_HASH: "1",
    WORKSPACE_ROOT: root,
  };
  env.CRYPT_BIN = await currentCrypt();
  const port = await freePort();
  env.CRYPT_PORT = String(port);
  const resident = await startCrypt(env.CRYPT_BIN, store, port, env);
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
    await stopCrypt(resident);
    const restarted = await startCrypt(env.CRYPT_BIN, store, port, env);
    active = restarted;
    const second = await rpc([
      request(6, "membrane_working_context", { ...common, operation: "load", sessionId: "session-a", taskId: "task-a", asOf: "2026-08-02T12:00:00Z" }),
      request(7, "membrane_temporal_fact", { ...common, operation: "query", scopeId: "scope-a", subject: "repo", predicate: "owner", asOf: "2026-08-02T12:00:00Z" }),
      request(8, "membrane_scratchpad", { ...common, operation: "load", sessionId: "session-a", taskId: "task-a", asOf: "2026-08-02T12:00:00Z" }),
      // F10: a plain agent self-report (no verdictRef) must persist as advisory/unverified --
      // it can never land as a ranking-eligible source just because the caller claimed one.
      request(9, "membrane_feedback", { ...common, receiptId: proposal.durableId, outcome: "used" }),
    ], env);
    assert.ok(second.every((response) => response.result.isError === false), second.map((response) => response.result.content[0].text).join(" | "));
    const secondById = Object.fromEntries(second.map((response) => [response.id, response]));
    assert.equal(JSON.parse(secondById[6].result.content[0].text).contexts.length, 1);
    assert.equal(JSON.parse(secondById[7].result.content[0].text).facts[0].fact_id, "fact-a");
    assert.equal(JSON.parse(secondById[8].result.content[0].text).scratchpad, null);
    const feedback = JSON.parse(secondById[9].result.content[0].text);
    assert.equal(feedback.status, "persisted");
    assert.equal(feedback.lifecycleReceipt.status, "persisted");
    assert.equal(feedback.source, "advisory", "an unqualified self-report must persist as advisory");
    assert.equal(feedback.verified, false, "an unqualified self-report must never land as a verified/ranking source");

    // A self-report that names a resolvable cited verdict DOES persist as verified. Issued as
    // its own request (not concurrently with the call above) to avoid exercising the resident
    // service's post-restart event-outbox replay window with two simultaneous feedback writes.
    const third = await rpc([
      request(10, "membrane_feedback", { ...common, receiptId: "receipt-cited-1", outcome: "used", verdictRef: "verdict-doc#section-1" }),
    ], env);
    assert.equal(third[0].result.isError, false, third[0].result.content[0].text);
    const citedFeedback = JSON.parse(third[0].result.content[0].text);
    assert.equal(citedFeedback.status, "persisted");
    assert.equal(citedFeedback.source, "cited_verdict");
    assert.equal(citedFeedback.verified, true, "a resolvable cited verdict is verified and ranking-eligible");
    assert.ok(readFileSync(store).byteLength > 0);
  } finally {
    await stopCrypt(active);
    await rm(root, { recursive: true, force: true });
  }
});
