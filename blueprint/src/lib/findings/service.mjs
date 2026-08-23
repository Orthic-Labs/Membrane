// Findings resident-service adapter — design §7 D0a/D0b lane, §7.1 Phase-0
// items 5–7.
//
// Assembles generation/hash-bound finding bundles from the canonical detect
// pipeline (registry rules BP001/BP002/BP003) and exposes them through the
// resident daemon surface together with named-generation baselines and SARIF
// rendering. All logic lives here; server.mjs only dispatches.
//
// Binding contract (§7.1 item 5): every bundle is bound AT EMISSION TIME to
// the exact sealed graph generation it was computed against — the
// generationId sealed by finalizeGenerationIdentity in the repo store, never a
// second generation model — plus the sha256 content hash of every scanned
// file contributing to it. Each finding additionally carries the hashes of its
// own evidencePath files.
//
// Freshness honesty (canonical doctrine §10.1/§10.2): a bundle whose working
// tree has moved past the sealed generation is served with freshness:"stale"
// and a typed `stale_generation` omission — it is never silently recomputed as
// if it were current. Callers that cannot tolerate stale evidence pass
// allowStale:false and receive a typed `stale_blocked` failure instead.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { SCAN_EXCLUSIONS } from "../../graph/static-provider.mjs";
import { normalizeIgnoredPrefixes, pathMatchesIgnoredPrefix } from "../../graph/ignored-prefixes.mjs";
import { closeStore, openStoreReadOnly } from "../../graph/store-sqlite.mjs";
import { readIndexedMeta } from "../../graph/traverse-store.mjs";
import { observeRepositoryFreshness } from "../../sources/freshness-observation.mjs";
import { fail } from "../application/errors.mjs";
import { toSarif } from "../sarif.mjs";
import { detectFindings } from "./detect.mjs";
import { normalizeRepoPath } from "./specifier.mjs";

export const FINDINGS_SERVICE_METHODS = Object.freeze([
  "findings.get",
  "findings.baseline.capture",
  "findings.baseline.list",
  "findings.sarif",
]);

const MAX_CACHED_BUNDLES = 16;

