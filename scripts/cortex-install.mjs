#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { removeInstallStateKey, sealInstallState, verifyInstallState } from "../lib/init/state-integrity.mjs";
import { writeProductManifest } from "../lib/init/manifest.mjs";
import { isConfinedPath, resolvePhysicalPath } from "../lib/path-confinement.mjs";

const START = "<!-- cortex:start -->";
const END = "<!-- cortex:end -->";
const MESSAGE = "Run cortex_orient first — Cortex Graph has current repository truth.";
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const SERVER_SCRIPT = join(SCRIPT_DIR, "cortex-mcp.mjs");
const CORTEX_SCRIPT = join(SCRIPT_DIR, "cortex.mjs");

function parseArgs(argv) {
  const args = { _: [] };
  for (let index = 0; index < argv.length; index += 1) {
    const value = argv[index];
    if (value === "--project") args.scope = "project";
    else if (value === "--global") args.scope = "global";
    else if (value === "--redirect" || value === "--uninstall" || value === "--redirect-check" || value === "--grant-check") args[value.slice(2)] = true;
    else if (value.startsWith("--")) {
      const [key, inline] = value.slice(2).split("=", 2);
      args[key] = inline === undefined ? argv[++index] : inline;
    }
    else args._.push(value);
  }
  return args;
}

function shellQuote(value) {
  return `'${String(value).replaceAll("'", "'\\''")}'`;
}

