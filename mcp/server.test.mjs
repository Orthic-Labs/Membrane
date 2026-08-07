import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, mkdir, readFile, realpath, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildRepositoryCatalog } from "./repository-catalog.mjs";

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
  let errorOutput = "";
  const expectedResponses = messages.filter((message) => message.id !== undefined).length;
  let responsesReady;
  const responsePromise = new Promise((resolve) => { responsesReady = resolve; });
  const closePromise = new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", (code, signal) => resolve({ code, signal }));
  });
  child.stdout.on("data", (chunk) => {
    output += chunk;
    if (output.split("\n").filter(Boolean).length >= expectedResponses) responsesReady();
  });
  child.stderr.on("data", (chunk) => { errorOutput += chunk; });
  child.stdin.write(messages.map(JSON.stringify).join("\n") + "\n");
  const timeout = new Promise((_, reject) => {
    setTimeout(() => reject(new Error(`RPC timed out waiting for ${expectedResponses} responses`)), 60_000).unref();
  });
  await Promise.race([
    responsePromise,
    closePromise.then(({ code, signal }) => {
      throw new Error(`RPC closed before every response (code=${code}, signal=${signal}): ${errorOutput.trim()}`);
    }),
    timeout,
  ]);
  child.stdin.end();
  const { code, signal } = await closePromise;
  assert.equal(code, 0, `RPC exited code=${code}, signal=${signal}: ${errorOutput.trim()}`);
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
assert.deepEqual(tools, ["membrane_checkpoint_load", "membrane_checkpoint_save", "membrane_context", "membrane_feedback", "membrane_knowledge_propose", "membrane_scratchpad", "membrane_source_read", "membrane_temporal_fact", "membrane_working_context"]);
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
const advisoryEnv = { MEMBRANE_PROJECT_REGISTRY: registry, MEMBRANE_DURABILITY_MODE: "advisory" };

const legacySession = await rpc([
  { jsonrpc: "2.0", id: 30, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "legacy-session-test", version: "1.0.0" } } },
  {
    jsonrpc: "2.0", id: 31, method: "tools/call",
    params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: { root: enrolledRoot, repositoryId: "repo-a", scopeId: "scope-a" }, receiptId: "receipt-legacy-session", outcome: "used" } },
  },
], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.match(legacySession[0].result.instructions, /federated context/i);
assert.equal(legacySession[1].result.isError, true);
assert.match(toolError(legacySession[1]), /durable feedback write failed/i);

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
}], advisoryEnv);
const feedbackReceipt = JSON.parse(feedback[0].result.content[0].text);
assert.equal(feedbackReceipt.status, "accepted_advisory");
assert.equal(feedbackReceipt.durable, false);
assert.equal(feedbackReceipt.receiptId, "receipt-1");
assert.match(feedbackReceipt.feedbackId, /^[a-z0-9][a-z0-9_-]{7,}$/i);
assert.equal(feedback[0].result.isError, false);
assert.deepEqual(feedback[0].result.structuredContent.data, feedbackReceipt);
assert.equal(typeof feedback[0].result.content[0].text, "string");

