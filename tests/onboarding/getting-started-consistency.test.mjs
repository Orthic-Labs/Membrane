// Native installed-path anti-drift checks for docs/product/getting-started.md.
import assert from "node:assert/strict";
import test from "node:test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));
const read = (path) => readFileSync(join(root, path), "utf8");
const doc = read("docs/product/getting-started.md");
const mcp = JSON.parse(read("mcp.json"));
const claudePlugin = JSON.parse(read(".claude-plugin/plugin.json"));
const tools = read("engine/crates/membrane-mcp/src/tools.rs");
const hub = read("engine/crates/membrane-protocol/src/hub.rs");
const product = read("docs/product/README.md");

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
      '"${CLAUDE_PLUGIN_ROOT}/membrane.exe" hook',
      event,
    );
    assert.doesNotMatch(command, /D:[\\/]Claude|node(?:\.exe)?|node_modules|\.mjs|(?:^|[\\/])(?:dist|target)(?:[\\/]|$)|python(?:\.exe)?/i);
  }
});

test("quickstart states native Windows runtime authority", () => {
  assert.match(product, /Current supported target is \*\*Windows\*\*/);
  assert.match(product, /visible native \*\*tray\*\* owns resident lifecycle/);
  assert.match(doc, /installed Windows package/);
  assert.match(doc, /Visible native tray owns (?:full )?resident/);
  assert.match(doc, /full resident lifecycle through its daemon/);
  assert.match(doc, /Hub dashboard is on demand/);
  assert.match(doc, /no agent-supplied Node or Python is required/);
  assert.match(doc, /Node 20\+ & pnpm 11 for development tooling only/);
});

test("membrane_context example matches native schema", () => {
  assert.match(doc, /"task":"orient me"/);
  assert.match(doc, /"repositoryId":"demo-repo"/);
  assert.match(doc, /"scopeId":"demo-scope"/);
  assert.match(tools, /"membrane_context" =>/);
  assert.match(tools, /vec!\[\s*"task",\s*"taskId",\s*"sessionId",\s*"repository",\s*"caller",\s*"remainingContextCeiling"/s);
  assert.match(tools, /required":\["root","repositoryId","scopeId"\]/);
});

test("Hub-off expectation matches native explicit-operation contract", () => {
  assert.match(doc, /execute through installed subsystem owners/);
  assert.match(doc, /No watcher, scheduler or Hub process should start/);
  assert.match(hub, /pub struct MembraneUnavailableV1/);
  assert.match(hub, /HubInactive/);
});

test("Blueprint lifecycle language preserves native installed contract", () => {
  assert.match(doc, /Blueprint is a native installed service/);
  assert.match(doc, /automatic watcher refresh requires active tray-owned daemon/);
  assert.match(doc, /`not_configured`/);
  assert.match(doc, /`degraded`/);
  assert.match(doc, /`blueprint_unavailable`/);
  assert.match(doc, /bounded (?:one-shot|work with Hub off)/);
});

test("offline fixture remains explicitly synthetic", () => {
  assert.match(doc, /node docs\/reference\/examples\/quickstart\/run\.mjs/);
  assert.match(doc, /node docs\/reference\/examples\/quickstart\/run\.mjs --degraded/);
  assert.ok(existsSync(join(root, "docs/reference/examples/quickstart/run.mjs")));
  assert.match(doc, /evidenceAuthority: synthetic/);
});
