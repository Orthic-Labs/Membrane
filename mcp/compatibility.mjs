// D31: MCP client/SDK compatibility matrix and host config generation.
// Generates host configs for Claude Code, Codex, Cursor, VS Code/Copilot,
// Continue, Windsurf/OpenCode, and generic MCP. Every generated config
// launches `cortex mcp serve`, implemented by the CX-B1 runtime adapter in
// scripts/cortex-mcp.mjs (six tools, nine resources, six prompts).

import { join } from "node:path";

export const MCP_COMPATIBILITY = Object.freeze([
  { sdk: "@modelcontextprotocol/sdk", major: 1, status: "supported" },
  { sdk: "@modelcontextprotocol/sdk", major: 0, status: "legacy" },
]);

export const HOSTS = Object.freeze([
  "claude-code",
  "codex",
  "cursor",
  "vscode-copilot",
  "continue",
  "windsurf-opencode",
  "generic",
]);

export function mcpConfigForHost(host, { root, serverCommand = "cortex" } = {}) {
  switch (host) {
    case "claude-code":
      return { path: join(root, ".mcp.json"), value: { mcpServers: { cortex: { command: serverCommand, args: ["mcp", "serve"] } } } };
    case "codex":
      return { path: join(root, ".codex", "config.toml"), value: `[mcp_servers.cortex]\ncommand = "${serverCommand}"\nargs = ["mcp", "serve"]\n` };
    case "cursor":
      return { path: join(root, ".cursor", "mcp.json"), value: { mcpServers: { cortex: { command: serverCommand, args: ["mcp", "serve"] } } } };
    case "vscode-copilot":
      return { path: join(root, ".vscode", "mcp.json"), value: { servers: { cortex: { command: serverCommand, args: ["mcp", "serve"] } } } };
    case "continue":
      return { path: join(root, ".continue", "config.json"), value: { mcpServers: [{ name: "cortex", command: serverCommand, args: ["mcp", "serve"] }] } };
    case "windsurf-opencode":
      return { path: join(root, ".windsurf", "mcp_config.json"), value: { mcpServers: { cortex: { command: serverCommand, args: ["mcp", "serve"] } } } };
    case "generic":
      return { path: join(root, ".mcp.json"), value: { mcpServers: { cortex: { command: serverCommand, args: ["mcp", "serve"] } } } };
    default:
      return null;
  }
}
