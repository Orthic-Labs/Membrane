import { createHash } from "node:crypto";
import { existsSync, readdirSync } from "node:fs";
import { realpath } from "node:fs/promises";
import { join, relative, resolve } from "node:path";
import { defaultRegistryPath, enroll } from "./project-registry.mjs";
import { readBlueprintReadiness } from "./blueprint-readiness.mjs";

/**
 * Known venture codes derived from observed workspace directory names plus the
 * codes documented for each venture (HR=HeardRight, CR=CodeRight, MR=MailRight,
 * CRt=CutRight, GR=GenRight, VR=ViewRight, VcR=VoiceRight, WR=WorkRight,
 * SR=SellRight, ScR=Scraperight, RS=RightSites, RSuite=RightSuite, cX=claudecodeX).
 *
 * Codes are derived from the actual child-repo directory names in this
 * workspace — they are NOT fabricated. The directory is the source of truth;
 * the venture code is a convenience alias for resolver ergonomics. Case-fold
 * comparison makes "HR" and "heardright" both resolve.
 */
const VENTURE_CODES = Object.freeze({
  heardright: "HR",
  coderight: "CR",
  mailright: "MR",
  cutright: "CRt",
  genright: "GR",
  viewright: "VR",
  voiceright: "VcR",
  workright: "WR",
  sellright: "SR",
  scraperight: "ScR",
  rightsites: "RS",
  rightsuite: "RSuite",
  claudecodex: "cX",
});

/**
 * Watcher-state enum from plan 4.1. The plan calls out "current | degraded |
 * stale | unwatched" — exactly one of these per entry.
 *
 * The honest baseline today is "unwatched" — no Membrane-resident watcher
 * exists yet (Phase 1 / 2.7), so we cannot claim current/degraded/stale. A
 * future build that consults the watcher feed will narrow the verdict; until
 * then, "unwatched" is the typed, falsifiable answer, not a fake confidence.
 */
const WATCHER_STATES = Object.freeze(["current", "degraded", "stale", "unwatched"]);
const WATCHER_STATE_SET = new Set(WATCHER_STATES);

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

