import { mkdir, readFile, realpath, rename, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";

const SCHEMA_VERSION = 1;
export const defaultRegistryPath = () => process.env.MEMBRANE_PROJECT_REGISTRY || join(process.env.APPDATA || process.env.HOME || ".", "MemRight", "project-registry.json");

export async function canonicalRoot(root) {
  if (typeof root !== "string" || !root.trim()) throw new Error("project root is required");
  return realpath(root);
}

function validateBinding(binding) {
  if (!binding || typeof binding !== "object") throw new Error("binding must be an object");
  for (const key of ["repository_id", "scope_id"]) {
    if (typeof binding[key] !== "string" || !binding[key].trim()) throw new Error(`binding ${key} is required`);
  }
  if (binding.provider_config !== undefined && (typeof binding.provider_config !== "object" || Array.isArray(binding.provider_config))) throw new Error("provider_config must be an object");
  if (binding.grant_policy !== undefined && (typeof binding.grant_policy !== "object" || Array.isArray(binding.grant_policy))) throw new Error("grant_policy must be an object");
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

export async function enroll(root, binding, path = defaultRegistryPath()) {
  validateBinding(binding);
  const canonical = await canonicalRoot(root);
  const registry = await readRegistry(path);
  registry.bindings[canonical] = { repository_id: binding.repository_id, scope_id: binding.scope_id, provider_config: binding.provider_config || {}, grant_policy: binding.grant_policy || {} };
  await mkdir(dirname(path), { recursive: true });
  const temp = `${path}.${process.pid}.${Date.now()}.tmp`;
  await writeFile(temp, JSON.stringify(registry, null, 2) + "\n", { encoding: "utf8", mode: 0o600 });
  await rename(temp, path);
  return { root: canonical, ...registry.bindings[canonical] };
}

async function writeRegistry(path, registry) {
  await mkdir(dirname(path), { recursive: true });
  const temp = `${path}.${process.pid}.${Date.now()}.tmp`;
  await writeFile(temp, JSON.stringify(registry, null, 2) + "\n", { encoding: "utf8", mode: 0o600 });
  await rename(temp, path);
}

export async function removeBinding(root, path = defaultRegistryPath()) {
  const canonical = await canonicalRoot(root);
  const registry = await readRegistry(path);
  const binding = registry.bindings[canonical];
  if (!binding) throw new Error("project is not enrolled");
  delete registry.bindings[canonical];
  await writeRegistry(path, registry);
  return { root: canonical, ...binding };
}

export async function bindingFor(root, path = defaultRegistryPath()) {
  const canonical = await canonicalRoot(root);
  const binding = (await readRegistry(path)).bindings[canonical];
  if (!binding) throw new Error("project is not enrolled");
  return { root: canonical, ...binding };
}
