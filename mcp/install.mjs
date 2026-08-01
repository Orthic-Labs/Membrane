#!/usr/bin/env node
// Native client enrollment. Client configuration is changed only through native MCP CLIs.
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { bindingFor, canonicalRoot, defaultRegistryPath, enroll, installationFor, removeBinding, rotateToken } from "./project-registry.mjs";
import { installationBindingFor } from "./installation-binding.mjs";
import { enrollRepositoryCatalog } from "./repository-catalog.mjs";

const SERVER = fileURLToPath(new URL("./server.mjs", import.meta.url));
const SELF = fileURLToPath(import.meta.url);
const SERVER_NAME = "membrane";
const CLIENTS = new Set(["codex", "claude"]);
function usage() { return "usage: membrane init <root> --repository <id> --scope <id> [--virtual-id <id> --tenant-id <id> --parent <id>] [--native <name>] [--config <path>] [--dry-run] | membrane catalog <root> [--grant <repository-id>] [--registry <path>] [--dry-run] | membrane install <root> [--client codex|claude] [--claude-scope local|project|user] [--dry-run] | membrane uninstall <root> [--client codex|claude] [--dry-run] | membrane token rotate <root> [--dry-run] | membrane token recover <root> --reason leak [--dry-run]"; }
const [, , command, ...args] = process.argv;
const value = (items, flag) => { const i = items.indexOf(flag); return i >= 0 ? items[i + 1] : undefined; };
const values = (items, flag) => items.flatMap((item, index) => item === flag && items[index + 1] ? [items[index + 1]] : []);
const dryRunFor = (items) => items.includes("--dry-run");
const selectedClients = (items) => {
  const clients = values(items, "--client");
  if (clients.some((client) => !CLIENTS.has(client))) throw new Error("client must be codex or claude");
  return clients.length ? [...new Set(clients)] : [...CLIENTS];
};
const claudeScopeFor = (items) => {
  const scope = value(items, "--claude-scope") || "local";
  if (!["local", "project", "user"].includes(scope)) throw new Error("--claude-scope must be local, project, or user");
  return scope;
};

function spawnRunner(command, args, options = {}) {
  return new Promise((resolve) => {
    let child;
    const cmdShim = process.platform === "win32" && /\.cmd$/i.test(command);
    try { child = spawn(command, args, { cwd: options.cwd, windowsHide: true, shell: cmdShim, stdio: ["ignore", "pipe", "pipe"] }); }
    catch (error) { resolve({ code: 127, stdout: "", stderr: error.message, error }); return; }
    let stdout = "", stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", (error) => resolve({ code: 127, stdout, stderr: error.message, error }));
    child.on("close", (code) => resolve({ code: code ?? 1, stdout, stderr }));
  });
}

function commandFor(client, action, { nodePath, serverPath, claudeScope, prior } = {}) {
  const binary = client === "codex" ? (process.env.MEMBRANE_CODEX_BIN || "codex") : (process.env.MEMBRANE_CLAUDE_BIN || "claude");
  if (action === "detect") return { binary, args: ["--version"] };
  if (action === "get") return { binary, args: ["mcp", "get", SERVER_NAME] };
  if (action === "remove") return { binary, args: ["mcp", "remove", SERVER_NAME] };
  const executable = prior?.command || nodePath;
  const serverArgs = prior?.args || [serverPath || SERVER];
  if (client === "codex") return { binary, args: ["mcp", "add", SERVER_NAME, "--", executable, ...serverArgs] };
  return { binary, args: ["mcp", "add", "--scope", claudeScope, SERVER_NAME, "--", executable, ...serverArgs] };
}

function redacted(value) {
  if (Array.isArray(value)) return value.map(redacted);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => (
      /token|secret|authorization|password|key/i.test(key) ? [key, "[redacted]"] : [key, redacted(item)]
    )));
  }
  return value;
}