function statePath(root) { return join(root, ".agent", "graph", "cortex-install-state.json"); }
function loadState(root) {
  const path = statePath(root);
  if (!isConfinedPath(root, path)) throw new Error("state_invalid");
  if (!existsSync(path)) return { version: 1, files: {} };
  try { return verifyInstallState(root, JSON.parse(readFileSync(path, "utf8"))); } catch { throw new Error("state_invalid"); }
}
function saveState(root, state) {
  const path = statePath(root);
  if (!isConfinedPath(root, path)) throw new Error("state_invalid");
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(sealInstallState(root, state), null, 2)}\n`, "utf8");
}
function remember(state, path) {
  if (Object.hasOwn(state.files, path)) return;
  const bytes = existsSync(path) ? readFileSync(path) : null;
  state.files[path] = { exists: bytes !== null, content: bytes?.toString("utf8") ?? null, bytes: bytes?.toString("base64") ?? null };
}
export function validateInstallState(root, state) {
  if (!state || typeof state !== "object" || state.version !== 1 || !state.files || typeof state.files !== "object" || Array.isArray(state.files)) throw new Error("state_invalid");
  for (const [path, original] of Object.entries(state.files)) {
    if (!isConfinedPath(root, path) || !allowedTarget(root, path) || !original || typeof original !== "object" || typeof original.exists !== "boolean" || !/^[a-f0-9]{64}$/.test(original.installed ?? "")) throw new Error("state_invalid");
    if (original.exists && original.bytes !== null && original.bytes !== undefined && (typeof original.bytes !== "string" || Buffer.from(original.bytes, "base64").toString("base64") !== original.bytes)) throw new Error("state_invalid");
    if (original.exists && original.bytes == null && typeof original.content !== "string") throw new Error("state_invalid");
  }
  return state;
}
function allowedTarget(root, path) {
  const target = resolvePhysicalPath(path);
  if (!target) return false;
  const base = realpathSync(root);
  const files = ["CLAUDE.md", "AGENTS.md", "CORTEX-AGENT.md", ".mcp.json", join(".cursor", "rules", "cortex.mdc"), join(".claude", "settings.json")];
  const hooks = ["post-checkout", "post-merge", "post-rewrite", "post-checkout.cmd", "post-merge.cmd", "post-rewrite.cmd"].map((name) => join(".git", "hooks", name));
  return new Set(files.concat(hooks).map((name) => join(base, name))).has(target);
}
function recordInstalled(state, path) { if (existsSync(path) && state.files[path]) state.files[path].installed = createHash("sha256").update(readFileSync(path)).digest("hex"); }
function writeManaged(root, state, path, content) {
  if (!isConfinedPath(root, path) || !allowedTarget(root, path)) throw new Error("state_invalid");
  remember(state, path);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content, "utf8");
  recordInstalled(state, path);
}
function removeBlock(content) {
  const escapedStart = START.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const escapedEnd = END.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return content.replace(new RegExp(`\\n?${escapedStart}[\\s\\S]*?${escapedEnd}\\n?`, "g"), "\n").replace(/^\n+$/, "");
}
function mergeBlock(content) {
  const block = `${START}\n## Cortex Graph\nBefore reading repository files, call \`cortex_orient\` with current repository root. Use \`cortex_expand\` for bounded context and \`cortex_resolve\` for search with graph neighbors.\n${END}`;
  const without = removeBlock(content);
  return `${without.trimEnd()}${without.trimEnd() ? "\n\n" : ""}${block}\n`;
}
function mergeJsonFile(root, state, path, update) {
  if (!isConfinedPath(root, path) || !allowedTarget(root, path)) throw new Error("state_invalid");
  const current = existsSync(path) ? readFileSync(path, "utf8") : "{}";
  let value;
  try { value = JSON.parse(current); } catch { throw new Error(`${path} is not valid JSON`); }
  remember(state, path);
  const next = update(value);
  if (!isConfinedPath(root, path) || !allowedTarget(root, path)) throw new Error("state_invalid");
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(next, null, 2)}\n`, "utf8");
  recordInstalled(state, path);
}

function instructionPath(root, host) {
  if (host === "claude-code") return join(root, "CLAUDE.md");
  if (host === "codex") return join(root, "AGENTS.md");
  if (host === "cursor") return join(root, ".cursor", "rules", "cortex.mdc");
  return join(root, "CORTEX-AGENT.md");
}

function installMcpEntry(state, root) {
  const path = join(root, ".mcp.json");
  mergeJsonFile(root, state, path, (value) => ({
    ...value,
    mcpServers: {
      ...(value.mcpServers ?? {}),
      cortex: { command: process.execPath, args: [SERVER_SCRIPT] },
    },
  }));
}

function redirectCommand(root, grants = false) {
  return `${shellQuote(process.execPath)} ${shellQuote(fileURLToPath(import.meta.url))} --${grants ? "grant-check" : "redirect-check"} --root ${shellQuote(root)}`;
}

function installRedirect(state, root, { grants = false } = {}) {
  const path = join(root, ".claude", "settings.json");
  mergeJsonFile(root, state, path, (value) => {
    const hooks = Array.isArray(value.hooks?.PreToolUse) ? value.hooks.PreToolUse : [];
    const filtered = hooks.filter((entry) => !JSON.stringify(entry).includes("--redirect-check") && !JSON.stringify(entry).includes("--grant-check"));
    return {
      ...value,
      hooks: {
        ...(value.hooks ?? {}),
        PreToolUse: [...filtered, {
          matcher: grants ? "^(Read|Edit)$" : "^(Read|Grep|Glob)$",
          hooks: [{ type: "command", command: redirectCommand(root, grants) }],
        }],
      },
    };
  });
}

function gitHookPaths(root) {
  const gitPath = execFileSync("git", ["-C", root, "rev-parse", "--git-path", "hooks"], { encoding: "utf8" }).trim();
  const hooksDir = resolve(root, gitPath), names = ["post-checkout", "post-merge", "post-rewrite", "post-checkout.cmd", "post-merge.cmd", "post-rewrite.cmd"];
  const paths = names.map((name) => join(hooksDir, name));
  if (paths.some((path) => !isConfinedPath(root, path) || !allowedTarget(root, path))) throw new Error("external_hooks_path");
  return paths;
}
function requireJson(root, path) {
  if (!isConfinedPath(root, path) || !allowedTarget(root, path)) throw new Error("state_invalid");
  if (!existsSync(path)) return;
  try { JSON.parse(readFileSync(path, "utf8")); } catch { throw new Error(`${path} is not valid JSON`); }
}
function installGitHooks(state, root, paths = gitHookPaths(root)) {
  for (const path of paths) remember(state, path);
  execFileSync(process.execPath, [CORTEX_SCRIPT, "hooks", "install-git"], { cwd: root, stdio: "ignore" });
  for (const path of paths) recordInstalled(state, path);
}

function restore(root, state) {
  for (const [path, original] of Object.entries(state.files ?? {})) {
    if (original.exists) {
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, typeof original.bytes === "string" ? Buffer.from(original.bytes, "base64") : (original.content ?? ""), "utf8");
    } else rmSync(path, { force: true });
  }
  const markerDir = join(root, ".agent", "graph");
  if (existsSync(markerDir)) for (const name of readdirSync(markerDir)) if (name.startsWith("cortex-orient-session-") && name.endsWith(".marker")) rmSync(join(markerDir, name), { force: true });
  rmSync(statePath(root), { force: true });
}

function redirectCheck(root) {
  let request = {};
  try { request = JSON.parse(readFileSync(0, "utf8") || "{}"); } catch {}
  const session = String(request.session_id ?? request.sessionId ?? "default").replace(/[^a-zA-Z0-9._-]/g, "_");
  const marker = join(root, ".agent", "graph", `cortex-orient-session-${session}.marker`);
  const allowed = existsSync(marker);
  process.stdout.write(JSON.stringify({ hookSpecificOutput: { hookEventName: "PreToolUse", permissionDecision: allowed ? "allow" : "deny", permissionDecisionReason: allowed ? "Cortex orientation receipt present." : MESSAGE } }));
  return 0;
}

function grantCheck(root) {
  let request = {};
  try { request = JSON.parse(readFileSync(0, "utf8") || "{}"); } catch {}
  const task = String(request.task_id ?? request.taskId ?? request.session_id ?? request.sessionId ?? "default");
  const input = request.tool_input ?? request.input ?? {};
  const requestedPath = String(input.path ?? input.file_path ?? input.filePath ?? request.path ?? "");
  let allowed = false;
  if (requestedPath) {
    try {
      execFileSync(process.execPath, [CORTEX_SCRIPT, "grant", "check", "--task", task, "--path", requestedPath], { cwd: root, stdio: "ignore" });
      allowed = true;
    } catch {}
  }
  process.stdout.write(JSON.stringify({ hookSpecificOutput: { hookEventName: "PreToolUse", permissionDecision: allowed ? "allow" : "deny", permissionDecisionReason: allowed ? "Cortex task-scoped grant present." : "Run cortex_expand for this path to issue a widened Cortex grant." } }));
  return 0;
}

function install(root, host, args) {
  const hookPaths = args.scope !== "global" ? gitHookPaths(root) : [];
  const state = validateInstallState(root, loadState(root));
  const instruction = instructionPath(root, host);
  if (!isConfinedPath(root, instruction) || !allowedTarget(root, instruction)) throw new Error("state_invalid");
  if (host === "claude-code" && args.scope !== "global") requireJson(root, join(root, ".mcp.json"));
  if (args.redirect) requireJson(root, join(root, ".claude", "settings.json"));
  const current = existsSync(instruction) ? readFileSync(instruction, "utf8") : "";
  writeManaged(root, state, instruction, mergeBlock(current));
  if (host === "claude-code" && args.scope !== "global") installMcpEntry(state, root);
  if (args.redirect) installRedirect(state, root, { grants: args.redirect === "grants" });
  if (args.scope !== "global") installGitHooks(state, root, hookPaths);
  // U56: use the sole manifest producer so install and package flows cannot drift.
  try { writeProductManifest({ installRoot: root }); } catch {}
  saveState(root, state);
  const output = { action: "installed", host, scope: args.scope, root, instruction, redirect: Boolean(args.redirect) };
  if (host === "claude-code" && args.scope === "global") output.advice = `claude mcp add cortex -- ${shellQuote(process.execPath)} ${shellQuote(SERVER_SCRIPT)}`;
  return output;
}

function main(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);
  const host = args.host;
  if (args["redirect-check"]) return redirectCheck(resolve(args.root ?? process.cwd()));
  if (args["grant-check"]) return grantCheck(resolve(args.root ?? process.cwd()));
  if (!host || !["claude-code", "codex", "cursor", "generic"].includes(host)) throw new Error("--host must be claude-code, codex, cursor, or generic");
  const root = resolve(args.root ?? (args.scope === "global" ? homedir() : process.cwd()));
  if (args.uninstall) {
    try {
      const state = validateInstallState(root, loadState(root));
      for (const [path, original] of Object.entries(state.files)) if (original.installed && (!existsSync(path) || createHash("sha256").update(readFileSync(path)).digest("hex") !== original.installed)) throw new Error("state_conflict");
      restore(root, state);
      removeInstallStateKey(root);
      return { ok: true, action: "uninstalled", host, scope: args.scope ?? "project", root };
    } catch (error) { return { ok: false, action: "uninstall_rejected", reason: String(error.message ?? error), host, root }; }
  }
  return install(root, host, args);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const output = main();
    if (typeof output === "number") process.exitCode = output;
    else process.stdout.write(`${JSON.stringify(output, null, 2)}\n`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

export { main, mergeBlock, removeBlock, redirectCheck };
