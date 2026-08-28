// Native installed-path anti-drift checks for docs/getting-started.md.
import assert from "node:assert/strict";
import test from "node:test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));
const read = (path) => readFileSync(join(root, path), "utf8");
const doc = read("docs/getting-started.md");
const mcp = JSON.parse(read("mcp.json"));
const claudePlugin = JSON.parse(read(".claude-plugin/plugin.json"));
const tools = read("engine/crates/membrane-mcp/src/tools.rs");
const hub = read("engine/crates/membrane-protocol/src/hub.rs");
const product = read("docs/product.md");

test("quickstart matches canonical native MCP entrypoint", () => {
  assert.equal(mcp.mcpServers.membrane.command, "membrane");
  assert.deepEqual(mcp.mcpServers.membrane.args, ["stdio-mcp"]);
  assert.match(doc, /"command": "membrane"/);
  assert.match(doc, /"args": \["stdio-mcp"\]/);
  assert.doesNotMatch(doc, /node mcp\/server\.mjs/);
});

test("Claude projection is installed-path bound & ships hooks", () => {
  const server = claudePlugin.mcpServers?.membrane;
  assert.equal(server?.command, "${CLAUDE_PLUGIN_ROOT}/membrane.exe");
  assert.deepEqual(server?.args, ["stdio-mcp"]);
  const hookEvents = ["SessionStart", "UserPromptSubmit", "PreCompact", "PostCompact", "PreToolUse", "PostToolUse", "PostToolUseFailure", "Stop", "TaskCompleted", "SessionEnd"];
  for (const event of hookEvents) {
    const hooks = claudePlugin.hooks?.[event];
    assert.ok(Array.isArray(hooks) && hooks.length > 0, event);
    const command = hooks[0].hooks?.[0]?.command;
    assert.equal(
      command,
      '"${CLAUDE_PLUGIN_ROOT}/runtime/blueprint/lib/node.exe" "${CLAUDE_PLUGIN_ROOT}/mcp/hooks/membrane-hook-entrypoint.mjs"',
      event,
    );
    assert.doesNotMatch(command, /D:[\\/]Claude|node_modules|(?:^|[\\/])(?:dist|target)(?:[\\/]|$)|python(?:\.exe)?/i);
  }
});

test("quickstart states native Windows runtime authority", () => {
  assert.match(product, /Current supported target is \*\*Windows\*\*/);
  assert.match(product, /Membrane Hub.*sole resident service authority/);
  assert.match(doc, /signed Windows install/);
  assert.match(doc, /Membrane Hub owns the resident/);
  assert.match(doc, /Node & Python are development\/test tooling/);
});

test("membrane_context example matches native schema", () => {
  assert.match(doc, /"task":"orient me"/);
  assert.match(doc, /"repositoryId":"demo-repo"/);
  assert.match(doc, /"scopeId":"demo-scope"/);
  assert.match(tools, /"membrane_context" =>/);
  assert.match(tools, /vec!\["task", "repository", "caller"\]/);
  assert.match(tools, /required":\["root","repositoryId","scopeId"\]/);
});

test("Hub-off expectation matches native typed contract", () => {
  assert.match(doc, /"kind":"membrane_unavailable","reason":"hub_inactive","retryable":true/);
  assert.match(hub, /kind: "membrane_unavailable"/);
  assert.match(hub, /reason: MembraneUnavailableReasonV1::HubInactive/);
  assert.match(hub, /retryable: true/);
});

test("Blueprint lifecycle language preserves installed runtime contract", () => {
  assert.match(doc, /runtime\s+shipped by Membrane installer/);
  assert.match(doc, /Watcher freshness is Hub-coupled/);
  assert.match(doc, /`not_configured`/);
  assert.match(doc, /`degraded`/);
  assert.match(doc, /`blueprint_unavailable`/);
  assert.match(doc, /bounded one-shot/);
});

test("offline fixture remains explicitly synthetic", () => {
  assert.match(doc, /node docs\/examples\/quickstart\/run\.mjs/);
  assert.match(doc, /node docs\/examples\/quickstart\/run\.mjs --degraded/);
  assert.ok(existsSync(join(root, "docs/examples/quickstart/run.mjs")));
  assert.match(doc, /evidenceAuthority: synthetic/);
});