function sha256Content(bytes) {
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

function throwIfAborted(signal) {
  if (signal?.aborted) fail("request_cancelled", "request cancelled");
}

/**
 * Bind a detect run to its generation identity. The bundle records the sealed
 * generationId, the sha256 content hash of every scanned file, and — on each
 * finding — the hashes of exactly the files in its evidencePath. This is the
 * single binding implementation shared by the resident service and the
 * watcher lane (re-exported through src/graph/watchman.mjs).
 */
export async function buildGenerationBoundBundle({
  files,
  generationId,
  generationName = null,
  manifestDigest = null,
  existsOutsideScan = null,
}) {
  const hashesByName = new Map();
  const scanned = [];
  for (const file of files ?? []) {
    const path = normalizeRepoPath(file.path);
    const bytes = Buffer.isBuffer(file.bytes) ? file.bytes : Buffer.from(String(file.text ?? ""), "utf8");
    hashesByName.set(path, sha256Content(bytes));
    scanned.push({ path, text: Buffer.isBuffer(file.bytes) ? bytes.toString("utf8") : String(file.text ?? "") });
  }
  const detected = await detectFindings({ files: scanned, generationId, existsOutsideScan });
  const findings = detected.findings.map((finding) => ({
    ...finding,
    generationId,
    perFileContentHashes: Object.fromEntries(
      (finding.evidencePath ?? []).filter((path) => hashesByName.has(path)).map((path) => [path, hashesByName.get(path)]),
    ),
  }));
  return Object.freeze({
    schemaVersion: 1,
    kind: "findings-bundle",
    generationId,
    generationName,
    manifestDigest,
    perFileContentHashes: Object.fromEntries([...hashesByName.entries()].sort(([left], [right]) => left.localeCompare(right))),
    findings,
    omissions: detected.omissions,
    coverage: detected.coverage,
  });
}

function deltaEntry(finding) {
  return {
    fingerprint: finding.fingerprint,
    ruleId: finding.ruleId,
    path: finding.path,
    startLine: finding.startLine ?? null,
    endLine: finding.endLine ?? null,
    name: finding.name ?? null,
    specifier: finding.specifier ?? null,
    severity: finding.severity ?? null,
  };
}

function identityKey(entry) {
  // Fingerprints cover (rule, path, name, specifier), so a specifier rewrite
  // under the same rule+path+name identity lands here as one changed pair
  // instead of an unrelated resolved+added couple.
  return `${entry.ruleId}|${entry.path}|${entry.name ?? ""}`;
}

function byLocation(left, right) {
  return left.path.localeCompare(right.path)
    || (left.startLine ?? 0) - (right.startLine ?? 0)
    || String(left.ruleId).localeCompare(String(right.ruleId))
    || String(left.fingerprint).localeCompare(String(right.fingerprint));
}

/** Delta of the current findings against a captured baseline record. */
export function computeBaselineDelta(findings, baselineRecord) {
  const baselineByFingerprint = new Map((baselineRecord?.findings ?? []).map((entry) => [entry.fingerprint, entry]));
  const currentByFingerprint = new Map((findings ?? []).map((finding) => [finding.fingerprint, finding]));
  const added = (findings ?? []).filter((finding) => !baselineByFingerprint.has(finding.fingerprint)).map(deltaEntry).sort(byLocation);
  const resolved = (baselineRecord?.findings ?? [])
    .filter((entry) => !currentByFingerprint.has(entry.fingerprint))
    .map(deltaEntry)
    .sort(byLocation);
  const unresolvedByIdentity = new Map(resolved.map((entry) => [identityKey(entry), entry]));
  const changed = [];
  const stillAdded = [];
  for (const entry of added) {
    const counterpart = unresolvedByIdentity.get(identityKey(entry));
    if (counterpart) {
      unresolvedByIdentity.delete(identityKey(entry));
      changed.push(entry);
    } else {
      stillAdded.push(entry);
    }
  }
  const stillResolved = resolved.filter((entry) => unresolvedByIdentity.has(identityKey(entry)));
  return {
    baselineGeneration: baselineRecord?.generationId ?? null,
    baselineName: baselineRecord?.name ?? null,
    added: stillAdded,
    resolved: stillResolved,
    changed: changed.sort(byLocation),
  };
}

function typedOmission(code, detail, extra = {}) {
  return { code, detail, ...extra };
}

function detectionOmission(entry) {
  const detail = [entry.path, entry.specifier ? `"${entry.specifier}"` : null, entry.detail]
    .filter(Boolean)
    .join(" ");
  return { code: entry.reason, detail, path: entry.path, line: entry.line ?? null, specifier: entry.specifier ?? null };
}

function matchesPaths(path, prefixes) {
  if (!prefixes?.length) return true;
  return prefixes.some((raw) => {
    const prefix = normalizeRepoPath(raw).replace(/^\.\//, "").replace(/\/$/, "");
    return prefix && (path === prefix || path.startsWith(`${prefix}/`));
  });
}

// --- production dependency defaults (injected seams for tests) -------------

function defaultRepositoryConfig(root, outDir) {
  const path = join(root, outDir, "config.json");
  if (!existsSync(path)) return {};
  try { return JSON.parse(readFileSync(path, "utf8")) ?? {}; } catch { return {}; }
}

function defaultScanRepository(root, config) {
  const ignored = normalizeIgnoredPrefixes(config?.ignoredPrefixes);
  const exclusions = new Set(SCAN_EXCLUSIONS);
  let listed;
  try {
    const raw = execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard", "-z"], {
      cwd: root,
      stdio: ["ignore", "pipe", "ignore"],
      maxBuffer: 512 * 1024 * 1024,
      windowsHide: true,
    });
    listed = raw.toString("utf8").split("\0").filter(Boolean);
  } catch {
    listed = [];
    const walk = (dir) => {
      let entries = [];
      try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
      for (const entry of entries) {
        if ([".git", "node_modules", "target"].includes(entry.name)) continue;
        const full = join(dir, entry.name);
        if (entry.isDirectory()) walk(full);
        else if (entry.isFile()) listed.push(relative(root, full));
      }
    };
    walk(root);
  }
  const seen = new Set();
  const files = [];
  for (const rawPath of listed) {
    const path = normalizeRepoPath(rawPath).replace(/^\.\//, "");
    if (!path || seen.has(path)) continue;
    if (path.split("/").some((segment) => exclusions.has(segment))) continue;
    if (pathMatchesIgnoredPrefix(path, ignored)) continue;
    let stat = null;
    try { stat = statSync(join(root, path)); } catch { continue; }
    if (!stat.isFile()) continue;
    let bytes;
    try { bytes = readFileSync(join(root, path)); } catch { continue; }
    seen.add(path);
    files.push({ path, text: bytes.toString("utf8"), bytes });
  }
  files.sort((left, right) => left.path.localeCompare(right.path));
  return files;
}

function defaultSealedGeneration(root, outDir) {
  const dbPath = join(root, outDir, "graph", "graph.db");
  if (!existsSync(dbPath)) fail("graph_missing", `Graph store is missing for ${root}.`);
  let db = null;
  try {
    db = openStoreReadOnly(dbPath);
    const meta = readIndexedMeta(db);
    const generationId = meta?.manifest?.generationId ?? null;
    if (!generationId) fail("graph_missing", "No sealed generation is available.");
    return {
      generationId,
      manifestDigest: meta.manifest.manifestDigest ?? null,
      baseCommit: meta.manifest.repo?.baseCommit ?? null,
      schemaVersion: meta.schemaVersion ?? null,
    };
  } finally {
    if (db) closeStore(db);
  }
}

function defaultFreshnessOverlay(root, baseCommit) {
  return observeRepositoryFreshness(resolve(root), { baseCommit });
}

function defaultToolVersion() {
  try {
    return JSON.parse(readFileSync(fileURLToPath(new URL("../../../package.json", import.meta.url)), "utf8")).version ?? "0.0.0";
  } catch {
    return "0.0.0";
  }
}

// --- baseline persistence beside existing daemon state (~/.blueprint) -------

function repositoryStateKey(root) {
  return createHash("sha256").update(resolve(root)).digest("hex").slice(0, 16);
}

function slugifyName(value) {
  const slug = String(value ?? "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 80);
  if (!slug) fail("baseline_name_invalid", "A baseline capture requires a non-empty name.");
  return slug;
}

function writeJsonAtomic(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  const tmp = `${path}.tmp-${process.pid}`;
  writeFileSync(tmp, `${JSON.stringify(value, null, 2)}\n`);
  renameSync(tmp, path);
}

function isBaselineRecord(record) {
  return record?.kind === "findings-baseline" && typeof record.name === "string";
}

function readBaselineRecords(baselinesDir) {
  let names = [];
  try { names = readdirSync(baselinesDir).filter((name) => name.endsWith(".json")); } catch { return []; }
  const records = [];
  for (const name of names.sort()) {
    const path = join(baselinesDir, name);
    try {
      const record = JSON.parse(readFileSync(path, "utf8"));
      if (isBaselineRecord(record)) records.push({ record, path });
    } catch {
      // An unreadable or corrupt baseline file is skipped, never served.
    }
  }
  return records;
}

function byNewestFirst(left, right) {
  return String(right.record.createdAt ?? "").localeCompare(String(left.record.createdAt ?? ""));
}

/**
 * Resolve a baseline reference: an exact captured name first, then a literal
 * generationId captured under any name (most recent capture wins).
 */
function resolveBaselineRecord(root, ref, baselinesDir) {
  const wanted = String(ref ?? "").trim();
  if (!wanted) return null;
  const records = readBaselineRecords(baselinesDir);
  return records.find(({ record }) => record.name === wanted)
    ?? records.slice().sort(byNewestFirst).find(({ record }) => record.generationId === wanted)
    ?? null;
}

// --- the resident findings service ------------------------------------------

/**
 * @param {{
 *   outDir?: string,
 *   stateDir?: string|null,
 *   scanRepository?: (root: string, config: object) => Array<{path: string, text: string, bytes?: Buffer}> | Promise<...>,
 *   repositoryConfig?: (root: string, outDir: string) => object,
 *   sealedGeneration?: (root: string, outDir: string) => {generationId, manifestDigest?, baseCommit?, schemaVersion?},
 *   freshnessOverlay?: (root: string, baseCommit: string|null) => object,
 *   clock?: () => string,
 * }} deps injected seams; defaults wire the real store, scanner and overlay.
 */
export function createFindingsService({
  outDir = ".agent",
  stateDir = null,
  scanRepository = defaultScanRepository,
  repositoryConfig = defaultRepositoryConfig,
  sealedGeneration = defaultSealedGeneration,
  freshnessOverlay = defaultFreshnessOverlay,
  clock = () => new Date().toISOString(),
} = {}) {
  const bundleCache = new Map(); // `${root}\0${generationId}` -> bundle

  function resolveRoot(input = {}) {
    return resolve(String(input.repoRoot ?? process.cwd()));
  }

  function baselineDirFor(root) {
    return stateDir ? resolve(stateDir) : join(homedir(), ".blueprint", "findings-baselines", repositoryStateKey(root));
  }

  function isCurrentOverlay(overlay) {
    return Boolean(overlay?.available && overlay?.stable && !overlay?.limitExceeded && (overlay?.entries?.length ?? 0) === 0);
  }

  function stalenessOmissions(overlay) {
    if (isCurrentOverlay(overlay)) return [];
    return [typedOmission(
      "stale_generation",
      overlay?.reason
        ? `working tree differs from the sealed generation (${overlay.reason})`
        : "working tree differs from the sealed generation",
      { dirtyFileCount: overlay?.entries?.length ?? null },
    )];
  }

  async function currentGeneration(input = {}, { signal } = {}) {
    throwIfAborted(signal);
    const root = resolveRoot(input);
    const effectiveOutDir = String(input.outDir ?? outDir);
    const sealed = await Promise.resolve(sealedGeneration(root, effectiveOutDir));
    if (input.generation && input.generation !== sealed.generationId) {
      fail("generation_mismatch", "Requested generation is not current.", {
        expected: input.generation,
        observed: sealed.generationId,
      });
    }
    return { root, effectiveOutDir, sealed };
  }

  function latestBaselineNameForGeneration(root, generationId) {
    const match = readBaselineRecords(baselineDirFor(root))
      .filter(({ record }) => record.generationId === generationId)
      .sort(byNewestFirst)[0];
    return match?.record.name ?? null;
  }

  async function bundleFor(root, effectiveOutDir, sealed, { signal } = {}) {
    const cacheKey = `${root}\u0000${sealed.generationId}`;
    const cached = bundleCache.get(cacheKey);
    if (cached) return cached;
    throwIfAborted(signal);
    const config = repositoryConfig(root, effectiveOutDir);
    const files = await Promise.resolve(scanRepository(root, config));
    const bundle = await buildGenerationBoundBundle({
      files,
      generationId: sealed.generationId,
      generationName: latestBaselineNameForGeneration(root, sealed.generationId),
      manifestDigest: sealed.manifestDigest,
    });
    bundleCache.set(cacheKey, bundle);
    if (bundleCache.size > MAX_CACHED_BUNDLES) bundleCache.delete(bundleCache.keys().next().value);
    return bundle;
  }

  function enforceFreshnessTolerance(stale, overlay, sealed, input) {
    if (!stale || input.allowStale !== false) return;
    fail("stale_blocked", "Findings for the requested generation are stale; rebuild or pass allowStale to accept known-stale evidence.", {
      generationId: sealed.generationId,
      reason: overlay?.reason ?? null,
    });
  }

  /**
   * findings.get {paths?, baselineGeneration?, generation?, allowStale?}
   *   → {schemaVersion, kind, root, generationId, generationName, freshness,
   *      findings, delta|null, omissions:[{code, detail, ...}], coverage,
   *      perFileContentHashes}
   *
   * Serves the sealed generation pinned above; a moved-on working tree yields
   * freshness:"stale" plus a typed omission rather than silent recomputation.
   * allowStale:false refuses stale evidence with a typed stale_blocked error.
   */
  async function findingsGet(input = {}, { signal } = {}) {
    const { root, effectiveOutDir, sealed } = await currentGeneration(input, { signal });
    const overlay = await Promise.resolve(freshnessOverlay(root, sealed.baseCommit));
    const stale = !isCurrentOverlay(overlay);
    enforceFreshnessTolerance(stale, overlay, sealed, input);
    const bundle = await bundleFor(root, effectiveOutDir, sealed, { signal });
    throwIfAborted(signal);
    const prefixes = (Array.isArray(input.paths) ? input.paths : []).map(normalizeRepoPath).filter(Boolean);
    const findings = bundle.findings.filter((finding) => matchesPaths(finding.path, prefixes));
    const omissions = [
      ...(stale ? stalenessOmissions(overlay) : []),
      ...bundle.omissions.filter((omission) => matchesPaths(omission.path, prefixes)).map(detectionOmission),
    ];

    let delta = null;
    if (input.baselineGeneration != null && input.baselineGeneration !== "") {
      const match = resolveBaselineRecord(root, input.baselineGeneration, baselineDirFor(root));
      if (!match) {
        omissions.push(typedOmission("baseline_unknown", `no captured baseline matches "${input.baselineGeneration}"`));
      } else {
        delta = computeBaselineDelta(bundle.findings, match.record);
        if (prefixes.length) {
          delta = {
            ...delta,
            added: delta.added.filter((entry) => matchesPaths(entry.path, prefixes)),
            resolved: delta.resolved.filter((entry) => matchesPaths(entry.path, prefixes)),
            changed: delta.changed.filter((entry) => matchesPaths(entry.path, prefixes)),
          };
        }
      }
    }

    return {
      schemaVersion: 1,
      kind: "findings.get",
      root,
      generationId: bundle.generationId,
      generationName: bundle.generationName,
      freshness: stale ? "stale" : "current",
      findings,
      delta,
      omissions,
      coverage: bundle.coverage,
      perFileContentHashes: bundle.perFileContentHashes,
    };
  }

  /** findings.baseline.capture {name} — persist the current bundle as a named
   * generation baseline beside existing daemon state (~/.blueprint). */
  async function baselineCapture(input = {}, { signal } = {}) {
    throwIfAborted(signal);
    const name = slugifyName(input.name);
    const { root, effectiveOutDir, sealed } = await currentGeneration(input, { signal });
    const overlay = await Promise.resolve(freshnessOverlay(root, sealed.baseCommit));
    const bundle = await bundleFor(root, effectiveOutDir, sealed, { signal });
    const path = join(baselineDirFor(root), `${name}.json`);
    const record = {
      schemaVersion: 1,
      kind: "findings-baseline",
      name,
      generationId: sealed.generationId,
      manifestDigest: sealed.manifestDigest,
      createdAt: clock(),
      repository: root,
      coverage: bundle.coverage,
      findingCount: bundle.findings.length,
      findings: bundle.findings.map(deltaEntry),
    };
    throwIfAborted(signal);
    writeJsonAtomic(path, record);
    // The bundle's generationName may now resolve; drop any cached copy.
    bundleCache.delete(`${root}\u0000${sealed.generationId}`);
    return {
      schemaVersion: 1,
      kind: "findings.baseline.capture",
      root,
      name,
      generationId: sealed.generationId,
      findingCount: record.findingCount,
      path,
      freshness: isCurrentOverlay(overlay) ? "current" : "stale",
    };
  }

  /** findings.baseline.list {} — captured named-generation baselines. */
  function baselineList(input = {}, { signal } = {}) {
    throwIfAborted(signal);
    const root = resolveRoot(input);
    const baselines = readBaselineRecords(baselineDirFor(root))
      .map(({ record, path }) => ({
        name: record.name,
        generationId: record.generationId ?? null,
        createdAt: record.createdAt ?? null,
        findingCount: Number(record.findingCount ?? record.findings?.length ?? 0),
        path,
      }))
      .sort((left, right) => left.name.localeCompare(right.name));
    return { schemaVersion: 1, kind: "findings.baseline.list", root, baselines };
  }

  /** findings.sarif {paths?, toolVersion?, allowStale?} → SARIF 2.1.0 rendered
   * from the SAME bound finding objects via lib/sarif.mjs — rendering, never
   * an independent truth source. */
  async function findingsSarif(input = {}, { signal } = {}) {
    const { root, effectiveOutDir, sealed } = await currentGeneration(input, { signal });
    const overlay = await Promise.resolve(freshnessOverlay(root, sealed.baseCommit));
    const stale = !isCurrentOverlay(overlay);
    enforceFreshnessTolerance(stale, overlay, sealed, input);
    const bundle = await bundleFor(root, effectiveOutDir, sealed, { signal });
    throwIfAborted(signal);
    const prefixes = (Array.isArray(input.paths) ? input.paths : []).map(normalizeRepoPath).filter(Boolean);
    const findings = bundle.findings.filter((finding) => matchesPaths(finding.path, prefixes));
    return {
      schemaVersion: 1,
      kind: "findings.sarif",
      root,
      generationId: bundle.generationId,
      freshness: stale ? "stale" : "current",
      findingCount: findings.length,
      omissions: [
        ...(stale ? stalenessOmissions(overlay) : []),
        ...bundle.omissions.filter((omission) => matchesPaths(omission.path, prefixes)).map(detectionOmission),
      ],
      sarif: toSarif(findings, String(input.toolVersion ?? defaultToolVersion())),
    };
  }

  return Object.freeze({
    "findings.get": findingsGet,
    "findings.baseline.capture": baselineCapture,
    "findings.baseline.list": baselineList,
    "findings.sarif": findingsSarif,
  });
}
