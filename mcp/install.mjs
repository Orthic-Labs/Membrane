#!/usr/bin/env node
// Native client enrollment. Client configuration is changed only through native MCP CLIs.
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, rename, writeFile, readFile, stat } from "node:fs/promises";
import { dirname, join } from "node:path";
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
// -----------------------------------------------------------------------------
// MBR-203 — Transactional install contract (additive).
//
// Mirrors `engine/crates/membrane/src/install_tx.rs` on the JS side. The Rust
// binary and the JS enrollment CLI both consume the same `InstallPlan` /
// `InstallReceiptV1` JSON shape so an operator can hand-edit one plan and
// hand it to either side. The contract is identical:
//
//   1. Install applies under a scratch MEMBRANE_ROOT.
//   2. Stages run in order; each stage has an action and a rollback.
//   3. The receipt is rewritten on disk after every stage.
//   4. A stage failure runs every prior rollback in reverse and marks the
//      receipt `rolled_back`.
//   5. `commit` is the only operation that touches the target root: it
//      atomically renames the scratch root to the target root.
//
// This block stays additive: no existing export above is renamed, removed,
// or re-exported; later install surfaces append their own sections.
// -----------------------------------------------------------------------------

export const INSTALL_RECEIPT_SCHEMA_VERSION = 1;
export const INSTALL_RECEIPT_FILE_NAME = "install-receipt.json";

export const INSTALL_STAGES = Object.freeze({
  Enumerate: "Enumerate",
  WriteManifest: "WriteManifest",
  MintLease: "MintLease",
  PublishReceipt: "PublishReceipt",
  RegisterBindings: "RegisterBindings",
});

/**
 * Thrown by `executePlan` when a stage fails. The rolled-back receipt is
 * carried on the error so the caller can surface it without re-reading the
 * scratch root.
 */
export class InstallRolledBackError extends Error {
  constructor(reason, receipt) {
    super(`install rolled back: ${reason}`);
    this.name = "InstallRolledBackError";
    this.reason = reason;
    this.receipt = receipt;
  }
}

function isInstallStage(value) {
  return typeof value === "string" && Object.values(INSTALL_STAGES).includes(value);
}

function runInstallCommand(command) {
  if (typeof command !== "string" || command.trim().length === 0) return;
  try {
    if (process.platform === "win32") {
      execFileSync("cmd", ["/C", command], { stdio: ["ignore", "ignore", "pipe"] });
    } else {
      execFileSync("sh", ["-c", command], { stdio: ["ignore", "ignore", "pipe"] });
    }
  } catch (error) {
    const stderr = error.stderr ? error.stderr.toString() : "";
    const stdout = error.stdout ? error.stdout.toString() : "";
    throw new Error(
      `exit=${error.status ?? "?"} stderr=${stderr.trim()} stdout=${stdout.trim()}`,
    );
  }
}

function computeInstallCommitDigest(plan) {
  const payload = JSON.stringify({
    plan_id: plan.plan_id,
    scratch_root: plan.scratch_root,
    steps: plan.steps,
  });
  return `sha256:${createHash("sha256").update(payload).digest("hex")}`;
}

async function persistInstallReceipt(receipt, path) {
  const body = JSON.stringify(receipt, null, 2);
  const tmp = `${path}.tmp`;
  await writeFile(tmp, body, "utf8");
  await rename(tmp, path);
}

/**
 * Drive `plan` stage by stage against `scratchRoot`. Persists a typed
 * receipt after every stage; on any stage failure, runs every previously
 * completed stage's `rollback` in reverse order, marks the receipt
 * `rolled_back`, and throws `InstallRolledBackError` with the receipt
 * attached.
 *
 * A second call against the same scratch root with the same `plan_id` and
 * a previously committed receipt is a noop: the existing receipt is
 * returned verbatim and no stage runs.
 *
 * @param {{plan_id: string, scratch_root: string, steps: Array<{stage: string, action: string, rollback: string}>}} plan
 * @param {string} scratchRoot
 * @param {number} startedAtUnixMs
 * @returns {Promise<object>} the receipt
 */
