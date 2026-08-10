import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { realpath } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { defaultRegistryPath, enroll } from "./project-registry.mjs";
import { readCortexReadiness } from "./cortex-readiness.mjs";

// Resolved once: the absolute URL to Cortex's exported static-provider reader.
// Membrane never re-parses graph.db — it calls the exported readGeneration,
// which is the only contract for reading a persisted generation.
//
// MBR-015: this is OPTIONAL and sibling-independent. The default points at a
// sibling source checkout, but the import is a guarded dynamic import (it
// degrades to a null generation when absent), so the installed runtime needs
// no sibling source tree. `MEMBRANE_CORTEX_STATIC_PROVIDER` overrides the path
// for installed layouts that ship the reader at a different location.
const CORTEX_STATIC_PROVIDER_URL = (() => {
  const override = process.env.MEMBRANE_CORTEX_STATIC_PROVIDER?.trim();
  if (override) return pathToFileURL(resolve(override)).href;
  const here = dirname(fileURLToPath(import.meta.url));
  // membrane/mcp/repository-catalog.mjs -> membrane/mcp -> membrane -> workspace -> cortex/graph/static-provider.mjs
  return pathToFileURL(resolve(here, "..", "..", "cortex", "graph", "static-provider.mjs")).href;
})();

const SUBMODULE_PATH = /^\s*path\s*=\s*(.+?)\s*$/gm;

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

/**
 * Read the git origin URL for a repo, or empty string if no origin is configured.
 * @param {string} root absolute path to the repo root
 * @returns {string}
 */
function readGitOrigin(root) {
  try {
    const out = execFileSync("git", ["-C", root, "config", "--get", "remote.origin.url"], { encoding: "utf8" });
    return out.trim();
  } catch {
    return "";
  }
}

/**
 * Read the HEAD commit SHA for a repo, or empty string if HEAD is unborn.
 * @param {string} root absolute path to the repo root
 * @returns {string}
 */
function readGitHead(root) {
  try {
    const out = execFileSync("git", ["-C", root, "rev-parse", "HEAD"], { encoding: "utf8" });
    return out.trim();
  } catch {
    return "";
  }
}

/**
 * Probe the repo's .agent/graph/graph.db for the current Cortex generation.
 * Returns { generationId, manifestDigest } or { generationId: null,
 * manifestDigest: null } when the graph is unbuilt or Cortex cannot be
 * reached. The store is read via Cortex's own exported reader
 * (cortex/graph/static-provider.mjs) — Membrane never re-parses graph.db.
 *
 * @param {string} root absolute path to the repo root
 * @returns {Promise<{generationId: string|null, manifestDigest: string|null}>}
 */