function priorConfig(stdout) {
  try {
    const parsed = JSON.parse(stdout);
    const config = parsed.server || parsed;
    const command = config.command || config.commandOrUrl;
    const args = config.args || config.arguments || [];
    if (typeof command !== "string" || !Array.isArray(args) || [command, ...args].some((arg) => typeof arg !== "string" || /[\r\n&|<>^]/.test(arg))) return null;
    if (config.env || config.headers) return null;
    return { command, args };
  } catch { return null; }
}

function isExpected(stdout, nodePath, serverPath = SERVER) {
  const parsed = priorConfig(stdout);
  if (parsed) return parsed.command === nodePath && parsed.args.length === 1 && parsed.args[0] === serverPath;
  const normalized = stdout.replaceAll("\\\\", "\\");
  return normalized.includes(nodePath) && normalized.includes(serverPath);
}

export function createNativeInstaller({ runner = spawnRunner, nodePath = process.execPath, serverPath = SERVER } = {}) {
  const execute = async (client, action, root, options = {}) => {
    const command = commandFor(client, action, { nodePath, serverPath, claudeScope: options.claudeScope || "local", prior: options.prior });
    const result = await runner(command.binary, command.args, { cwd: root, windowsHide: true });
    return { command: [command.binary, ...command.args], result };
  };
  const inspect = async (client, root) => {
    const detected = await execute(client, "detect", root);
    if (detected.result.code !== 0) return { client, status: "not_installed", detected };
    const current = await execute(client, "get", root);
    if (current.result.code !== 0) return { client, status: "absent", detected, current };
    if (isExpected(current.result.stdout, nodePath, serverPath)) return { client, status: "already_correct", detected, current };
    const prior = priorConfig(current.result.stdout);
    if (!prior) throw new Error(`${client} has a conflicting ${SERVER_NAME} entry that cannot be safely restored`);
    return { client, status: "conflict", detected, current, prior };
  };
  const install = async (root, clients, { dryRun = false, claudeScope = "local" } = {}) => {
    const inspections = await Promise.all(clients.map((client) => inspect(client, root)));
    const unavailable = inspections.filter((entry) => entry.status === "not_installed").map((entry) => entry.client);
    if (unavailable.length) {
      const detail = inspections.filter((entry) => entry.status === "not_installed").map((entry) => `${entry.client}: ${entry.detected.result.stderr}`).join("; ");
      throw new Error(`native MCP client not installed: ${unavailable.join(", ")} (${detail})`);
    }
    const planned = inspections.flatMap((entry) => entry.status === "already_correct" ? [] : [entry.status === "conflict" ? commandFor(entry.client, "remove", { claudeScope, nodePath, serverPath }) : null, commandFor(entry.client, "add", { claudeScope, nodePath, serverPath })].filter(Boolean));
    if (dryRun) return { clients: inspections.map((entry) => ({ client: entry.client, before: entry.status })), commands: planned.map((entry) => [entry.binary, ...entry.args]), files: { server: serverPath, client_config: "native-cli-managed" }, dry_run: true };
    const completed = [];
    let active;
    try {
      for (const entry of inspections) {
        if (entry.status === "already_correct") { completed.push({ ...entry, changed: false }); continue; }
        if (entry.status === "conflict") {
          const removed = await execute(entry.client, "remove", root, { claudeScope });
          if (removed.result.code !== 0) throw new Error(`${entry.client} conflicting entry removal failed: ${removed.result.stderr}`);
          active = entry;
        }
        const added = await execute(entry.client, "add", root, { claudeScope });
        if (added.result.code !== 0) throw new Error(`${entry.client} add failed: ${added.result.stderr}`);
        const verified = await execute(entry.client, "get", root);
        if (verified.result.code !== 0 || !isExpected(verified.result.stdout, nodePath, serverPath)) throw new Error(`${entry.client} add verification failed`);
        completed.push({ ...entry, changed: true, after: "installed" });
        active = undefined;
      }
    } catch (error) {
      if (active?.status === "conflict") await execute(active.client, "add", root, { claudeScope, prior: active.prior });
      for (const entry of completed.reverse()) {
        if (!entry.changed) continue;
        await execute(entry.client, "remove", root, { claudeScope });
        if (entry.status === "conflict") await execute(entry.client, "add", root, { claudeScope, prior: entry.prior });
      }
      throw error;
    }
    return {
      clients: completed.map((entry) => ({ client: entry.client, before: entry.status, after: entry.after || "unchanged", owned: true, ...(entry.prior ? { prior: redacted(entry.prior) } : {}) })),
      dry_run: false,
    };
  };
  const uninstall = async (root, receipts, { dryRun = false, claudeScope = "local" } = {}) => {
    const planned = [];
    for (const receipt of receipts) {
      const current = await execute(receipt.client, "get", root);
      if (current.result.code !== 0 || !isExpected(current.result.stdout, nodePath, serverPath)) throw new Error(`${receipt.client} ${SERVER_NAME} entry is not receipt-owned`);
      planned.push(commandFor(receipt.client, "remove", { claudeScope, nodePath, serverPath }));
      if (receipt.prior) planned.push(commandFor(receipt.client, "add", { claudeScope, nodePath, serverPath, prior: receipt.prior }));
    }
    if (dryRun) return { clients: receipts.map((receipt) => receipt.client), commands: planned.map((entry) => [entry.binary, ...entry.args]), dry_run: true };
    for (const receipt of receipts) {
      const removed = await execute(receipt.client, "remove", root, { claudeScope });
      if (removed.result.code !== 0) throw new Error(`${receipt.client} removal failed: ${removed.result.stderr}`);
      if (receipt.prior) {
        const restored = await execute(receipt.client, "add", root, { claudeScope, prior: receipt.prior });
        if (restored.result.code !== 0) throw new Error(`${receipt.client} prior-entry restore failed: ${restored.result.stderr}`);
      }
    }
    return { clients: receipts.map((receipt) => receipt.client), dry_run: false };
  };
  return { install, uninstall };
}

