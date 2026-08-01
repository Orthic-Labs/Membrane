import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { chmod, mkdtemp, mkdir, realpath, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
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

test("C1 durable proposal/feedback returns readback receipts across MCP restart", async () => {
  const root = await mkdtemp(join(tmpdir(), "membrane-durable-"));
  const enrolled = join(root, "enrolled");
  await mkdir(enrolled);
  const canonical = await realpath(enrolled);
  const registry = join(root, "registry.json");
  const fake = join(root, "memright-fixture.mjs");
  const store = join(root, "durable.store");
  await writeFile(registry, JSON.stringify({ schema_version: 1, bindings: { [canonical]: { repository_id: "repo-a", scope_id: "scope-a", provider_config: {}, grant_policy: { level: "write-proposed" } } } }));
  await writeFile(fake, `#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
let input = ""; process.stdin.on("data", c => input += c); process.stdin.on("end", () => {
  const command = process.argv[2]; const path = process.env.FAKE_STORE;
  if (command === "put") { writeFileSync(path, input.trim()); console.log(JSON.stringify({ put: "mem-durable-1" })); }
  else if (command === "get") console.log(readFileSync(path, "utf8"));
  else if (command === "feedback") console.log(JSON.stringify({ ok: true, verified: true }));
  else process.exit(2);
});
`);
  await chmod(fake, 0o755);
  const env = { MEMBRANE_PROJECT_REGISTRY: registry, MEMRIGHT_BIN: fake, FAKE_STORE: store };
  const request = (id, name, args) => ({ jsonrpc: "2.0", id, method: "tools/call", params: { name, arguments: args } });
  const common = { repository: enrolled, caller: { root: enrolled, repositoryId: "repo-a", scopeId: "scope-a" } };
  const first = await rpc([request(1, "membrane_knowledge_propose", { ...common, emission: { text: "durable proposal" } })], env);
  const proposal = JSON.parse(first[0].result.content[0].text);
  assert.equal(proposal.status, "persisted");
  assert.equal(proposal.durable, true);
  assert.equal(proposal.lifecycleReceipt.status, "persisted");
  const second = await rpc([request(2, "membrane_feedback", { ...common, receiptId: "candidate-1", outcome: "used" })], env);
  const feedback = JSON.parse(second[0].result.content[0].text);
  assert.equal(feedback.status, "persisted");
  assert.equal(feedback.lifecycleReceipt.status, "persisted");
  assert.equal(readFileSync(store, "utf8"), "durable proposal");
});
