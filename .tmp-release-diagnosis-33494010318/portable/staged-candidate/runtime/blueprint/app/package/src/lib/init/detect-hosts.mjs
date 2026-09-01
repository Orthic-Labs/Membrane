// D08: deterministic host detection for `blueprint init`. Explicit flag wins;
// then existing host config files; then installed command probes; otherwise
// generic. Never installs more than the selected host set.

import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";

export const HOSTS = Object.freeze(["claude-code", "codex", "cursor", "generic"]);

const HOST_CONFIG_PATHS = Object.freeze({
  "claude-code": [".claude/settings.json", "CLAUDE.md", ".mcp.json"],
  codex: ["AGENTS.md", ".codex/config.toml"],
  cursor: [".cursor/rules/blueprint.mdc", ".cursor/mcp.json"],
  generic: ["BLUEPRINT-AGENT.md"],
});

const HOST_COMMANDS = Object.freeze({
  "claude-code": ["claude", "codex"],
  codex: ["codex"],
  cursor: ["cursor"],
});

function probeCommand(name) {
  // `command -v` respects PATH without spawning a shell pipeline; a missing
  // binary yields a non-zero exit and no output.
  const result = spawnSync("sh", ["-c", `command -v ${name} 2>/dev/null`], { encoding: "utf8" });
  return result.status === 0 && String(result.stdout ?? "").trim().length > 0;
}

export function detectHosts({ explicit = null, root = process.cwd() } = {}) {
  if (explicit) {
    const value = String(explicit);
    if (value === "auto") return detectHosts({ explicit: null, root });
    if (!HOSTS.includes(value)) throw new Error(`--host must be auto, ${HOSTS.join(", ")}`);
    return [value];
  }
  const detected = new Set();
  for (const host of HOSTS) {
    const hasConfig = HOST_CONFIG_PATHS[host].some((path) => existsSync(join(root, path)));
    if (hasConfig) detected.add(host);
  }
  if (detected.size === 0) {
    for (const host of HOSTS) {
      const found = (HOST_COMMANDS[host] ?? []).some((cmd) => probeCommand(cmd));
      if (found) detected.add(host);
    }
  }
  return detected.size ? [...detected] : ["generic"];
}