function descriptorFrom(rest, scope_id) {
  const id = value(rest, "--virtual-id");
  if (!id) return undefined;
  const tenant_id = value(rest, "--tenant-id");
  if (!tenant_id) throw new Error("--tenant-id is required with --virtual-id");
  return { kind: "virtual", id, tenant_id, parents: values(rest, "--parent"), inherit_global: false };
}

async function init(root, rest) {
  const repository_id = value(rest, "--repository");
  const scope_id = value(rest, "--scope");
  if (!root || !repository_id || !scope_id) throw new Error(usage());
  const canonical = await canonicalRoot(root);
  const config = value(rest, "--config");
  const native = value(rest, "--native");
  const provider_config = { transport: "loopback", ...(config ? { config: { path: config } } : {}), ...(native ? { native: { name: native } } : {}) };
  const receipt = { action: "enroll", root: canonical, repository_id, scope_id, registry: defaultRegistryPath(), dry_run: dryRunFor(rest) };
  if (config || native) receipt.installation = installationFor({ provider_config });
  if (!receipt.dry_run) {
    const installation = await installationBindingFor({
      root: canonical,
      repository_id,
      scope_id,
      scope_descriptor: descriptorFrom(rest, scope_id),
      provider_config,
    }, { registryPath: defaultRegistryPath() });
    Object.assign(receipt, await enroll(canonical, {
      repository_id,
      scope_id,
      scope_descriptor: descriptorFrom(rest, scope_id),
      provider_config: { ...provider_config, installation_binding: installation },
      grant_policy: { level: "read-only" },
    }));
    receipt.installation_binding = installation;
  }
  return receipt;
}

