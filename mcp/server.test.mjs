import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, mkdir, realpath, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const server = fileURLToPath(new URL("./server.mjs", import.meta.url));

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

const rows = await rpc([
  { jsonrpc: "2.0", id: 1, method: "initialize", params: {} },
  { jsonrpc: "2.0", id: 2, method: "tools/list", params: {} },
  { jsonrpc: "2.0", id: 3, method: "resources/read", params: { uri: "membrane://protocol/v1" } },
]);
assert.match(rows[0].result.instructions, /federated context/i);
const tools = rows[1].result.tools.map((tool) => tool.name).sort();
assert.deepEqual(tools, ["membrane_checkpoint_load", "membrane_checkpoint_save", "membrane_context", "membrane_feedback", "membrane_knowledge_propose", "membrane_source_read"]);
assert.deepEqual(tools.filter((name) => /(?:^|_)(?:put|get|recall|doctor|schema|filesystem|plan_context)(?:$|_)/.test(name)), []);
assert.match(rows[2].result.contents[0].text, /federate/i);
assert.doesNotMatch(rows[2].result.contents[0].text, /plan_context/i);

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

const denied = await rpc([{
  jsonrpc: "2.0", id: 4, method: "tools/call",
  params: { name: "membrane_context", arguments: { task: "inspect", repository: foreignRoot } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.match(denied[0].error.message, /not enrolled/i);

await writeFile(registry, "{corrupt", "utf8");
const corruptDenied = await rpc([{
  jsonrpc: "2.0", id: 5, method: "tools/call",
  params: { name: "membrane_context", arguments: { task: "inspect", repository: enrolledRoot } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.match(corruptDenied[0].error.message, /registry unavailable/i);

await writeFile(registry, JSON.stringify({
  schema_version: 1,
  bindings: { [enrolledCanonicalRoot]: { repository_id: "repo-a", scope_id: "scope-a", provider_config: {}, grant_policy: { level: "write-proposed" } } },
}), "utf8");

const proposal = await rpc([{
  jsonrpc: "2.0", id: 6, method: "tools/call",
  params: { name: "membrane_knowledge_propose", arguments: { emission: { kind: "fact", text: "x".repeat(65_537) } } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.match(proposal[0].error.message, /(?:bounded|too large|limit)/i);

const feedback = await rpc([{
  jsonrpc: "2.0", id: 7, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: { root: enrolledRoot, repositoryId: "repo-a", scopeId: "scope-a" }, receiptId: "receipt-1", outcome: "used" } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
const feedbackReceipt = JSON.parse(feedback[0].result.content[0].text);
assert.equal(feedbackReceipt.status, "accepted");
assert.equal(feedbackReceipt.receiptId, "receipt-1");
assert.match(feedbackReceipt.feedbackId, /^[a-z0-9][a-z0-9_-]{7,}$/i);

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
assert.equal(JSON.parse(virtualFeedback[0].result.content[0].text).status, "accepted");
const crossTenant = await rpc([{
  jsonrpc: "2.0", id: 9, method: "tools/call",
  params: { name: "membrane_feedback", arguments: { repository: enrolledRoot, caller: { ...virtualCaller, scopeDescriptor: { ...virtualCaller.scopeDescriptor, tenant_id: "tenant-b" } }, receiptId: "receipt-denied", outcome: "used" } },
}], { MEMBRANE_PROJECT_REGISTRY: registry });
assert.match(crossTenant[0].error.message, /caller_scope_binding_denied/);
