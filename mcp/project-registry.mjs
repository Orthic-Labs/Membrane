import { mkdir, readFile, realpath, rename, rm, writeFile } from "node:fs/promises";
import { createHash, randomBytes } from "node:crypto";
import { dirname, join } from "node:path";

const SCHEMA_VERSION = 1;
export const defaultRegistryPath = () => process.env.MEMBRANE_PROJECT_REGISTRY || join(process.env.APPDATA || process.env.HOME || ".", "MemRight", "project-registry.json");
export const defaultTokenPath = (root, registry = defaultRegistryPath()) => join(dirname(registry), "tokens", `${createHash("sha256").update(root).digest("hex")}.token`);

export async function canonicalRoot(root) {
  if (typeof root !== "string" || !root.trim()) throw new Error("project root is required");
  return realpath(root);
}

function validateTokenGrant(grant) {
  if (!grant || typeof grant !== "object" || Array.isArray(grant)) throw new Error("token_grant must be an object");
  if (!Number.isInteger(grant.generation) || grant.generation < 1) throw new Error("token_grant generation is invalid");
  if (typeof grant.token_sha256 !== "string" || !/^[a-f0-9]{64}$/.test(grant.token_sha256)) throw new Error("token_grant hash is invalid");
  if (typeof grant.path !== "string" || !grant.path) throw new Error("token_grant path is required");
  if (!Array.isArray(grant.revoked_generations) || grant.revoked_generations.some((value) => !Number.isInteger(value) || value < 1 || value >= grant.generation)) throw new Error("token_grant revocations are invalid");
  if (typeof grant.issued_at !== "string" || !grant.issued_at) throw new Error("token_grant issued_at is required");
}

function validateTokenAudit(audit) {
  if (!Array.isArray(audit) || audit.length > 32) throw new Error("token_audit is invalid");
  for (const event of audit) {
    if (!event || typeof event !== "object" || !["rotation", "leak_recovery"].includes(event.reason)) throw new Error("token_audit event is invalid");
    if (typeof event.audit_id !== "string" || !/^[a-f0-9]{24}$/.test(event.audit_id)) throw new Error("token_audit id is invalid");
    if (!Number.isInteger(event.generation) || event.generation < 1 || !Array.isArray(event.revoked_generations)) throw new Error("token_audit generation is invalid");
    if (typeof event.at !== "string" || !event.at) throw new Error("token_audit timestamp is invalid");
  }
}

function validateBinding(binding) {
  if (!binding || typeof binding !== "object") throw new Error("binding must be an object");
  for (const key of ["repository_id", "scope_id"]) {
    if (typeof binding[key] !== "string" || !binding[key].trim()) throw new Error(`binding ${key} is required`);
  }
  if (binding.provider_config !== undefined && (typeof binding.provider_config !== "object" || Array.isArray(binding.provider_config))) throw new Error("provider_config must be an object");
  if (binding.grant_policy !== undefined && (typeof binding.grant_policy !== "object" || Array.isArray(binding.grant_policy))) throw new Error("grant_policy must be an object");
  if (binding.token_grant !== undefined) validateTokenGrant(binding.token_grant);
  if (binding.token_audit !== undefined) validateTokenAudit(binding.token_audit);
}

function validateRegistry(value) {
  if (!value || value.schema_version !== SCHEMA_VERSION || !value.bindings || typeof value.bindings !== "object" || Array.isArray(value.bindings)) throw new Error("registry is malformed");
  for (const [root, binding] of Object.entries(value.bindings)) {
    if (!root || typeof root !== "string") throw new Error("registry root is malformed");
    validateBinding(binding);
  }
  return value;
}

export async function readRegistry(path = defaultRegistryPath()) {
  try { return validateRegistry(JSON.parse(await readFile(path, "utf8"))); }
  catch (error) { if (error?.code === "ENOENT") return { schema_version: SCHEMA_VERSION, bindings: {} }; throw new Error(`registry unavailable: ${error.message}`); }
}

async function writeRegistry(path, registry) {
  await mkdir(dirname(path), { recursive: true });
  const temp = `${path}.${process.pid}.${Date.now()}.tmp`;
  await writeFile(temp, JSON.stringify(registry, null, 2) + "\n", { encoding: "utf8", mode: 0o600 });
  await rename(temp, path);
}

