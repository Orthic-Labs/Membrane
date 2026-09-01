// D08: pure init plan builder — returns planned file edits, MCP entries,
// hooks, watcher enrollment, first-build command, and uninstall instructions
// WITHOUT writing anything.

import { join } from "node:path";
import { detectHosts, HOSTS } from "./detect-hosts.mjs";

export const INIT_PLAN_VERSION = 1;
const SCOPES = Object.freeze(["project", "user"]);

function instructionPath(root, host) {
  if (host === "claude-code") return join(root, "CLAUDE.md");
  if (host === "codex") return join(root, "AGENTS.md");
  if (host === "cursor") return join(root, ".cursor", "rules", "blueprint.mdc");
  return join(root, "BLUEPRINT-AGENT.md");
}

export function buildInitPlan({
  root = process.cwd(),
  host = "auto",
  scope = "project",
  mcp = "auto",
  watch = "auto",
  hooks = "none",
  policy = "advisory",
} = {}) {
  const hosts = detectHosts({ explicit: host, root });
  if (!SCOPES.includes(scope)) throw new Error("--scope must be project or user");
  const actions = [];
  const files = [];
  for (const h of hosts) {
    const path = instructionPath(root, h);
    files.push({ path, kind: "file-edit", reversible: true, host: h });
    actions.push({ id: `write-host-instructions-${h}`, kind: "file-edit", path, reversible: true });
  }
  const mcpEnabled = mcp === "on" || (mcp === "auto" && hosts.includes("claude-code"));
  if (mcpEnabled) {
    const path = join(root, ".mcp.json");
    files.push({ path, kind: "file-edit", reversible: true, host: "claude-code" });
    actions.push({ id: "install-mcp", kind: "file-edit", path, reversible: true });
  }
  const watchEnabled = watch === "on" || (watch === "auto" && scope === "project");
  if (watchEnabled) {
    actions.push({ id: "enroll-watch", kind: "service", reversible: true });
  }
  if (hooks !== "none") {
    actions.push({ id: "install-hooks", kind: "hooks", level: hooks, reversible: true });
  }
  actions.push({ id: "build-generation", kind: "command", reversible: false });
  return {
    schemaVersion: INIT_PLAN_VERSION,
    root,
    hosts,
    scope,
    policy,
    mcpEnabled,
    watchEnabled,
    actions,
    files,
    uninstallCommand: `blueprint uninstall --root ${root}`,
  };
}