// F09: an unresolvable binding (runtime.json present but failing validation) is a distinct
// hard failure -- advisory mode must NEVER convert it into a success shape.
const corruptRoot = await mkdtemp(join(tmpdir(), "membrane-corrupt-binding-"));
await mkdir(join(corruptRoot, "tools", "lib", "memory"), { recursive: true });
await writeFile(join(corruptRoot, "tools", "lib", "memory", "runtime.json"), JSON.stringify({ schemaVersion: 1, serviceId: "crypt-local-v1", host: "127.0.0.1", port: 70 }), "utf8");
const corruptEnrolled = join(corruptRoot, "enrolled");
await mkdir(corruptEnrolled, { recursive: true });
const corruptEnrolledCanonical = await realpath(corruptEnrolled);
const corruptRegistry = join(corruptRoot, "registry.json");
await writeFile(corruptRegistry, JSON.stringify({
  schema_version: 1,
  bindings: { [corruptEnrolledCanonical]: { repository_id: "repo-corrupt", scope_id: "scope-corrupt", provider_config: {}, grant_policy: { level: "write-proposed" } } },
}), "utf8");
const corruptAdvisoryEnv = { MEMBRANE_PROJECT_REGISTRY: corruptRegistry, MEMBRANE_DURABILITY_MODE: "advisory" };
const corruptCaller = { root: corruptEnrolled, repositoryId: "repo-corrupt", scopeId: "scope-corrupt" };
const unresolvableFeedback = await rpc([{
  jsonrpc: "2.0", id: 40, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: corruptEnrolled, caller: corruptCaller, receiptId: "receipt-unresolvable", outcome: "used" } },
}], corruptAdvisoryEnv);
assert.equal(unresolvableFeedback[0].result.isError, true, "an unresolvable binding must not be downgraded to a success shape");
assert.match(toolError(unresolvableFeedback[0]), /binding could not be resolved to a real installation/i);
const unresolvableProposal = await rpc([{
  jsonrpc: "2.0", id: 41, method: "tools/call",
  params: { name: "membrane_knowledge_propose", arguments: { repository: corruptEnrolled, caller: corruptCaller, emission: { text: "unresolvable" } } },
}], corruptAdvisoryEnv);
assert.equal(unresolvableProposal[0].result.isError, true, "an unresolvable binding must not be downgraded to a success shape");
assert.match(toolError(unresolvableProposal[0]), /binding could not be resolved to a real installation/i);

// F09: a genuine store-write failure IS eligible for advisory downgrade -- but only when the
// BINDING itself says so (grant_policy.durability), independent of the process-wide env var.
const bogusCrypt = "/nonexistent/membrane-test-crypt-binary";
const perBindingRoot = await mkdtemp(join(tmpdir(), "membrane-per-binding-advisory-"));
const perBindingEnrolled = join(perBindingRoot, "enrolled");
await mkdir(perBindingEnrolled, { recursive: true });
const perBindingCanonical = await realpath(perBindingEnrolled);
const perBindingRegistry = join(perBindingRoot, "registry.json");
await writeFile(perBindingRegistry, JSON.stringify({
  schema_version: 1,
  bindings: { [perBindingCanonical]: { repository_id: "repo-per-binding", scope_id: "scope-per-binding", provider_config: {}, grant_policy: { level: "write-proposed", durability: "advisory" } } },
}), "utf8");
const perBindingCaller = { root: perBindingEnrolled, repositoryId: "repo-per-binding", scopeId: "scope-per-binding" };
const perBindingFeedback = await rpc([{
  jsonrpc: "2.0", id: 42, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: perBindingEnrolled, caller: perBindingCaller, receiptId: "receipt-per-binding", outcome: "used" } },
}], { MEMBRANE_PROJECT_REGISTRY: perBindingRegistry, CRYPT_BIN: bogusCrypt }); // no MEMBRANE_DURABILITY_MODE env at all
const perBindingReceipt = JSON.parse(perBindingFeedback[0].result.content[0].text);
assert.equal(perBindingFeedback[0].result.isError, false, toolError(perBindingFeedback[0]));
assert.equal(perBindingReceipt.status, "accepted_advisory", "grant_policy.durability=advisory downgrades a store-write failure without the env var");