export async function executePlan(plan, scratchRoot, startedAtUnixMs) {
  if (!plan || typeof plan !== "object") {
    throw new Error("install plan is required");
  }
  if (typeof plan.plan_id !== "string" || plan.plan_id.trim().length === 0) {
    throw new Error("install plan.plan_id is required");
  }
  if (plan.scratch_root !== scratchRoot) {
    throw new Error(
      `install plan scratch_root ${plan.scratch_root} does not match provided scratchRoot ${scratchRoot}`,
    );
  }
  if (!Array.isArray(plan.steps)) {
    throw new Error("install plan.steps must be an array");
  }
  for (const step of plan.steps) {
    if (!isInstallStage(step.stage)) {
      throw new Error(`unknown install stage: ${step.stage}`);
    }
    if (typeof step.action !== "string") {
      throw new Error("install step action must be a string");
    }
    if (typeof step.rollback !== "string") {
      throw new Error("install step rollback must be a string");
    }
  }

  await mkdir(scratchRoot, { recursive: true });
  const receiptPath = join(scratchRoot, INSTALL_RECEIPT_FILE_NAME);

  // Idempotency: a committed receipt with the same plan_id is reused verbatim.
  try {
    const existingText = await readFile(receiptPath, "utf8");
    const existing = JSON.parse(existingText);
    if (
      existing
      && existing.schema_version === INSTALL_RECEIPT_SCHEMA_VERSION
      && existing.plan_id === plan.plan_id
      && existing.outcome === "committed"
    ) {
      return existing;
    }
  } catch {
    // No existing receipt — fall through.
  }

  const receipt = {
    schema_version: INSTALL_RECEIPT_SCHEMA_VERSION,
    plan_id: plan.plan_id,
    commit_digest: computeInstallCommitDigest(plan),
    started_at_unix_ms: startedAtUnixMs,
    finished_at_unix_ms: null,
    stages_completed: [],
    outcome: "pending",
    rollback_actions: [],
  };
  await persistInstallReceipt(receipt, receiptPath);

  for (let index = 0; index < plan.steps.length; index++) {
    const step = plan.steps[index];
    try {
      runInstallCommand(step.action);
    } catch (error) {
      const rollbackActions = [];
      for (let prior = index - 1; prior >= 0; prior--) {
        try {
          runInstallCommand(plan.steps[prior].rollback);
        } catch {
          // Best-effort: record the chain regardless of per-rollback failure.
        }
        rollbackActions.push(plan.steps[prior].rollback);
      }
      receipt.stages_completed = plan.steps.slice(0, index).map((s) => s.stage);
      receipt.rollback_actions = rollbackActions;
      receipt.outcome = { rolled_back: { reason: error.message } };
      receipt.finished_at_unix_ms = startedAtUnixMs;
      await persistInstallReceipt(receipt, receiptPath);
      throw new InstallRolledBackError(error.message, receipt);
    }
    receipt.stages_completed.push(step.stage);
    await persistInstallReceipt(receipt, receiptPath);
  }

  receipt.finished_at_unix_ms = startedAtUnixMs;
  await persistInstallReceipt(receipt, receiptPath);
  return receipt;
}

/**
 * Atomic rename from scratch to target. The only operation that touches the
 * target root. Sets the receipt outcome to `committed` and rewrites the
 * receipt under the target root so the live install carries the audit trail.
 *
 * @param {object} receipt
 * @param {string} scratchRoot
 * @param {string} targetRoot
 * @param {number} [nowUnixMs]
 */