/** Discover root + registered child Git repositories without indexing child contents. */
export async function discoverRepositoryRoots(workspaceRoot) {
  const root = await realpath(resolve(workspaceRoot));
  // Repository truth comes from the live filesystem, never .gitmodules text.
  const candidates = new Set(["."]);
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

/**
 * Capabilities a repo advertises through the catalog. The list is honest: it
 * enumerates what the catalog can resolve for this repo today, not a wish list.
 * "graph" requires a built graph.db; "git" requires a working tree; the
 * workspace root receives "catalog-binding" because enroll binds it for the
 * whole workspace.
 *
 * @param {{relativeRoot: string, hasGraphDb: boolean, isWorkspaceRoot: boolean}} entry
 * @returns {string[]}
 */
function capabilitiesFor({ relativeRoot, hasGraphDb, isWorkspaceRoot }) {
  const out = ["git"];
  if (hasGraphDb) out.push("graph");
  if (isWorkspaceRoot) out.push("catalog-binding");
  else out.push("catalog-child");
  return [...out].sort();
}

/**
 * Build the alias set for a repo: the directory name (canonical), and the
 * venture code (if known) plus the CamelCase variant of the directory name.
 *
 * Aliases are case-insensitive at resolve time (see resolveByAlias). The
 * venture code is derived from the directory name — no invented repos.
 *
 * @param {string} relativeRoot
 * @returns {string[]}
 */
function aliasesFor(relativeRoot) {
  if (relativeRoot === ".") return [];
  const canonical = relativeRoot.toLowerCase();
  const aliases = new Set([canonical]);
  const camel = canonical
    .replace(/[-_]+/g, " ")
    .split(/\s+/)
    .filter(Boolean)
    .map((part, index) => index === 0 ? part : part.charAt(0).toUpperCase() + part.slice(1))
    .join("");
  if (camel && camel !== canonical) aliases.add(camel);
  const ventureCode = VENTURE_CODES[canonical];
  if (ventureCode) aliases.add(ventureCode.toLowerCase());
  return [...aliases].sort();
}

export async function buildRepositoryCatalog(workspaceRoot, options = {}) {
  const root = await realpath(resolve(workspaceRoot));
  const discovered = await discoverRepositoryRoots(root);
  const workspaceId = options.workspaceId || repositoryId(".");
  // Status is read through Blueprint's published IPC seam concurrently. A missing
  // daemon is a typed degradation, never a graph.db read or a spawned fallback.
  const readinessByAbsolute = new Map(await Promise.all(discovered.map(async ({ absoluteRoot }) => [
    absoluteRoot, await readBlueprintReadiness(absoluteRoot),
  ])));
  const repositories = await Promise.all(discovered.map(async ({ relativeRoot, absoluteRoot }) => {
    const repoId = repositoryId(relativeRoot);
    const isWorkspaceRoot = relativeRoot === ".";
    const graphRel = `${relativeRoot === "." ? "" : `${relativeRoot}/`}.agent/graph/graph.db`;
    const readiness = readinessByAbsolute.get(absoluteRoot);
    const { generationId: blueprintGenerationId, manifestDigest } = readiness ?? { generationId: null, manifestDigest: null };
    const hasGraphDb = Boolean(blueprintGenerationId);
    const rootBinding = absoluteRoot;
    const aliases = aliasesFor(relativeRoot);
    const watcherState = readiness?.freshness ?? "unwatched";
    const capabilities = capabilitiesFor({ relativeRoot, hasGraphDb, isWorkspaceRoot });
    // grantPolicy mirrors the existing enrollment shape so callers do not need
    // to special-case the new field. The workspace-root gets an empty
    // child_repository_ids list; child repos get a parent_repository_id
    // pointer back at the workspace root.
    const grantPolicy = isWorkspaceRoot
      ? { level: "read-only", child_repository_ids: [] }
      : { level: "read-only", parent_repository_id: workspaceId };
    return {
      // Plan 4.1 typed entry surface.
      repoId,
      aliases,
      // Blueprint owns repository identity & source truth. This catalog only
      // carries typed absence until its published contract provides those
      // fields; it never reads Git origin or HEAD directly.
      origin: null,
      rootBinding,
      graphPath: graphRel,
      blueprintGenerationId,
      manifestDigest,
      sourceCommit: null,
      identityStatus: blueprintGenerationId && manifestDigest ? "known" : "unknown",
      identityReason: blueprintGenerationId && manifestDigest ? null : "blueprint_repository_identity_unavailable",
      watcherState,
      capabilities,
      grantPolicy,
    };
  }));
  const body = { schema: "membrane.repository-catalog.v1", workspace_id: workspaceId, repositories };
  return { ...body, catalog_digest: digest(body) };
}

export function hasExplicitChildGrant(catalog, callerRepositoryId, targetRepositoryId, grants = []) {
  const target = catalog?.repositories?.find((entry) => entry.repoId === targetRepositoryId);
  const workspaceId = catalog?.workspace_id || repositoryId(".");
  if (!target || target.repoId === workspaceId) return callerRepositoryId === targetRepositoryId;
  return grants.includes(targetRepositoryId) && callerRepositoryId === workspaceId;
}

export function catalogDigest(catalog) {
  const { catalog_digest: _ignored, ...body } = catalog || {};
  return digest(body);
}

/**
 * Resolve a repo by alias or canonical name. Plan 4.2 — "HR" and "HeardRight"
 * must both resolve to the heardright entry. Comparison is case-insensitive.
 *
 * The catalog stores aliases in canonical lower-case form (see aliasesFor);
 * the resolver lower-cases the query before matching, so HR == hr == HR.
 *
 * @param {object} catalog a catalog built by buildRepositoryCatalog
 * @param {string} alias an alias, venture code, or canonical directory name
 * @returns {object|null} the matching catalog entry, or null if no match
 */
export function resolveByAlias(catalog, alias) {
  if (!catalog?.repositories?.length || typeof alias !== "string" || alias.length === 0) return null;
  const needle = alias.toLowerCase();
  for (const entry of catalog.repositories) {
    if (entry.aliases?.some((candidate) => candidate.toLowerCase() === needle)) return entry;
    if (entry.repoId?.toLowerCase() === needle) return entry;
  }
  return null;
}

export async function enrollRepositoryCatalog(workspaceRoot, options = {}) {
  const root = await realpath(resolve(workspaceRoot));
  const catalog = await buildRepositoryCatalog(root, options);
  const registryPath = options.registryPath || defaultRegistryPath();
  const childGrants = [...new Set(options.childGrants || [])];
  const workspace = catalog.repositories.find((entry) => entry.repoId === catalog.workspace_id);
  const plan = catalog.repositories.map((entry) => ({
    root: entry.rootBinding,
    repository_id: entry.repoId,
    scope_id: `scope-${entry.repoId.slice("repo-".length)}`,
    repository_catalog_digest: catalog.catalog_digest,
    grant_policy: entry.repoId === workspace.repoId
      ? { level: "read-only", child_repository_ids: childGrants }
      : { level: "read-only", parent_repository_id: workspace.repoId },
  }));
  if (options.dryRun) return { action: "catalog_enroll", catalog, registry: registryPath, bindings: plan, dry_run: true };
  const bindings = [];
  for (const binding of plan) bindings.push(await enroll(binding.root, binding, registryPath));
  return { action: "catalog_enroll", catalog, registry: registryPath, bindings, dry_run: false };
}

// Re-exported for any caller that wants to validate the watcher-state enum at
// runtime (the plan fixes it; this export lets a future test or tool check the
// set without re-typing it).
export { WATCHER_STATES };