// An explicit per-binding "durable" policy must never be masked by the process-wide advisory
// env override, even when that env var is set.
const perBindingDurableRoot = await mkdtemp(join(tmpdir(), "membrane-per-binding-durable-"));
const perBindingDurableEnrolled = join(perBindingDurableRoot, "enrolled");
await mkdir(perBindingDurableEnrolled, { recursive: true });
const perBindingDurableCanonical = await realpath(perBindingDurableEnrolled);
const perBindingDurableRegistry = join(perBindingDurableRoot, "registry.json");
await writeFile(perBindingDurableRegistry, JSON.stringify({
  schema_version: 1,
  bindings: { [perBindingDurableCanonical]: { repository_id: "repo-per-binding-durable", scope_id: "scope-per-binding-durable", provider_config: {}, grant_policy: { level: "write-proposed", durability: "durable" } } },
}), "utf8");
const perBindingDurableFeedback = await rpc([{
  jsonrpc: "2.0", id: 43, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: perBindingDurableEnrolled, caller: { root: perBindingDurableEnrolled, repositoryId: "repo-per-binding-durable", scopeId: "scope-per-binding-durable" }, receiptId: "receipt-per-binding-durable", outcome: "used" } },
}], { MEMBRANE_PROJECT_REGISTRY: perBindingDurableRegistry, MEMBRANE_DURABILITY_MODE: "advisory", CRYPT_BIN: bogusCrypt });
assert.equal(perBindingDurableFeedback[0].result.isError, true, "grant_policy.durability=durable must never be masked by the process-wide advisory env override");
assert.match(toolError(perBindingDurableFeedback[0]), /durable feedback write failed/i);

// C2-authz: repository-catalog wiring into authorize() -- a workspace-root-bound caller may
// reach a distinct child repository ONLY via an explicit grant_policy.child_repository_ids
// grant that the live catalog also confirms; every other cross-root call still denies exactly
// as before.
const catalogRoot = await mkdtemp(join(tmpdir(), "membrane-catalog-authz-"));
const catalogWorkspace = join(catalogRoot, "workspace");
await mkdir(catalogWorkspace, { recursive: true });
const grantedChildDir = join(catalogWorkspace, "granted-child");
const ungrantedChildDir = join(catalogWorkspace, "ungranted-child");
await mkdir(join(grantedChildDir, ".git"), { recursive: true });
await mkdir(join(ungrantedChildDir, ".git"), { recursive: true });
const catalogWorkspaceCanonical = await realpath(catalogWorkspace);
const catalog = await buildRepositoryCatalog(catalogWorkspace);
const workspaceEntry = catalog.repositories.find((entry) => entry.role === "workspace-root");
const grantedEntry = catalog.repositories.find((entry) => entry.root === "granted-child");
const ungrantedEntry = catalog.repositories.find((entry) => entry.root === "ungranted-child");
const catalogRegistry = join(catalogRoot, "registry.json");
await writeFile(catalogRegistry, JSON.stringify({
  schema_version: 2,
  bindings: {
    [catalogWorkspaceCanonical]: { repository_id: workspaceEntry.repository_id, scope_id: workspaceEntry.scope_id, provider_config: {}, grant_policy: { level: "write-proposed", child_repository_ids: [grantedEntry.repository_id] } },
    [await realpath(grantedChildDir)]: { repository_id: grantedEntry.repository_id, scope_id: grantedEntry.scope_id, provider_config: {}, grant_policy: { level: "write-proposed", parent_repository_id: workspaceEntry.repository_id } },
    [await realpath(ungrantedChildDir)]: { repository_id: ungrantedEntry.repository_id, scope_id: ungrantedEntry.scope_id, provider_config: {}, grant_policy: { level: "write-proposed", parent_repository_id: workspaceEntry.repository_id } },
  },
}), "utf8");
const catalogEnv = { MEMBRANE_PROJECT_REGISTRY: catalogRegistry, MEMBRANE_DURABILITY_MODE: "advisory", CRYPT_BIN: bogusCrypt };
const workspaceCaller = { root: catalogWorkspace, repositoryId: workspaceEntry.repository_id, scopeId: workspaceEntry.scope_id };
const grantedAccess = await rpc([{
  jsonrpc: "2.0", id: 44, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: grantedChildDir, caller: workspaceCaller, receiptId: "receipt-granted-child", outcome: "used" } },
}], catalogEnv);
assert.equal(grantedAccess[0].result.isError, false, `explicit child grant must authorize cross-repository access: ${toolError(grantedAccess[0])}`);
const ungrantedAccess = await rpc([{
  jsonrpc: "2.0", id: 45, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: ungrantedChildDir, caller: workspaceCaller, receiptId: "receipt-ungranted-child", outcome: "used" } },
}], catalogEnv);
assert.equal(ungrantedAccess[0].result.isError, true, "an ungranted child repository must still be denied");
assert.match(toolError(ungrantedAccess[0]), /cross_root_binding_denied/);
const revokedAccess = await rpc([{
  jsonrpc: "2.0", id: 46, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: catalogWorkspace, caller: { root: grantedChildDir, repositoryId: grantedEntry.repository_id, scopeId: grantedEntry.scope_id }, receiptId: "receipt-child-cannot-reach-parent", outcome: "used" } },
}], catalogEnv);
assert.equal(revokedAccess[0].result.isError, true, "a child repository must not gain reverse access to its parent workspace");
assert.match(toolError(revokedAccess[0]), /cross_root_binding_denied/);

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
}], advisoryEnv);
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
], advisoryEnv);
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
  ], { MEMBRANE_PROJECT_REGISTRY: registry, CRYPT_PORT: String(port), CRYPT_API_TOKEN: "test-token" });
  const invalidById = new Map(invalidCalls.map((row) => [row.id, row]));
  for (const id of [23, 24, 25, 26, 27, 28]) {
    assert.equal(invalidById.get(id).result.isError, true, `invalid request ${id} returns an invalid-params tool result`);
    assert.match(invalidById.get(id).result.content[0].text, /Input validation error/i);
  }
  assert.equal(federateHits, 0, "invalid tool arguments do not invoke the planner forge");
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
  ], { MEMBRANE_PROJECT_REGISTRY: registry, CRYPT_PORT: String(port), CRYPT_API_TOKEN: "test-token" });
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
  assert.equal(federateHits, 1, "only valid context calls invoke the planner forge");
} finally {
  await new Promise((resolve) => federate.close(resolve));
}

