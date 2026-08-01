import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, mkdir, readFile, realpath, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const server = fileURLToPath(new URL("./server.mjs", import.meta.url));
const packageJson = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
assert.equal(packageJson.engines?.node, ">=20");
assert.equal(packageJson.dependencies?.["@modelcontextprotocol/server"], "2.0.0");

async function rpc(messages, env = {}) {
  const child = spawn(process.execPath, [server], {
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
    env: { ...process.env, ...env },
  });
  let output = "";
  child.stdout.on("data", (chunk) => { output += chunk; });
  child.stdin.end(messages.map(JSON.stringify).join("\n") + "\n");
  await new Promise((resolve, reject) => { child.once("error", reject); child.once("close", resolve); });
  return output.trim().split("\n").filter(Boolean).map(JSON.parse);
}
function toolError(row) { return row.error?.message || row.result?.content?.[0]?.text || ""; }

const rows = await rpc([
  { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "legacy-test", version: "1.0.0" } } },
  { jsonrpc: "2.0", id: 2, method: "tools/list", params: {} },
  { jsonrpc: "2.0", id: 3, method: "resources/list", params: {} },
  { jsonrpc: "2.0", id: 4, method: "resources/read", params: { uri: "membrane://protocol/v1" } },
]);
assert.match(rows[0].result.instructions, /federated context/i);
const tools = rows[1].result.tools.map((tool) => tool.name).sort();
assert.deepEqual(tools, ["membrane_checkpoint_load", "membrane_checkpoint_save", "membrane_context", "membrane_feedback", "membrane_knowledge_propose", "membrane_source_read"]);
assert.deepEqual(tools.filter((name) => /(?:^|_)(?:put|get|recall|doctor|schema|filesystem|plan_context)(?:$|_)/.test(name)), []);
assert.deepEqual(rows[2].result.resources, [{ uri: "membrane://protocol/v1", name: "Membrane protocol v1", mimeType: "text/markdown" }]);
assert.match(rows[3].result.contents[0].text, /federate/i);
assert.doesNotMatch(rows[3].result.contents[0].text, /plan_context/i);
for (const tool of rows[1].result.tools) {
  assert.ok(tool.outputSchema, `${tool.name} declares an output schema`);
  assert.equal(tool.inputSchema.properties.traceparent, undefined, `${tool.name} does not advertise traceparent as an argument`);
  assert.equal(tool.inputSchema.properties.tracestate, undefined, `${tool.name} does not advertise tracestate as an argument`);
  assert.equal(tool.inputSchema.properties.baggage, undefined, `${tool.name} does not advertise baggage as an argument`);
}

const registryRoot = await mkdtemp(join(tmpdir(), "membrane-server-"));
const enrolledRoot = join(registryRoot, "enrolled");
const foreignRoot = join(registryRoot, "foreign");
const registry = join(registryRoot, "registry.json");
await mkdir(enrolledRoot);
await mkdir(foreignRoot);
const enrolledCanonicalRoot = await realpath(enrolledRoot);
await writeFile(registry, JSON.stringify({
  schema_version: 1,
  bindings: { [enrolledCanonicalRoot]: { repository_id: "repo-a", scope_id: "scope-a", provider_config: {}, grant_policy: { level: "write-proposed" } } },
}), "utf8");

const legacySession = await rpc([
  { jsonrpc: "2.0", id: 30, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "legacy-session-test", version: "1.0.0" } } },
  {
    jsonrpc: "2.0", id: 31, method: "tools/call",
    params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: { root: enrolledRoot, repositoryId: "repo-a", scopeId: "scope-a" }, receiptId: "receipt-legacy-session", outcome: "used" } },
  },
], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.match(legacySession[0].result.instructions, /federated context/i);
assert.equal(legacySession[1].result.isError, false);
assert.ok(legacySession[1].result.structuredContent, "initialized legacy call includes structuredContent");
assert.equal(typeof legacySession[1].result.content?.[0]?.text, "string", "initialized legacy call retains text fallback");

