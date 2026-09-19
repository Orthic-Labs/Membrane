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
const hooksManifest = JSON.parse(read("hooks/hooks.json"));
const codexHooks = JSON.parse(read("hooks/codex-hooks.json"));
const codexMcp = JSON.parse(read(".mcp.json"));
const codexPlugin = JSON.parse(read(".codex-plugin/plugin.json"));
const tools = read("engine/crates/membrane-mcp/src/tools.rs");
const hub = read("engine/crates/membrane-protocol/src/hub.rs");
const product = read("docs/product/README.md");

test("quickstart matches canonical native MCP entrypoint", () => {
  assert.equal(mcp.mcpServers.membrane.type, "streamable-http");
  assert.equal(mcp.mcpServers.membrane.url, "http://127.0.0.1:47851/mcp");
  assert.equal(mcp.mcpServers.membrane.headers?.Authorization, "Bearer ${MEMBRANE_BEARER_TOKEN}");
  assert.match(doc, /"type": "streamable-http"/);
  assert.match(doc, /"url": "http:\/\/127\.0\.0\.1:47851\/mcp"/);
  assert.match(doc, /Bearer \$\{MEMBRANE_BEARER_TOKEN\}/);
  assert.doesNotMatch(doc, /node mcp\/server\.mjs/);
});

test("Claude projection is installed-path bound & ships hooks", () => {
  const server = claudePlugin.mcpServers?.membrane;
  assert.equal(server?.type, "http");
  assert.equal(server?.url, "http://127.0.0.1:47851/mcp");
  assert.equal(server?.headers?.Authorization, "Bearer ${MEMBRANE_BEARER_TOKEN}");
  // The plugin hook surface lives in hooks/hooks.json (Claude auto-loads it
  // from the plugin root; inline plugin.json hooks would double-register).
  const hookEvents = ["SessionStart", "UserPromptSubmit", "PreCompact", "PostCompact", "PreToolUse", "PostToolUse", "PostToolUseFailure", "Stop", "TaskCompleted", "SessionEnd"];
  for (const event of hookEvents) {
    const hooks = hooksManifest.hooks?.[event];
    assert.ok(Array.isArray(hooks) && hooks.length > 0, event);
    const command = hooks[0].hooks?.[0]?.command;
    assert.equal(
      command,
      '"${CLAUDE_PLUGIN_ROOT}/membrane-client.exe" hook',
      event,
    );
    assert.doesNotMatch(command, /D:[\\/]Claude|node(?:\.exe)?|node_modules|\.mjs|(?:^|[\\/])(?:dist|target)(?:[\\/]|$)|python(?:\.exe)?/i);
  }
});

test("Codex projection uses verified bearer env binding & Codex event set", () => {
  // `.mcp.json` is the Codex plugin MCP declaration; `bearerTokenEnvVar` is
  // the declared bearer binding (Codex strips auth fields from plugin
  // servers — activation keeps the authenticated config.toml registration),
  // and no literal token may appear.
  assert.equal(codexMcp.mcpServers?.membrane?.url, "http://127.0.0.1:47851/mcp");
  assert.equal(codexMcp.mcpServers?.membrane?.bearerTokenEnvVar, "MEMBRANE_BEARER_TOKEN");
  assert.equal(codexPlugin.mcpServers, "./.mcp.json");
  assert.equal(codexPlugin.hooks, "./hooks/codex-hooks.json");
  const codexEvents = ["SessionStart", "UserPromptSubmit", "PreCompact", "PostCompact", "PreToolUse", "PostToolUse", "Stop", "SessionEnd"];
  for (const event of codexEvents) {
    const hooks = codexHooks.hooks?.[event];
    assert.ok(Array.isArray(hooks) && hooks.length > 0, event);
    // Codex substitutes ${PLUGIN_ROOT}/${CLAUDE_PLUGIN_ROOT} for plugin hooks
    // and strictly parses hook stdout (deny_unknown_fields), so hook commands
    // pin the Codex wire shape. Windows needs `commandWindows`: Codex runs
    // hooks through the detected user shell, PowerShell rejects `"path" arg`,
    // and powershell.exe -File is valid under cmd, PowerShell, and Git Bash.
    const handler = hooks[0].hooks?.[0];
    assert.equal(handler?.command, '"${PLUGIN_ROOT}/membrane-client" hook --wire=codex', event);
    assert.equal(handler?.commandWindows, 'powershell -NoProfile -ExecutionPolicy Bypass -File "${PLUGIN_ROOT}/membrane-hook.ps1"', event);
  }
  // Host event sets only carry events the host supports.
  assert.ok(!codexHooks.hooks?.PostToolUseFailure, "Codex lacks PostToolUseFailure");
  assert.ok(!codexHooks.hooks?.TaskCompleted, "Codex lacks TaskCompleted");
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
  // PUL-050: remainingContextCeiling moved out of `required` — budgetMode
  // (bounded_response vs host_fit) decides whether it must be supplied.
  assert.match(tools, /vec!\[\s*"task",\s*"taskId",\s*"sessionId",\s*"repository",\s*"caller",?\s*\]/s);
  assert.match(tools, /"remainingContextCeiling":remaining_context_ceiling\(\)/);
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