// MBR-001 (SN-NODE-01): remove the CommonJS `require()` from the ESM workspace
// federation branch. Before this task the workspace branch called
// require("node:path").resolve(...) under ESM, which throws ReferenceError:
// require is not defined in ESM. Import `resolve` normally and drive exactly
// the acceptance: an executable test invokes scope=workspace and reaches the
// first child repository without a ReferenceError.
const workspaceRoot = await mkdtemp(join(tmpdir(), "membrane-workspace-catalog-"));
await mkdir(join(workspaceRoot, "child", ".git"), { recursive: true }); // child repo marker => child-repository catalog entry
const workspaceCanonical = await realpath(workspaceRoot);
await realpath(join(workspaceRoot, "child"));
const workspaceRegistry = join(workspaceRoot, "registry.json");
await writeFile(workspaceRegistry, JSON.stringify({
  schema_version: 1,
  bindings: {
    [workspaceCanonical]: { repository_id: "ws-repo", scope_id: "ws-scope", provider_config: {}, grant_policy: { level: "read-only" } },
  },
}), "utf8");
// A loopback port with no listener so the per-child federation clients fail fast
// and deterministically (they emit a typed fallback envelope rather than hanging).
const deadCryptPort = 65533;
const wsCaller = { root: workspaceCanonical, repositoryId: "ws-repo", scopeId: "ws-scope" };
// MBR-004 bounded routing: select the workspace root and the child explicitly so
// the task still fans out to the first child; a no-match task would abstain.
const wsFixtureCatalog = await buildRepositoryCatalog(workspaceCanonical);
const wsRootEntry = wsFixtureCatalog.repositories.find((entry) => entry.role === "workspace-root");
const wsChildEntry = wsFixtureCatalog.repositories.find((entry) => entry.role === "child-repository");
const workspaceContext = await rpc([{
  jsonrpc: "2.0", id: 70, method: "tools/call",
  params: {
    name: "membrane_context",
    arguments: { task: "inspect", repository: workspaceCanonical, caller: wsCaller, scope: "workspace", explicitRepositoryIds: [wsRootEntry?.repository_id, wsChildEntry?.repository_id].filter(Boolean) },
  },
}], { MEMBRANE_PROJECT_REGISTRY: workspaceRegistry, CRYPT_PORT: String(deadCryptPort), CRYPT_API_TOKEN: "test-token", MEMBRANE_DURABILITY_MODE: "advisory" });
const workspaceRow = workspaceContext[0];
assert.equal(workspaceRow.result.isError, false, `scope=workspace must reach the first child without ReferenceError: ${toolError(workspaceRow)}`);
assert.doesNotMatch(toolError(workspaceRow), /ReferenceError/);
const fused = workspaceRow.result.structuredContent.data;
assert.equal(fused.ok, true);
assert.equal(fused.scope, "workspace");
assert.ok(Array.isArray(fused.repos), "scope=workspace returns a per-repository fan-out trace");
assert.ok(fused.repos.length >= 2, `workspace fan-out reaches the workspace root and a child repo, got ${fused.repos.length}`);
assert.ok(fused.repos.some((entry) => entry.repoId !== null), "fan-out produced a resolved repo entry");
assert.doesNotMatch(JSON.stringify(fused.repos), /ReferenceError/);

