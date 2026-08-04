import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { realpath } from "node:fs/promises";
import { join, relative, resolve } from "node:path";
import { defaultRegistryPath, enroll } from "./project-registry.mjs";

const SUBMODULE_PATH = /^\s*path\s*=\s*(.+?)\s*$/gm;

function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
  return JSON.stringify(value);
}

function digest(value) {
  return `sha256:${createHash("sha256").update(canonicalJson(value)).digest("hex")}`;
}

function portableRelative(root, candidate) {
  const value = relative(root, candidate).replaceAll("\\", "/");
  return value || ".";
}

function repositoryId(relativeRoot) {
  return `repo-${createHash("sha256").update(relativeRoot).digest("hex").slice(0, 24)}`;
}

function scopeId(relativeRoot) {
  return `scope-${repositoryId(relativeRoot).slice("repo-".length)}`;
}

function submodulePaths(root) {
  const path = join(root, ".gitmodules");
  if (!existsSync(path)) return [];
  const text = readFileSync(path, "utf8");
  return [...text.matchAll(SUBMODULE_PATH)].map((match) => match[1].trim());
}

/** Discover root + registered child Git repositories without indexing child contents. */
export async function discoverRepositoryRoots(workspaceRoot) {
  const root = await realpath(resolve(workspaceRoot));
  const candidates = new Set([".", ...submodulePaths(root)]);
  for (const entry of requireDirectoryNames(root)) {
    if (entry === ".git" || entry.startsWith(".")) continue;
    const candidate = join(root, entry);
    if (existsSync(join(candidate, ".git"))) candidates.add(entry);
  }
  return [...candidates]
    .map((relativeRoot) => ({ relativeRoot: relativeRoot.replaceAll("\\", "/").replace(/^\.\//, "") || ".", absoluteRoot: resolve(root, relativeRoot) }))
    .filter(({ absoluteRoot }) => existsSync(absoluteRoot))
    .sort((left, right) => left.relativeRoot.localeCompare(right.relativeRoot));
}

function requireDirectoryNames(root) {
  try {
    return readdirSync(root, { withFileTypes: true }).filter((entry) => entry.isDirectory()).map((entry) => entry.name);
  } catch {
    return [];
  }
}

export async function buildRepositoryCatalog(workspaceRoot, options = {}) {
  const root = await realpath(resolve(workspaceRoot));
  const discovered = await discoverRepositoryRoots(root);
  const workspaceId = options.workspaceId || repositoryId(".");
  const repositories = discovered.map(({ relativeRoot }) => {
    const repository_id = repositoryId(relativeRoot);
    return {
      repository_id,
      root: relativeRoot,
      scope_id: scopeId(relativeRoot),
      role: relativeRoot === "." ? "workspace-root" : "child-repository",
      ...(relativeRoot === "." ? {} : { parent_repository_id: repositoryId(".") }),
      // Verified in cortex/scripts/blueprint.mjs and cortex/graph/static-provider.mjs: the manifest is stored inside graph.db, not as a JSON file.
      cortex_graph: `${relativeRoot === "." ? "" : `${relativeRoot}/`}.agent/graph/graph.db`,
      grants: [],
    };
  });
  const body = { schema: "orthic.repository-catalog.v1", workspace_id: workspaceId, repositories };
  return { ...body, catalog_digest: digest(body) };
}

export function hasExplicitChildGrant(catalog, callerRepositoryId, targetRepositoryId, grants = []) {
  const target = catalog?.repositories?.find((entry) => entry.repository_id === targetRepositoryId);
  if (!target || target.role !== "child-repository") return callerRepositoryId === targetRepositoryId;
  return grants.includes(targetRepositoryId) && callerRepositoryId === catalog.repositories.find((entry) => entry.role === "workspace-root")?.repository_id;
}

export function catalogDigest(catalog) {
  const { catalog_digest: _ignored, ...body } = catalog || {};
  return digest(body);
}

export async function enrollRepositoryCatalog(workspaceRoot, options = {}) {
  const root = await realpath(resolve(workspaceRoot));
  const catalog = await buildRepositoryCatalog(root, options);
  const registryPath = options.registryPath || defaultRegistryPath();
  const childGrants = [...new Set(options.childGrants || [])];
  const workspace = catalog.repositories.find((entry) => entry.role === "workspace-root");
  const plan = catalog.repositories.map((entry) => ({
    root: resolve(root, entry.root),
    repository_id: entry.repository_id,
    scope_id: entry.scope_id,
    repository_catalog_digest: catalog.catalog_digest,
    grant_policy: entry.repository_id === workspace.repository_id
      ? { level: "read-only", child_repository_ids: childGrants }
      : { level: "read-only", parent_repository_id: workspace.repository_id },
  }));
  if (options.dryRun) return { action: "catalog_enroll", catalog, registry: registryPath, bindings: plan, dry_run: true };
  const bindings = [];
  for (const binding of plan) bindings.push(await enroll(binding.root, binding, registryPath));
  return { action: "catalog_enroll", catalog, registry: registryPath, bindings, dry_run: false };
}