export function installationFor(binding) {
  const provider = binding.provider_config || {};
  const native = provider.native && typeof provider.native === "object" ? provider.native : null;
  const config = provider.config && typeof provider.config === "object" ? provider.config : null;
  return { precedence: "native_then_config", selected: native ? "native" : config ? "config" : "loopback", native: native || undefined, config: config || undefined };
}

function publicBinding(root, binding) {
  return { root, repository_id: binding.repository_id, scope_id: binding.scope_id, provider_config: binding.provider_config, grant_policy: binding.grant_policy, ...(binding.token_grant ? { token_grant: binding.token_grant } : {}), ...(binding.token_audit ? { token_audit: binding.token_audit } : {}) };
}

export async function enroll(root, binding, path = defaultRegistryPath()) {
  validateBinding(binding);
  const canonical = await canonicalRoot(root);
  const registry = await readRegistry(path);
  const prior = registry.bindings[canonical];
  registry.bindings[canonical] = {
    repository_id: binding.repository_id,
    scope_id: binding.scope_id,
    provider_config: binding.provider_config || {},
    grant_policy: binding.grant_policy || {},
    ...(prior?.token_grant ? { token_grant: prior.token_grant } : {}),
    ...(prior?.token_audit ? { token_audit: prior.token_audit } : {}),
  };
  await writeRegistry(path, registry);
  return publicBinding(canonical, registry.bindings[canonical]);
}

export async function rotateToken(root, { reason = "rotation", tokenPath } = {}, path = defaultRegistryPath()) {
  if (!["rotation", "leak_recovery"].includes(reason)) throw new Error("token rotation reason is invalid");
  const canonical = await canonicalRoot(root);
  const registry = await readRegistry(path);
  const binding = registry.bindings[canonical];
  if (!binding) throw new Error("project is not enrolled");
  const previous = binding.token_grant;
  const generation = (previous?.generation || 0) + 1;
  const revoked_generations = [...new Set([...(previous?.revoked_generations || []), ...(previous ? [previous.generation] : [])])].sort((a, b) => a - b);
  const token = randomBytes(32).toString("base64url");
  const target = tokenPath || previous?.path || defaultTokenPath(canonical, path);
  await mkdir(dirname(target), { recursive: true });
  const temp = `${target}.${process.pid}.${Date.now()}.tmp`;
  await writeFile(temp, `${token}\n`, { encoding: "utf8", mode: 0o600 });
  await rename(temp, target);
  const issued_at = new Date().toISOString();
  const audit_id = createHash("sha256").update(`${canonical}:${generation}:${reason}:${issued_at}`).digest("hex").slice(0, 24);
  binding.token_grant = { generation, token_sha256: createHash("sha256").update(token).digest("hex"), path: target, issued_at, revoked_generations };
  binding.token_audit = [...(binding.token_audit || []), { audit_id, generation, revoked_generations, reason, at: issued_at }].slice(-32);
  await writeRegistry(path, registry);
  return { root: canonical, repository_id: binding.repository_id, token_generation: generation, revoked_token_generations: revoked_generations, reason, audit_id };
}

export async function removeBinding(root, path = defaultRegistryPath()) {
  const canonical = await canonicalRoot(root);
  const registry = await readRegistry(path);
  const binding = registry.bindings[canonical];
  if (!binding) throw new Error("project is not enrolled");
  delete registry.bindings[canonical];
  await writeRegistry(path, registry);
  if (binding.token_grant?.path) await rm(binding.token_grant.path, { force: true });
  const revoked_token_generations = [...new Set([...(binding.token_grant?.revoked_generations || []), ...(binding.token_grant ? [binding.token_grant.generation] : [])])].sort((a, b) => a - b);
  return { ...publicBinding(canonical, binding), revoked_token_generations };
}

export async function bindingFor(root, path = defaultRegistryPath()) {
  const canonical = await canonicalRoot(root);
  const binding = (await readRegistry(path)).bindings[canonical];
  if (!binding) throw new Error("project is not enrolled");
  return publicBinding(canonical, binding);
}
