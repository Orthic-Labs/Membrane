import { access, readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { defaultTokenPath } from "./project-registry.mjs";

const RUNTIME_FILE = join("tools", "lib", "memory", "runtime.json");
const DEFAULT_DB = join("tools", ".cache", "memory", "cortex-engine.db");
const DEFAULT_TOKEN = join("tools", ".cache", "memory", "api-token");
const DEFAULT_HOST = "127.0.0.1";
const DEFAULT_PORT = 47851;

async function exists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

async function workspaceRootFrom(start) {
  let current = start;
  while (true) {
    if (await exists(join(current, RUNTIME_FILE))) return current;
    const parent = dirname(current);
    if (parent === current) break;
    current = parent;
  }
  throw new Error("installation binding unavailable: tools/lib/memory/runtime.json not found");
}

async function readRuntime(path) {
  const raw = JSON.parse(await readFile(path, "utf8"));
  const port = Number(raw.port);
  if (!Number.isInteger(port) || port < 1024 || port > 65535) {
    throw new Error("installation binding unavailable: runtime port is invalid");
  }
  if (raw.serviceId && raw.serviceId !== "cortex-local-v1") {
    throw new Error("installation binding unavailable: runtime serviceId mismatch");
  }
  if (raw.host && raw.host !== DEFAULT_HOST) {
    throw new Error("installation binding unavailable: runtime host must be loopback");
  }
  return { host: raw.host || DEFAULT_HOST, port };
}

function envPort() {
  const raw = process.env.CORTEX_PORT?.trim();
  if (!raw) return undefined;
  const port = Number(raw);
  return Number.isInteger(port) && port >= 1024 && port <= 65535 ? port : undefined;
}

function envDb(workspaceRoot) {
  const raw = process.env.CORTEX_DB?.trim();
  return raw || join(workspaceRoot, DEFAULT_DB);
}

function tokenPathFor(binding, workspaceRoot, registryPath) {
  const envFile = process.env.CORTEX_API_TOKEN_FILE?.trim();
  if (envFile) return envFile;
  if (binding.token_grant?.path) return binding.token_grant.path;
  return defaultTokenPath(binding.root, registryPath) || join(workspaceRoot, DEFAULT_TOKEN);
}

function finishBinding(binding, workspaceRoot, runtime, registryPath) {
  const port = envPort() ?? runtime.port;
  const db = envDb(workspaceRoot);
  const tokenPath = tokenPathFor(binding, workspaceRoot, registryPath);
  return {
    workspaceRoot,
    host: runtime.host,
    port,
    endpoint: `http://${runtime.host}:${port}`,
    db,
    tokenPath,
    tokenGeneration: binding.token_grant?.generation ?? null,
    repositoryCatalogDigest: binding.repository_catalog_digest ?? null,
  };
}

/** One resolved installation binding: token + loopback endpoint + canonical DB. */
export async function installationBindingFor(binding, { registryPath } = {}) {
  let workspaceRoot;
  try {
    workspaceRoot = await workspaceRootFrom(binding.root);
  } catch {
    // tools/lib/memory/runtime.json was not found anywhere above binding.root: the workspace
    // runtime manifest is not installed yet (tests, dry-run enrollments). This is the ONLY
    // legitimate silent-fallback case -- fall back to env overrides and canonical relative
    // paths under the caller-supplied root.
    return finishBinding(binding, binding.root, { host: DEFAULT_HOST, port: DEFAULT_PORT }, registryPath);
  }
  // workspaceRootFrom only ever returns a root where RUNTIME_FILE is confirmed present, so a
  // failure here means the manifest exists but failed validation (invalid port, serviceId
  // mismatch, non-loopback host) -- a real stale/corrupt binding. Let it throw loudly instead
  // of silently masking it behind the default endpoint.
  const runtime = await readRuntime(join(workspaceRoot, RUNTIME_FILE));
  return finishBinding(binding, workspaceRoot, runtime, registryPath);
}

export function installationEnv(bindingRecord) {
  return {
    WORKSPACE_ROOT: bindingRecord.workspaceRoot,
    CORTEX_PORT: String(bindingRecord.port),
    CORTEX_DB: bindingRecord.db,
    CORTEX_API_TOKEN_FILE: bindingRecord.tokenPath,
    MEMBRANE_REPOSITORY_CATALOG_DIGEST: bindingRecord.repositoryCatalogDigest || "",
  };
}