// MBR-002 integration (SN-NODE-02): a read-only workspace caller must not be
// able to invoke a mutating operation on a write-trusted child even though the
// child binding alone claims write-trusted; effective privilege is the
// monotone intersection, not the target's declared level.
const mbr2Root = await mkdtemp(join(tmpdir(), "membrane-mbr002-"));
await mkdir(join(mbr2Root, "child", ".git"), { recursive: true });
const mbr2Workspace = await realpath(mbr2Root);
const mbr2Child = await realpath(join(mbr2Root, "child"));
const mbr2Catalog = await buildRepositoryCatalog(mbr2Workspace);
const mbr2WsEntry = mbr2Catalog.repositories.find((entry) => entry.role === "workspace-root");
const mbr2ChildEntry = mbr2Catalog.repositories.find((entry) => entry.role === "child-repository");
assert.ok(mbr2WsEntry && mbr2ChildEntry, "MBR-002 fixture catalog has a workspace root and one child");
const mbr2Registry = join(mbr2Root, "registry.json");
await writeFile(mbr2Registry, JSON.stringify({
  schema_version: 1,
  bindings: {
    [mbr2Workspace]: {
      repository_id: mbr2WsEntry.repository_id, scope_id: mbr2WsEntry.scope_id, provider_config: {},
      grant_policy: { level: "read-only", child_repository_ids: [mbr2ChildEntry.repository_id] },
    },
    [mbr2Child]: {
      repository_id: mbr2ChildEntry.repository_id, scope_id: mbr2ChildEntry.scope_id, provider_config: {},
      grant_policy: { level: "write-trusted", parent_repository_id: mbr2WsEntry.repository_id },
    },
  },
}), "utf8");
const mbr2Env = { MEMBRANE_PROJECT_REGISTRY: mbr2Registry, MEMBRANE_DURABILITY_MODE: "advisory", CRYPT_BIN: "/nonexistent/membrane-test-crypt-binary" };
const mbr2Caller = { root: mbr2Workspace, repositoryId: mbr2WsEntry.repository_id, scopeId: mbr2WsEntry.scope_id };
const mbr2Denied = await rpc([{
  jsonrpc: "2.0", id: 80, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: mbr2Child, caller: mbr2Caller, receiptId: "mbr2-child-feedback", outcome: "used" } },
}], mbr2Env);
assert.equal(mbr2Denied[0].result.isError, true, "read-only workspace caller must be denied a mutating child operation");
assert.match(toolError(mbr2Denied[0]), /caller_not_authorized/);

