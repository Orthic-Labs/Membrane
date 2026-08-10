// D08: apply an init plan with full rollback. Reuses the reversible installer
// state capture; applies only the generated plan and rolls back every
// completed action on failure.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { defaultConfigPath, readWatchConfig, writeWatchConfig } from "../../watchman/supervisor.mjs";
import { buildInitPlan } from "./plan.mjs";
import { removeInstallStateKey, sealInstallState, verifyInstallState } from "./state-integrity.mjs";

const START = "<!-- cortex:start -->";
const END = "<!-- cortex:end -->";
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const CORTEX_SCRIPT = join(SCRIPT_DIR, "..", "..", "scripts", "cortex.mjs");

function statePath(root) { return join(root, ".agent", "graph", "cortex-install-state.json"); }
function loadState(root) {
  if (!confined(root, statePath(root))) throw new Error("state_invalid");
  if (!existsSync(statePath(root))) return { version: 1, files: {} };
  try { return verifyInstallState(root, JSON.parse(readFileSync(statePath(root), "utf8"))); } catch { throw new Error("state_invalid"); }
}
function saveState(root, state) {
  if (!confined(root, statePath(root))) throw new Error("state_invalid");
  mkdirSync(dirname(statePath(root)), { recursive: true });
  writeFileSync(statePath(root), `${JSON.stringify(sealInstallState(root, state), null, 2)}\n`, "utf8");
}
function remember(state, path) {
  if (Object.hasOwn(state.files, path)) return;
  const bytes = existsSync(path) ? readFileSync(path) : null;
  state.files[path] = { exists: bytes !== null, content: bytes?.toString("utf8") ?? null, bytes: bytes?.toString("base64") ?? null };
}
function confined(root, path) {
  const canonicalRoot = realpathSync(root);
  const target = resolve(path);
  const rel = relative(canonicalRoot, target);
  if (!isAbsolute(path) || !rel || rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel)) return false;
  let ancestor = target;
  while (!existsSync(ancestor)) {
    const parent = dirname(ancestor);
    if (parent === ancestor) return false;
    ancestor = parent;
  }
  return !lstatSync(ancestor).isSymbolicLink() && (() => { const under = relative(canonicalRoot, realpathSync(ancestor)); return !under || (!under.startsWith(`..${sep}`) && under !== ".." && !isAbsolute(under)); })();
}
export function validateInstallState(root, state) {
  if (!state || typeof state !== "object" || state.version !== 1 || !state.files || typeof state.files !== "object" || Array.isArray(state.files)) throw new Error("state_invalid");
  for (const [path, original] of Object.entries(state.files)) {
    if (!confined(root, path) || !allowedTarget(root, path) || !original || typeof original !== "object" || typeof original.exists !== "boolean" || !/^[a-f0-9]{64}$/.test(original.installed ?? "")) throw new Error("state_invalid");
    if (original.exists && original.bytes !== null && original.bytes !== undefined && (typeof original.bytes !== "string" || Buffer.from(original.bytes, "base64").toString("base64") !== original.bytes)) throw new Error("state_invalid");
    if (original.exists && original.bytes == null && typeof original.content !== "string") throw new Error("state_invalid");
  }
  if (state.watch !== undefined && (!state.watch || typeof state.watch !== "object" || typeof state.watch.added !== "boolean")) throw new Error("state_invalid");
  return state;
}
function allowedTarget(root, path) {
  const base = resolve(root);
  return new Set(["CLAUDE.md", "AGENTS.md", "CORTEX-AGENT.md", ".mcp.json", join(".cursor", "rules", "cortex.mdc"), join(".claude", "settings.json")].map((name) => join(base, name))).has(resolve(path));
}
function recordInstalled(state, path) {
  state.files[path].installed = createHash("sha256").update(readFileSync(path)).digest("hex");
}
function removeBlock(content) {
  const escapedStart = START.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const escapedEnd = END.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return content.replace(new RegExp(`\\n?${escapedStart}[\\s\\S]*?${escapedEnd}\\n?`, "g"), "\n").replace(/^\n+$/, "");
}
function mergeBlock(content) {
  const block = `${START}\n## Cortex Graph\nBefore reading repository files, call \`cortex_orient\` with current repository root. Use \`cortex_expand\` for bounded context and \`cortex_search\` for queries.\n${END}`;
  const without = removeBlock(content);
  return `${without.trimEnd()}${without.trimEnd() ? "\n\n" : ""}${block}\n`;
}
function mergeJsonFile(state, path, update) {
  const current = existsSync(path) ? readFileSync(path, "utf8") : "{}";
  let value;
  try { value = JSON.parse(current); } catch { throw new Error(`${path} is not valid JSON`); }
  remember(state, path);
  const next = update(value);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(next, null, 2)}\n`, "utf8");
}

function restore(root, state) {
  for (const [path, original] of Object.entries(state.files ?? {})) {
    if (original.exists) {
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, typeof original.bytes === "string" ? Buffer.from(original.bytes, "base64") : (original.content ?? ""), "utf8");
    } else {
      try { rmSync(path, { force: true }); } catch {}
    }
  }
}

function safeWatchConfig(path) {
  let ancestor = resolve(path);
  while (!existsSync(ancestor)) {
    const parent = dirname(ancestor);
    if (parent === ancestor) throw new Error("watch_config_invalid");
    ancestor = parent;
  }
  if (lstatSync(ancestor).isSymbolicLink() || (existsSync(path) && lstatSync(path).isSymbolicLink())) throw new Error("watch_config_invalid");
  return resolve(path);
}

function enrollWatch(root, configPath) {
  const path = safeWatchConfig(configPath);
  const config = readWatchConfig(path);
  const canonicalRoot = realpathSync(root);
  if (config.repos.some((repo) => {
    try { return realpathSync(repo.root) === canonicalRoot; } catch { return resolve(repo.root) === canonicalRoot; }
  })) return false;
  writeWatchConfig({ ...config, repos: [...config.repos, { root: canonicalRoot, enabled: true }] }, path);
  return true;
}

function unenrollWatch(root, configPath) {
  const path = safeWatchConfig(configPath);
  const config = readWatchConfig(path);
  const canonicalRoot = realpathSync(root);
  writeWatchConfig({ ...config, repos: config.repos.filter((repo) => {
    try { return realpathSync(repo.root) !== canonicalRoot; } catch { return resolve(repo.root) !== canonicalRoot; }
  }) }, path);
}

export function applyInitPlan({ root = process.cwd(), plan = null, build = true, watchConfigPath = defaultConfigPath() } = {}) {
  const resolved = plan ?? buildInitPlan({ root });
  const completed = [];
  const buildRan = { ran: false };
  let state = { version: 1, files: {} };
  let stateTrusted = false, operationStarted = false;
  try {
    state = validateInstallState(root, loadState(root));
    stateTrusted = true;
    for (const action of resolved.actions) if (action.path && (!confined(root, action.path) || !allowedTarget(root, action.path))) throw new Error("state_invalid");
    for (const action of resolved.actions) {
      operationStarted = true;
      if (action.kind === "file-edit") {
        if (action.path.endsWith(".mcp.json") || action.path.endsWith("settings.json")) {
          mergeJsonFile(state, action.path, (value) => ({
            ...value,
            mcpServers: {
              ...(value.mcpServers ?? {}),
              cortex: { command: process.execPath, args: [join(SCRIPT_DIR, "..", "..", "scripts", "cortex-mcp.mjs"), "--root", root] },
            },
          }));
        } else {
          const current = existsSync(action.path) ? readFileSync(action.path, "utf8") : "";
          remember(state, action.path);
          writeFileSync(action.path, mergeBlock(current), "utf8");
        }
        recordInstalled(state, action.path);
      } else if (action.kind === "hooks") {
        // Hook installation is owned by its dedicated command; init records no unmodified file.
      } else if (action.kind === "service" && action.id === "enroll-watch") {
        state.watch = { added: enrollWatch(root, watchConfigPath) };
      } else if (action.kind === "command" && action.id === "build-generation" && build) {
        execFileSync(process.execPath, [CORTEX_SCRIPT, "graph", "build", "--out", ".agent"], { cwd: root, stdio: "ignore" });
        buildRan.ran = true;
      }
      completed.push(action.id);
    }
    saveState(root, state);
    return { ok: true, applied: completed, buildRan: buildRan.ran, uninstallCommand: resolved.uninstallCommand };
  } catch (error) {
    if (stateTrusted && operationStarted) try { restore(root, state); if (state.watch?.added) unenrollWatch(root, watchConfigPath); rmSync(statePath(root), { force: true }); removeInstallStateKey(root); } catch {}
    return { ok: false, applied: completed, error: String(error.message ?? error), uninstallCommand: resolved.uninstallCommand };
  }
}

export function uninstallInit({ root = process.cwd(), watchConfigPath = defaultConfigPath() } = {}) {
  const path = statePath(root);
  if (!confined(root, path)) return { schemaVersion: 1, ok: false, action: "uninstall_failed", root, restored: [], error: "state_invalid", idempotent: false };
  if (!existsSync(path)) return { schemaVersion: 1, ok: true, action: "uninstalled", root, restored: [], idempotent: true };
  let state;
  try {
    state = validateInstallState(root, loadState(root));
    for (const [target, original] of Object.entries(state.files)) if (original.installed && (!existsSync(target) || createHash("sha256").update(readFileSync(target)).digest("hex") !== original.installed)) throw new Error("state_conflict");
    const restored = Object.keys(state.files);
    restore(root, state);
    if (state.watch?.added) unenrollWatch(root, watchConfigPath);
    rmSync(path, { force: true });
    removeInstallStateKey(root);
    return { schemaVersion: 1, ok: true, action: "uninstalled", root, restored, idempotent: false };
  } catch (error) {
    return { schemaVersion: 1, ok: false, action: "uninstall_failed", root, restored: [], error: String(error.message ?? error), idempotent: false };
  }
}