async function readCortexGeneration(root) {
  let mod;
  try {
    mod = await import(CORTEX_STATIC_PROVIDER_URL);
  } catch {
    return { generationId: null, manifestDigest: null };
  }
  try {
    const gen = mod?.readGeneration?.(root, ".agent");
    const manifest = gen?.manifest ?? null;
    return {
      generationId: manifest?.generationId ?? null,
      manifestDigest: manifest?.manifestDigest ?? null,
    };
  } catch {
    return { generationId: null, manifestDigest: null };
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

/**
 * Determine watcher state by reading the repo's `watch_state` table in graph.db.
 *
 * Plan 2.7a: replaces the hardcoded "unwatched" stub. The keys written by
 * the Cortex watchman supervisor (supervisor.mjs:73-112) are `watcher_pid`,
 * `event_gap`, `event_gap_reason`, `last_error`, `source_clock`, `applied_clock`.
 *
 * Mapping to exactly four values (no others) — delegated to
 * Cortex's authoritative readiness (MBR-009):
 *   - no graph.db or no row → "unwatched"
 *   - watcher_pid missing    → "unwatched" (watcher_never_started)
 *   - watcher_pid dead       → "unwatched" (watcher_process_dead)
 *   - watcher_pid alive but owner stale → "unwatched" (watcher_owner_stale)
 *   - watcher alive + event_gap → "degraded"
 *   - watcher alive + pending events → "stale"
 *   - watcher alive + no gap → "current"
 *
 * @param {string} absoluteRoot absolute path to the repo root
 * @returns {"current"|"degraded"|"stale"|"unwatched"}
 */
function readWatcherState(absoluteRoot) {
  // MBR-009: delegate to Cortex's authoritative readiness contract. Stale
  // actors (never started / process dead / owner superseded) are unwatched;
  // stale data (event gap / pending events) is degraded/stale. A supervisor
  // restart no longer marks live repos dead.
  return readCortexReadiness(absoluteRoot).freshness;
}

export async function buildRepositoryCatalog(workspaceRoot, options = {}) {
  const root = await realpath(resolve(workspaceRoot));
  const discovered = await discoverRepositoryRoots(root);
  const workspaceId = options.workspaceId || repositoryId(".");
  // Read each repo's generation through Cortex's exported reader. We do this
  // up-front so a slow/missing graph.db does not delay catalog construction
  // by serializing the entry mapping; an async per-entry probe would have the
  // same shape but worse latency because cortex/graph/import cost is paid on
  // the first call regardless.
  const generationByAbsolute = new Map();
  for (const { absoluteRoot } of discovered) {
    generationByAbsolute.set(absoluteRoot, await readCortexGeneration(absoluteRoot));
  }
  const repositories = discovered.map(({ relativeRoot, absoluteRoot }) => {
    const repository_id = repositoryId(relativeRoot);
    const isWorkspaceRoot = relativeRoot === ".";
    const graphRel = `${relativeRoot === "." ? "" : `${relativeRoot}/`}.agent/graph/graph.db`;
    const graphAbs = join(absoluteRoot, ".agent", "graph", "graph.db");
    const hasGraphDb = existsSync(graphAbs);
    const { generationId: cortexGenerationId, manifestDigest } = generationByAbsolute.get(absoluteRoot) ?? { generationId: null, manifestDigest: null };
    const origin = readGitOrigin(absoluteRoot);
    const sourceCommit = readGitHead(absoluteRoot);
    const rootBinding = absoluteRoot;
    const aliases = aliasesFor(relativeRoot);
    const watcherState = readWatcherState(absoluteRoot);
    const capabilities = capabilitiesFor({ relativeRoot, hasGraphDb, isWorkspaceRoot });
    // grantPolicy mirrors the existing enrollment shape so callers do not need
    // to special-case the new field. The workspace-root gets an empty
    // child_repository_ids list; child repos get a parent_repository_id
    // pointer back at the workspace root.
    const grantPolicy = isWorkspaceRoot
      ? { level: "read-only", child_repository_ids: [] }
      : { level: "read-only", parent_repository_id: repositoryId(".") };
    return {
      // Plan 4.1 typed entry surface.
      repoId: repository_id,
      aliases,
      origin,
      rootBinding,
      graphPath: graphRel,
      cortexGenerationId,
      manifestDigest,
      sourceCommit,
      watcherState,
      capabilities,
      grantPolicy,
      // Legacy fields — preserved so the existing callers in
      // mcp/server.test.mjs, mcp/server.mjs, mcp/install.mjs, and the rest of
      // the binding surface still resolve. The plan does not retire these; it
      // adds the typed layer above them.
      repository_id,
      root: relativeRoot,
      scope_id: scopeId(relativeRoot),
      role: isWorkspaceRoot ? "workspace-root" : "child-repository",
      ...(isWorkspaceRoot ? {} : { parent_repository_id: repositoryId(".") }),
      // `cortex_graph` is the legacy path-pointer to the local graph.db. The
      // plan moved this to `graphPath`; we keep `cortex_graph` for
      // backward-compat with the existing entry shape used by caller code
      // that still reads it. The retired `blueprint_manifest` pointer (defect
      // 26) is GONE — that path no longer exists and the contract asserts it
      // must not appear on any entry.
      cortex_graph: graphRel,
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
    if (entry.root?.toLowerCase() === needle) return entry;
    if (entry.repoId?.toLowerCase() === needle) return entry;
  }
  return null;
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

// Re-exported for any caller that wants to validate the watcher-state enum at
// runtime (the plan fixes it; this export lets a future test or tool check the
// set without re-typing it).
export { WATCHER_STATES };