const retiredAlias = await rpc([{
  jsonrpc: "2.0", id: 32, method: "tools/call",
  params: { name: "rightcontext_feedback", arguments: { repository: enrolledRoot, caller: { root: enrolledRoot, repositoryId: "repo-a", scopeId: "scope-a" }, receiptId: "retired-alias", outcome: "used" } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.equal(retiredAlias[0].error.code, -32602);
assert.match(retiredAlias[0].error.message, /rightcontext_feedback not found/i);

const denied = await rpc([{
  jsonrpc: "2.0", id: 4, method: "tools/call",
  params: { name: "membrane_context", arguments: { task: "inspect", repository: foreignRoot, caller: { root: enrolledRoot, repositoryId: "repo-a", scopeId: "scope-a" } } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.equal(denied[0].id, 4);
assert.equal(denied[0].result.isError, true);
assert.match(toolError(denied[0]), /not enrolled/i);

await writeFile(registry, "{corrupt", "utf8");
const corruptDenied = await rpc([{
  jsonrpc: "2.0", id: 5, method: "tools/call",
  params: { name: "membrane_context", arguments: { task: "inspect", repository: enrolledRoot, caller: { root: enrolledRoot, repositoryId: "repo-a", scopeId: "scope-a" } } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.equal(corruptDenied[0].result.isError, true);
assert.match(toolError(corruptDenied[0]), /registry unavailable/i);

await writeFile(registry, JSON.stringify({
  schema_version: 1,
  bindings: { [enrolledCanonicalRoot]: { repository_id: "repo-a", scope_id: "scope-a", provider_config: {}, grant_policy: { level: "write-proposed" } } },
}), "utf8");

const proposal = await rpc([{
  jsonrpc: "2.0", id: 6, method: "tools/call",
  params: { name: "membrane_knowledge_propose", arguments: { repository: enrolledRoot, caller: { root: enrolledRoot, repositoryId: "repo-a", scopeId: "scope-a" }, emission: { kind: "fact", text: "x".repeat(65_537) } } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.equal(proposal[0].result.isError, true);
assert.match(toolError(proposal[0]), /(?:bounded|too large|limit)/i);

const feedback = await rpc([{
  jsonrpc: "2.0", id: 7, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: { root: enrolledRoot, repositoryId: "repo-a", scopeId: "scope-a" }, receiptId: "receipt-1", outcome: "used" } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
const feedbackReceipt = JSON.parse(feedback[0].result.content[0].text);
assert.equal(feedbackReceipt.status, "accepted_advisory");
assert.equal(feedbackReceipt.durable, false);
assert.equal(feedbackReceipt.receiptId, "receipt-1");
assert.match(feedbackReceipt.feedbackId, /^[a-z0-9][a-z0-9_-]{7,}$/i);
assert.equal(feedback[0].result.isError, false);
assert.deepEqual(feedback[0].result.structuredContent.data, feedbackReceipt);
assert.equal(typeof feedback[0].result.content[0].text, "string");

await writeFile(registry, JSON.stringify({
  schema_version: 2,
  bindings: {
    [enrolledCanonicalRoot]: {
      repository_id: "repo-a", scope_id: "opaque-thread", provider_config: {}, grant_policy: { level: "write-proposed" },
      scope_descriptor: { kind: "virtual", id: "thread:abc-123", tenant_id: "tenant-a", parents: [], inherit_global: false },
    },
  },
}), "utf8");
const virtualCaller = { root: enrolledRoot, repositoryId: "repo-a", scopeId: "opaque-thread", scopeDescriptor: { kind: "virtual", id: "thread:abc-123", tenant_id: "tenant-a", parents: [], inherit_global: false } };
const virtualFeedback = await rpc([{
  jsonrpc: "2.0", id: 8, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: virtualCaller, receiptId: "receipt-virtual", outcome: "used" } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.equal(JSON.parse(virtualFeedback[0].result.content[0].text).status, "accepted_advisory");
const crossTenant = await rpc([{
  jsonrpc: "2.0", id: 9, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: { ...virtualCaller, scopeDescriptor: { ...virtualCaller.scopeDescriptor, tenant_id: "tenant-b" } }, receiptId: "receipt-denied", outcome: "used" } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.equal(crossTenant[0].result.isError, true);
assert.match(toolError(crossTenant[0]), /caller_scope_binding_denied/);
const virtualScopeMismatch = await rpc([{
  jsonrpc: "2.0", id: 15, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: { ...virtualCaller, scopeId: "opaque-thread-other" }, receiptId: "receipt-scope-denied", outcome: "used" } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.equal(virtualScopeMismatch[0].result.isError, true);
assert.match(toolError(virtualScopeMismatch[0]), /caller_scope_binding_denied/);

const modernMeta = {
  "io.modelcontextprotocol/protocolVersion": "2026-07-28",
  "io.modelcontextprotocol/clientCapabilities": {},
};
const modern = await rpc([
  { jsonrpc: "2.0", id: 10, method: "server/discover", params: { _meta: modernMeta } },
  { jsonrpc: "2.0", id: 11, method: "tools/list", params: { _meta: modernMeta } },
  {
    jsonrpc: "2.0", id: 12, method: "tools/call",
    params: {
      name: "membrane_feedback",
      arguments: { repository: enrolledRoot, caller: virtualCaller, receiptId: "receipt-modern", outcome: "used" },
      _meta: {
        ...modernMeta,
        traceparent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        tracestate: "\t1@system=foo \t, \t, vendor=bar\t",
        baggage: "tenant=foo=bar;meta=one, other=%80value;flag",
      },
    },
  },
  {
    jsonrpc: "2.0", id: 13, method: "tools/call",
    params: {
      name: "membrane_feedback",
      arguments: { repository: enrolledRoot, caller: virtualCaller, receiptId: "receipt-invalid-trace", outcome: "used" },
      _meta: { ...modernMeta, traceparent: "00-4BF92F3577B34DA6A3CE929D0E0E4736-00F067AA0BA902B7-01", tracestate: "vendor=foo", baggage: "tenant=foo=bar" },
    },
  },
  {
    jsonrpc: "2.0", id: 14, method: "tools/call",
    params: { name: "membrane_feedback", arguments: { repository: foreignRoot, caller: virtualCaller, receiptId: "receipt-modern-denied", outcome: "used" }, _meta: modernMeta },
  },
  {
    jsonrpc: "2.0", id: 16, method: "tools/call",
    params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: virtualCaller, receiptId: "receipt-no-parent", outcome: "used" }, _meta: { ...modernMeta, tracestate: "vendor=foo", baggage: "tenant=foo=bar" } },
  },
  {
    jsonrpc: "2.0", id: 17, method: "tools/call",
    params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: virtualCaller, receiptId: "receipt-duplicate-state", outcome: "used" }, _meta: { ...modernMeta, traceparent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01", tracestate: "vendor=foo,vendor=bar", baggage: "tenant=foo=bar" } },
  },
  {
    jsonrpc: "2.0", id: 18, method: "tools/call",
    params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: virtualCaller, receiptId: "receipt-raw-obs", outcome: "used" }, _meta: { ...modernMeta, traceparent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01", tracestate: "vendor=foo", baggage: "tenant=\x80value" } },
  },
  {
    jsonrpc: "2.0", id: 19, method: "tools/call",
    params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: virtualCaller, receiptId: "receipt-value-256", outcome: "used" }, _meta: { ...modernMeta, traceparent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01", tracestate: `vendor=${"x".repeat(256)}`, baggage: "tenant=foo=bar" } },
  },
  {
    jsonrpc: "2.0", id: 29, method: "tools/call",
    params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: virtualCaller, receiptId: "receipt-value-257", outcome: "used" }, _meta: { ...modernMeta, traceparent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01", tracestate: `vendor=${"x".repeat(257)}`, baggage: "tenant=foo=bar" } },
  },
], { MEMBRANE_PROJECT_REGISTRY: registry });
const modernById = new Map(modern.map((row) => [row.id, row]));
assert.deepEqual(modernById.get(10).result.supportedVersions, ["2026-07-28"]);
for (const tool of modernById.get(11).result.tools) {
  assert.ok(tool.outputSchema, `${tool.name} declares an output schema`);
}
for (const id of [12, 13, 16, 17, 18, 19, 29]) {
  const row = modernById.get(id);
  assert.ok(row.result, JSON.stringify(row));
  assert.equal(row.result.isError, false);
  assert.ok(row.result.structuredContent, "modern calls include structuredContent");
  assert.equal(typeof row.result.content?.[0]?.text, "string", "modern calls retain text fallback");
}
assert.deepEqual(modernById.get(12).result.structuredContent.trace, {
  traceparent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
  tracestate: "\t1@system=foo \t, \t, vendor=bar\t",
  baggage: "tenant=foo=bar;meta=one, other=%80value;flag",
});
assert.deepEqual(modernById.get(13).result.structuredContent.trace, { baggage: "tenant=foo=bar" });
assert.deepEqual(modernById.get(16).result.structuredContent.trace, { baggage: "tenant=foo=bar" });
assert.deepEqual(modernById.get(17).result.structuredContent.trace, {
  traceparent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01", baggage: "tenant=foo=bar",
});
assert.deepEqual(modernById.get(18).result.structuredContent.trace, {
  traceparent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01", tracestate: "vendor=foo",
});
assert.deepEqual(modernById.get(19).result.structuredContent.trace, {
  traceparent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01", tracestate: `vendor=${"x".repeat(256)}`, baggage: "tenant=foo=bar",
});
assert.deepEqual(modernById.get(29).result.structuredContent.trace, {
  traceparent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01", baggage: "tenant=foo=bar",
});
assert.equal(modernById.get(14).result.isError, true);
assert.match(toolError(modernById.get(14)), /not enrolled/i);

let federateHeaders;
let federateBody;
let federateHits = 0;
const federate = createServer((request, response) => {
  const chunks = [];
  request.on("data", (chunk) => chunks.push(chunk));
  request.on("end", () => {
    federateHits += 1;
    federateHeaders = request.headers;
    federateBody = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    const body = JSON.stringify({ providerStatus: "ready", packet: { blocks: [] }, receipts: [] });
    response.writeHead(200, { "content-type": "application/json", "content-length": Buffer.byteLength(body) });
    response.end(body);
  });
});
await new Promise((resolve, reject) => { federate.once("error", reject); federate.listen(0, "127.0.0.1", resolve); });
try {
  const { port } = federate.address();
  const invalidCalls = await rpc([
    { jsonrpc: "2.0", id: 22, method: "server/discover", params: { _meta: modernMeta } },
    { jsonrpc: "2.0", id: 23, method: "tools/call", params: { name: "membrane_context", arguments: { repository: enrolledRoot, caller: virtualCaller }, _meta: modernMeta } },
    { jsonrpc: "2.0", id: 24, method: "tools/call", params: { name: "membrane_context", arguments: { task: "", repository: enrolledRoot, caller: virtualCaller }, _meta: modernMeta } },
    { jsonrpc: "2.0", id: 25, method: "tools/call", params: { name: "membrane_context", arguments: { task: "bad budget type", repository: enrolledRoot, caller: virtualCaller, budget: "64" }, _meta: modernMeta } },
    { jsonrpc: "2.0", id: 26, method: "tools/call", params: { name: "membrane_context", arguments: { task: "negative budget", repository: enrolledRoot, caller: virtualCaller, budget: -1 }, _meta: modernMeta } },
    { jsonrpc: "2.0", id: 27, method: "tools/call", params: { name: "membrane_checkpoint_load", arguments: { repository: enrolledRoot, caller: virtualCaller, id: "checkpoint-1", asOfMs: -1 }, _meta: modernMeta } },
    { jsonrpc: "2.0", id: 28, method: "tools/call", params: { name: "membrane_checkpoint_load", arguments: { repository: enrolledRoot, caller: virtualCaller, id: "checkpoint-1", asOfMs: "yesterday" }, _meta: modernMeta } },
  ], { MEMBRANE_PROJECT_REGISTRY: registry, MEMRIGHT_PORT: String(port), MEMRIGHT_API_TOKEN: "test-token" });
  const invalidById = new Map(invalidCalls.map((row) => [row.id, row]));
  for (const id of [23, 24, 25, 26, 27, 28]) {
    assert.equal(invalidById.get(id).result.isError, true, `invalid request ${id} returns an invalid-params tool result`);
    assert.match(invalidById.get(id).result.content[0].text, /Input validation error/i);
  }
  assert.equal(federateHits, 0, "invalid tool arguments do not invoke the planner sentinel");
  const tracedContext = await rpc([
    { jsonrpc: "2.0", id: 20, method: "server/discover", params: { _meta: modernMeta } },
    {
      jsonrpc: "2.0", id: 21, method: "tools/call",
      params: {
        name: "membrane_context",
        arguments: { task: "trace context", repository: enrolledRoot, caller: virtualCaller },
        _meta: { ...modernMeta, traceparent: "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-future", tracestate: "membrane=server", baggage: "tenant=repo-a" },
      },
    },
  ], { MEMBRANE_PROJECT_REGISTRY: registry, MEMRIGHT_PORT: String(port), MEMRIGHT_API_TOKEN: "test-token" });
  const tracedById = new Map(tracedContext.map((row) => [row.id, row]));
  assert.deepEqual(tracedById.get(21).result.structuredContent.trace, {
    traceparent: "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-future", tracestate: "membrane=server", baggage: "tenant=repo-a",
  });
  assert.deepEqual({ traceparent: federateBody.traceparent, tracestate: federateBody.tracestate, baggage: federateBody.baggage }, {
    traceparent: "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-future", tracestate: "membrane=server", baggage: "tenant=repo-a",
  });
  assert.equal(federateHeaders.traceparent, "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-future");
  assert.equal(federateHeaders.tracestate, "membrane=server");
  assert.equal(federateHeaders.baggage, "tenant=repo-a");
  assert.equal(federateHeaders["x-membrane-trace"], "4bf92f3577b34da6a3ce929d0e0e4736");
  assert.equal(federateHeaders["x-rightcontext-trace"], "4bf92f3577b34da6a3ce929d0e0e4736");
  assert.equal(federateHeaders["x-membrane-version"], "membrane-mcp/1");
  assert.equal(federateHeaders["x-rightcontext-version"], "rightcontext-mcp/1");
  assert.equal(federateHits, 1, "only valid context calls invoke the planner sentinel");
} finally {
  await new Promise((resolve) => federate.close(resolve));
}