// MBR-003 (SN-NODE-02 / R03): every workspace target is authorized
// independently through the SAME primitive a direct call uses. An adversarial
// ungranted sibling is omitted with a typed denial receipt and is never
// invoked, while a granted child is still federated to.
const mbr3Root = await mkdtemp(join(tmpdir(), "membrane-mbr003-"));
await mkdir(join(mbr3Root, "granted", ".git"), { recursive: true });
await mkdir(join(mbr3Root, "ungranted", ".git"), { recursive: true });
const mbr3Workspace = await realpath(mbr3Root);
const mbr3Granted = await realpath(join(mbr3Root, "granted"));
const mbr3Ungranted = await realpath(join(mbr3Root, "ungranted"));
const mbr3Catalog = await buildRepositoryCatalog(mbr3Workspace);
const mbr3Ws = mbr3Catalog.repositories.find((entry) => entry.role === "workspace-root");
const mbr3G = mbr3Catalog.repositories.find((entry) => entry.role === "child-repository" && entry.root === "granted");
const mbr3U = mbr3Catalog.repositories.find((entry) => entry.role === "child-repository" && entry.root === "ungranted");
assert.ok(mbr3Ws && mbr3G && mbr3U, "MBR-003 fixture catalog has root plus granted and ungranted children");
const mbr3Registry = join(mbr3Root, "registry.json");
await writeFile(mbr3Registry, JSON.stringify({
  schema_version: 1,
  bindings: {
    [mbr3Workspace]: {
      repository_id: mbr3Ws.repository_id, scope_id: mbr3Ws.scope_id, provider_config: {},
      grant_policy: { level: "write-trusted", child_repository_ids: [mbr3G.repository_id] },
    },
    [mbr3Granted]: {
      repository_id: mbr3G.repository_id, scope_id: mbr3G.scope_id, provider_config: {},
      grant_policy: { level: "write-trusted", parent_repository_id: mbr3Ws.repository_id },
    },
    [mbr3Ungranted]: {
      repository_id: mbr3U.repository_id, scope_id: mbr3U.scope_id, provider_config: {},
      grant_policy: { level: "write-trusted", parent_repository_id: mbr3Ws.repository_id },
    },
  },
}), "utf8");
const mbr3Env = { MEMBRANE_PROJECT_REGISTRY: mbr3Registry, CRYPT_PORT: "65532", CRYPT_API_TOKEN: "test-token", MEMBRANE_DURABILITY_MODE: "advisory" };
const mbr3Caller = { root: mbr3Workspace, repositoryId: mbr3Ws.repository_id, scopeId: mbr3Ws.scope_id };
const mbr3Scope = await rpc([{
  jsonrpc: "2.0", id: 90, method: "tools/call",
  params: {
    name: "membrane_context",
    arguments: { task: "inspect", repository: mbr3Workspace, caller: mbr3Caller, scope: "workspace", explicitRepositoryIds: [mbr3Ws.repository_id, mbr3G.repository_id, mbr3U.repository_id] },
  },
}], mbr3Env);
assert.equal(mbr3Scope[0].result.isError, false, toolError(mbr3Scope[0]));
const mbr3Fused = mbr3Scope[0].result.structuredContent.data;
const mbr3ByRoot = new Map((mbr3Fused.repos || []).map((entry) => {
  const root = entry.repoId === mbr3G.repository_id ? "granted" : (entry.repoId === mbr3U.repository_id ? "ungranted" : "root");
  return [root, entry];
}));
assert.ok(mbr3ByRoot.has("root"), "workspace root target is reached");
assert.ok(mbr3ByRoot.has("granted"), "granted child target is reached");
assert.ok(mbr3ByRoot.get("granted").omissions.includes("planner_unavailable"), "granted child WAS invoked (client attempted federation)");
assert.ok(mbr3ByRoot.has("ungranted"), "ungranted sibling appears as a typed omission");
assert.deepEqual(mbr3ByRoot.get("ungranted").omissions, ["target_denied"], "ungranted sibling is omitted with a typed denial receipt");
assert.equal(mbr3ByRoot.get("ungranted").candidates, 0, "ungranted sibling is never consulted for candidates");
assert.ok(!mbr3ByRoot.get("ungranted").omissions.includes("planner_unavailable"), "ungranted sibling was never called");