export async function commit(receipt, scratchRoot, targetRoot, nowUnixMs) {
  if (!receipt || receipt.outcome !== "pending") {
    throw new Error(
      `cannot commit receipt with outcome ${receipt?.outcome ?? "?"} — only Pending receipts are committable`,
    );
  }
  let scratchStat;
  try {
    scratchStat = await stat(scratchRoot);
  } catch {
    throw new Error(`scratch root ${scratchRoot} does not exist`);
  }
  if (!scratchStat.isDirectory()) {
    throw new Error(`scratch root ${scratchRoot} is not a directory`);
  }
  try {
    await stat(targetRoot);
    throw new Error(
      `target root ${targetRoot} already exists; refusing to clobber an existing install`,
    );
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  const parent = dirname(targetRoot);
  if (parent && parent !== "." && parent !== "/") {
    await mkdir(parent, { recursive: true });
  }
  await rename(scratchRoot, targetRoot);
  receipt.outcome = "committed";
  receipt.finished_at_unix_ms = nowUnixMs ?? Date.now();
  const receiptPath = join(targetRoot, INSTALL_RECEIPT_FILE_NAME);
  await persistInstallReceipt(receipt, receiptPath);
}

// -----------------------------------------------------------------------------
// MBR-205 — Ownership-safe uninstall (additive).
//
// Mirrors `engine/crates/membrane/src/uninstall.rs` on the JS side. The JS
// enrollment CLI uses these helpers to refuse the same candidates the Rust
// binary would refuse, so the operator gets the same refuse-by-default
// contract from either entry point. The contract is:
//
//   1. The ownership table lives at `<receiptRoot>/ownership.json`.
//   2. A missing table is treated as empty — `authorizeUninstall` returns
//      an empty authorised set and the caller refuses everything.
//   3. `registerOwnershipClaim` rejects duplicate `(kind, path)` pairs
//      with `DuplicateOwnershipError` so the audit trail cannot be
//      silently overwritten.
//
// This block stays additive: no existing export above is renamed, removed,
// or re-exported. The integrator merges the MBR-205 additions with the
// MBR-203 and MBR-206 blocks by concatenating the three sections.
// -----------------------------------------------------------------------------

export const UNINSTALL_RECEIPT_SCHEMA_VERSION = 1;
export const OWNERSHIP_TABLE_FILE_NAME = "ownership.json";
export const UNINSTALL_RECEIPT_FILE_NAME = "uninstall-receipt.json";

export const OWNERSHIP_KINDS = Object.freeze({
  Manifest: "Manifest",
  Lease: "Lease",
  Receipt: "Receipt",
  Binding: "Binding",
});

/**
 * Thrown by `registerOwnershipClaim` when the caller asks to record a
 * `(kind, path)` pair that is already in the table. The audit trail must
 * never be silently overwritten, so the duplicate is surfaced as a typed
 * error instead of being merged.
 */
export class DuplicateOwnershipError extends Error {
  constructor(kind, path) {
    super(`duplicate ownership claim for kind ${kind} at path ${path}`);
    this.name = "DuplicateOwnershipError";
    this.kind = kind;
    this.path = path;
  }
}

function isOwnershipKind(value) {
  return typeof value === "string" && Object.values(OWNERSHIP_KINDS).includes(value);
}

/**
 * Build a fresh ownership table tied to a stable `installation_id`. The id
 * is `sha256:<hex>` of the receipt root so two installs at different paths
 * cannot collide.
 *
 * @param {string} receiptRoot
 * @returns {{installationId: string, claims: Array<object>}}
 */
export function freshOwnershipTable(receiptRoot) {
  const digest = createHash("sha256").update(String(receiptRoot)).digest("hex");
  return { installationId: `sha256:${digest}`, claims: [] };
}

/**
 * Load the ownership table from `<receiptRoot>/ownership.json`. When the
 * file is missing a fresh table is returned; a corrupt file surfaces as
 * `ParseError` so the caller can decide whether to fail loudly or fall
 * through. The shape is `{ installationId, claims: [{ kind, path,
 * receiptId, registeredAtUnixMs }] }`.
 *
 * @param {string} receiptRoot
 * @returns {Promise<{installationId: string, claims: Array<object>}>}
 */
export async function loadOwnershipTable(receiptRoot) {
  const tablePath = join(receiptRoot, OWNERSHIP_TABLE_FILE_NAME);
  try {
    const text = await readFile(tablePath, "utf8");
    const parsed = JSON.parse(text);
    if (!parsed || typeof parsed !== "object" || !Array.isArray(parsed.claims)) {
      throw new Error("ownership table is malformed: missing claims array");
    }
    return parsed;
  } catch (error) {
    if (error.code === "ENOENT") return freshOwnershipTable(receiptRoot);
    throw error;
  }
}

/**
 * Register one ownership claim. Rejects duplicate `(kind, path)` pairs
 * with `DuplicateOwnershipError`. The claim shape is `{ kind, path,
 * receiptId, registeredAtUnixMs }`. Same path with a different kind is
 * permitted; same kind with a different path is permitted.
 *
 * @param {{installationId: string, claims: Array<object>}} table
 * @param {{kind: string, path: string, receiptId: string, registeredAtUnixMs: number}} claim
 * @returns {void}
 */
export function registerOwnershipClaim(table, claim) {
  if (!table || !Array.isArray(table.claims)) {
    throw new Error("ownership table is required");
  }
  if (!isOwnershipKind(claim.kind)) {
    throw new Error(`unknown ownership kind: ${claim.kind}`);
  }
  if (typeof claim.path !== "string" || claim.path.length === 0) {
    throw new Error("ownership claim path must be a non-empty string");
  }
  for (const existing of table.claims) {
    if (existing.kind === claim.kind && existing.path === claim.path) {
      throw new DuplicateOwnershipError(claim.kind, claim.path);
    }
  }
  table.claims.push(claim);
}

/**
 * Return the subset of `candidates` that ARE in the ownership table.
 * Anything not in the table is left alone — the caller is expected to log
 * a warning so the operator sees which paths were refused. The returned
 * vector preserves the input order so the receipt's `removed` list
 * matches what the operator typed on the command line.
 *
 * @param {{installationId: string, claims: Array<object>}} table
 * @param {string[]} candidates
 * @returns {string[]}
 */
export function authorizeUninstall(table, candidates) {
  const claims = (table && Array.isArray(table.claims)) ? table.claims : [];
  const authorised = [];
  for (const candidate of candidates) {
    if (claims.some((claim) => claim.path === candidate)) authorised.push(candidate);
  }
  return authorised;
}

/**
 * Persist the ownership table to `<receiptRoot>/ownership.json`. Atomic
 * write (tmp + rename) so a half-written table can never be parsed.
 *
 * @param {string} receiptRoot
 * @param {{installationId: string, claims: Array<object>}} table
 * @returns {Promise<void>}
 */
export async function persistOwnershipTable(receiptRoot, table) {
  const path = join(receiptRoot, OWNERSHIP_TABLE_FILE_NAME);
  const body = JSON.stringify(table, null, 2);
  const tmp = `${path}.tmp`;
  await writeFile(tmp, body, "utf8");
  await rename(tmp, path);
}

// -----------------------------------------------------------------------------
// MBR-206 — Client registry + support matrix surface (additive).
//
// These helpers are the install path's read-only view of the MBR-206 client
// registry. They consume artifacts generated by
// scripts/tools/productization/generate-client-matrix.mjs and are intentionally
// additive: no existing export above is renamed, removed, or re-exported.
//
// Rationale for keeping this surface inside mcp/install.mjs: the install
// command already reads the binding and decides which clients to enroll; the
// matrix answers 'is this client eligible for clean install on this host?'
// without re-deriving the YAML registry on every command. Transactional
// install and ownership-safe uninstall remain separate additive sections.
// -----------------------------------------------------------------------------

import { fileURLToPath as _mbr206FileURLToPath } from "node:url";
import { dirname as _mbr206Dirname, join as _mbr206Join } from "node:path";

const _MBR206_HERE = _mbr206Dirname(_mbr206FileURLToPath(import.meta.url));
const _MBR206_REPO_ROOT = _mbr206Join(_MBR206_HERE, "..");
export const _MBR206_CLIENT_CAPABILITIES_PATH = _mbr206Join(_MBR206_REPO_ROOT, "schemas", "registry", "clients.capabilities.v1.json");
export const _MBR206_SUPPORT_MATRIX_PATH = _mbr206Join(_MBR206_REPO_ROOT, "docs", "clients", "support-matrix.v1.json");
export const _MBR206_CLIENT_REGISTRY_PATH = _mbr206Join(_MBR206_REPO_ROOT, "schemas", "registry", "clients.yaml");
export const _MBR206_OPERATIONS_INDEX_PATH = _mbr206Join(_MBR206_REPO_ROOT, "schemas", "registry", "operations", "operations-index.v1.golden.json");

/**
 * Load the generated client capabilities artifact (one envelope per declared
 * client). Pure file I/O — no install side effects.
 */
export async function loadClientCapabilities({ path = _MBR206_CLIENT_CAPABILITIES_PATH } = {}) {
  const text = await readFile(path, "utf8");
  return JSON.parse(text);
}

/**
 * Load the generated client x operation support matrix. Pure file I/O.
 */
export async function loadSupportMatrix({ path = _MBR206_SUPPORT_MATRIX_PATH } = {}) {
  const text = await readFile(path, "utf8");
  return JSON.parse(text);
}

/**
 * Return the list of client ids the install path is allowed to attempt to
 * enroll on a clean install. Today this is the union of every client the
 * generated capabilities artifact declares; the matrix's `unsupported` cells
 * are intentionally not surfaced here because the install path does not
 * require end-to-end operation support — it only needs to know which native
 * MCP CLIs are present and addressable.
 *
 * The returned ids are stable, lowercase, and in the order they appear in
 * `schemas/registry/clients.yaml`.
 */
export async function clientsForEnrollment({ capabilitiesPath = _MBR206_CLIENT_CAPABILITIES_PATH } = {}) {
  const caps = await loadClientCapabilities({ path: capabilitiesPath });
  return caps.clients.map((client) => client.id);
}

/**
 * For a single client id, return the subset of operations whose matrix cell
 * is `supported`. Returns an empty array when the matrix has no entry for
 * the client, when the operation name is unknown to the matrix, or when the
 * cell is `degraded` / `unsupported`. Pure function over the matrix.
 */
export async function supportedOperationsFor(clientId, { matrixPath = _MBR206_SUPPORT_MATRIX_PATH } = {}) {
  const matrix = await loadSupportMatrix({ path: matrixPath });
  const names = [];
  for (const cell of matrix.cells) {
    if (cell.client === clientId && cell.support === "supported") names.push(cell.operation);
  }
  return names;
}