async function install(root, rest) {
  if (!root) throw new Error(usage());
  const binding = await bindingFor(root);
  const native = createNativeInstaller();
  const result = await native.install(binding.root, selectedClients(rest), { dryRun: dryRunFor(rest), claudeScope: claudeScopeFor(rest) });
  const receipt = { action: "install", root: binding.root, repository_id: binding.repository_id, registry: defaultRegistryPath(), ...result };
  if (!receipt.dry_run) {
    const installations = Object.fromEntries(result.clients.map((client) => [client.client, client]));
    const installation = await installationBindingFor(binding, { registryPath: defaultRegistryPath() });
    await enroll(binding.root, {
      repository_id: binding.repository_id,
      scope_id: binding.scope_id,
      scope_descriptor: binding.scope_descriptor,
      provider_config: { ...(binding.provider_config || {}), installations, installation_binding: installation },
      grant_policy: binding.grant_policy || { level: "read-only" },
    });
    receipt.installation_binding = installation;
  }
  return receipt;
}

async function catalog(root, rest) {
  if (!root) throw new Error(usage());
  const registryPath = value(rest, "--registry");
  return enrollRepositoryCatalog(root, {
    ...(registryPath ? { registryPath } : {}),
    childGrants: values(rest, "--grant"),
    dryRun: dryRunFor(rest),
  });
}

async function uninstall(root, rest) {
  if (!root) throw new Error(usage());
  const binding = await bindingFor(root);
  const clients = selectedClients(rest);
  const receipts = clients.map((client) => binding.provider_config?.installations?.[client]).filter(Boolean);
  const revoked_token_generations = [...new Set([...(binding.token_grant?.revoked_generations || []), ...(binding.token_grant ? [binding.token_grant.generation] : [])])].sort((a, b) => a - b);
  const receipt = { action: "uninstall", root: binding.root, repository_id: binding.repository_id, registry: defaultRegistryPath(), revoked_token_generations, dry_run: dryRunFor(rest) };
  if (receipts.length) receipt.native = await createNativeInstaller().uninstall(binding.root, receipts, { dryRun: receipt.dry_run, claudeScope: claudeScopeFor(rest) });
  if (!receipt.dry_run) Object.assign(receipt, await removeBinding(root));
  return receipt;
}

async function token(subcommand, root, rest) {
  if (!root || !["rotate", "recover"].includes(subcommand)) throw new Error(usage());
  const binding = await bindingFor(root);
  const reason = subcommand === "recover" ? "leak_recovery" : "rotation";
  if (subcommand === "recover" && value(rest, "--reason") !== "leak") throw new Error("token recovery requires --reason leak");
  if (dryRunFor(rest)) return { action: "token_rotate", root: binding.root, repository_id: binding.repository_id, registry: defaultRegistryPath(), token_generation: (binding.token_grant?.generation || 0) + 1, revoked_token_generations: [...new Set([...(binding.token_grant?.revoked_generations || []), ...(binding.token_grant ? [binding.token_grant.generation] : [])])].sort((a, b) => a - b), reason, dry_run: true };
  const rotated = await rotateToken(root, { reason });
  const refreshed = await bindingFor(root);
  const installation = await installationBindingFor(refreshed, { registryPath: defaultRegistryPath() });
  await enroll(root, {
    repository_id: refreshed.repository_id,
    scope_id: refreshed.scope_id,
    scope_descriptor: refreshed.scope_descriptor,
    provider_config: { ...(refreshed.provider_config || {}), installation_binding: installation },
    grant_policy: refreshed.grant_policy || { level: "read-only" },
  });
  return { action: "token_rotate", registry: defaultRegistryPath(), dry_run: false, installation_binding: installation, ...rotated };
}

if (process.argv[1] === SELF) {
  if (command === "init") process.stdout.write(JSON.stringify(await init(args[0], args.slice(1))) + "\n");
  else if (command === "catalog") process.stdout.write(JSON.stringify(await catalog(args[0], args.slice(1))) + "\n");
  else if (command === "install") process.stdout.write(JSON.stringify(await install(args[0], args.slice(1))) + "\n");
  else if (command === "uninstall") process.stdout.write(JSON.stringify(await uninstall(args[0], args.slice(1))) + "\n");
  else if (command === "token") process.stdout.write(JSON.stringify(await token(args[0], args[1], args.slice(2))) + "\n");
  else throw new Error(usage());
}