// MBR-006 (R05): every workspace repository row carries a canonical generation
// identity or a typed unknown reason -- generationId (never a revision hub
// identity), manifestDigest, sourceCommit, identityStatus, identityReason.
for (const [key, row] of mbr3ByRoot.entries()) {
  assert.equal(typeof row.identityStatus, "string", `${key} row declares an identityStatus`);
  assert.ok(["known", "unknown"].includes(row.identityStatus), `${key} row identityStatus is known|unknown`);
  if (row.identityStatus === "known") assert.equal(row.identityReason, null);
  if (row.identityStatus === "unknown") assert.equal(typeof row.identityReason, "string");
  assert.ok(Object.prototype.hasOwnProperty.call(row, "manifestDigest"), `${key} row carries manifestDigest`);
  assert.ok(Object.prototype.hasOwnProperty.call(row, "sourceCommit"), `${key} row carries sourceCommit`);
  assert.equal(Object.prototype.hasOwnProperty.call(row, "revision"), false, `${key} row must not use the retired freshness.revision identity`);
}

// MBR-004 integration: a workspace task with no explicit or mentioned target
// abstains (reports target_selection_abstained) instead of querying every repo.
const mbr4Abstained = await rpc([{
  jsonrpc: "2.0", id: 91, method: "tools/call",
  params: {
    name: "membrane_context",
    arguments: { task: "entirely unrelated sentence without any repo alias", repository: mbr3Workspace, caller: mbr3Caller, scope: "workspace" },
  },
}], mbr3Env);
assert.equal(mbr4Abstained[0].result.isError, false, toolError(mbr4Abstained[0]));
const mbr4Data = mbr4Abstained[0].result.structuredContent.data;
assert.equal(mbr4Data.scope, "workspace");
assert.equal(mbr4Data.ok, false);
assert.equal(mbr4Data.error, "target_selection_abstained");
assert.ok(Array.isArray(mbr4Data.considered), "abstention reports the set of repositories considered");
assert.ok(!Array.isArray(mbr4Data.repos) || mbr4Data.repos.length === 0, "no repository was queried on abstention");

// MBR-007 (R06): exact task/turn/client/overlay envelopes are preserved end to
// end -- a fixture sends taskEnvelope and turnEnvelope together and the delivery
// carries the exact identities (never synthesized or dropped).
const mbr7TaskEnvelope = { schema: "orthic.task-envelope.v1", taskId: "task_alpha", text: "do the thing", intent: "implementation" };
const mbr7TurnEnvelope = { schema: "orthic.turn-envelope.v1", turnId: "turn_1", sessionId: "sess_a", sequence: 1 };
const mbr7ClientEnvelope = { schema: "orthic.client-envelope.v1", clientId: "fixture_client", adapterVersion: "1.0.0" };
const mbr7Call = await rpc([{
  jsonrpc: "2.0", id: 95, method: "tools/call",
  params: {
    name: "membrane_context",
    arguments: {
      task: "do the thing", repository: mbr3Workspace, caller: mbr3Caller, scope: "workspace",
      explicitRepositoryIds: [mbr3Ws.repository_id],
      taskEnvelope: mbr7TaskEnvelope, turnEnvelope: mbr7TurnEnvelope, clientEnvelope: mbr7ClientEnvelope,
    },
  },
}], mbr3Env);
assert.equal(mbr7Call[0].result.isError, false, toolError(mbr7Call[0]));
const mbr7Data = mbr7Call[0].result.structuredContent.data;
assert.equal(mbr7Data.taskEnvelope.schema, "orthic.task-envelope.v1");
assert.equal(mbr7Data.taskEnvelope.taskId, "task_alpha");
assert.equal(mbr7Data.turnEnvelope.turnId, "turn_1");
assert.equal(mbr7Data.turnEnvelope.sessionId, "sess_a");
assert.equal(mbr7Data.clientEnvelope.clientId, "fixture_client");
assert.equal(mbr7Data.overlay.worktreePath, mbr3Workspace, "overlay worktree identity is preserved");
