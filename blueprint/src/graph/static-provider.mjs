import { randomUUID } from "node:crypto";
import { createXXHash128 } from "hash-wasm";
import { spawnSync } from "node:child_process";
import { performance } from "node:perf_hooks";
import {
  existsSync,
  mkdirSync,
  opendirSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import {
  CODE_EXTENSIONS,
  extractCallNames,
  extractImports,
  extractSymbols,
  extractUnresolvedImportSpecifiers,
} from "./language-extractors.mjs";
import { languageCapabilityRecords } from "./language-registry.mjs";
import { addSchemaReferenceEdges } from "./schema-extractors.mjs";
import { emptyCache, loadParseCache, writeParseCache, nextCache } from "./parse-cache.mjs";
import {
  EDGE_CONFIDENCE_TIERS,
  EDGE_CONFIDENCE_TIER_ORDER,
  EDGE_CONFIDENCE_TIER_DESCRIPTIONS,
  tierConfidence,
} from "./confidence-tiers.mjs";
import { PRECISION_TIERS, PRECISION_TIER_ORDER } from "./precision-tiers.mjs";
import { STATIC_PROVIDER, TREESITTER_PROVIDER } from "./provider-identity.mjs";
import { compareRepoPaths } from "./path-order.mjs";
import {
  openStore,
  openStoreReadOnly,
  closeStore,
  saveGeneration,
  loadGeneration,
  hasGeneration,
  getGenerationEnvelope,
  loadFileContentHashes,
} from "./store-sqlite.mjs";
import { finalizeGenerationIdentity, computeGenerationId, computeManifestDigest } from "./generation-identity.mjs";
import { sliceTokens } from "../lib/token-budget.mjs";
import { diffLedgerAgainstTree } from "./merkle-ledger.mjs";
import { probeScip } from "./scip-provider.mjs";
import { normalizeIgnoredPrefixes, pathMatchesIgnoredPrefix } from "./ignored-prefixes.mjs";

export { EDGE_CONFIDENCE_TIERS, EDGE_CONFIDENCE_TIER_ORDER, EDGE_CONFIDENCE_TIER_DESCRIPTIONS, tierConfidence };
export { PRECISION_TIERS, PRECISION_TIER_ORDER };
export { augmentGenerationWithScip } from "./scip-provider.mjs";

const PROVIDER = STATIC_PROVIDER;
const BENCHMARK_TIMING_PATH = process.env.BLUEPRINT_BENCHMARK_TIMINGS_PATH ?? null;
let environmentTimingSink = null;

// Opt-in only: normal graph construction allocates no timing state or output.
// The benchmark process receives these records on exit, after all graph work.
function timingSink(options = {}) {
  if (options.timingSink && typeof options.timingSink.record === "function") return options.timingSink;
  if (!BENCHMARK_TIMING_PATH) return null;
  if (!environmentTimingSink) {
    const records = [];
    environmentTimingSink = { record: (stage, elapsedMs) => records.push({ stage, elapsedMs }) };
    process.once("exit", () => {
      try { writeFileSync(BENCHMARK_TIMING_PATH, `${JSON.stringify({ schema: "blueprint-build-stage-timings-v1", records })}\n`); } catch { /* benchmark evidence must not alter build outcome */ }
    });
  }
  return environmentTimingSink;
}
function measureStage(sink, stage, work) {
  if (!sink) return work();
  const started = performance.now();
  try { return work(); } finally { sink.record(stage, performance.now() - started); }
}
async function measureStageAsync(sink, stage, work) {
  if (!sink) return await work();
  const started = performance.now();
  try { return await work(); } finally { sink.record(stage, performance.now() - started); }
}

// Blueprint writes its own graph into `--out`, and tests point that at suffixed
// directories (`.agent-test73`, `.agent-test-graph`). Naming only `.agent` let a
// leaked run directory count graph.db/-shm/-wal as unparseable source, which
// reported a healthy repo as `degraded`.
function isBlueprintOutputDir(name) {
  return /^\.agent(-|$)/.test(name);
}

const IGNORED = new Set([
  // VCS internals — git refs, codex checkpoints, branch metadata. Walking
  // these on a real repo overflows both the stack and memory because they
  // contain transient file handles that disappear between scans.
  ".git",
  ".codex-tmp",
  // Local graph/build caches and generated internals.
  ".agent",
  ".agent-test-graph",
  ".audit",
  ".cache",
  ".next",
  ".nuxt",
  ".output",
  ".parcel-cache",
  ".pytest_cache",
  ".svelte-kit",
  ".turbo",
  ".vercel",
  ".worktrees",
  // Python / Java / Node caches and package metadata.
  "__pycache__",
  ".gradle",
  ".idea",
  ".mypy_cache",
  ".ruff_cache",
  ".tox",
  ".vscode",
  ".yarn",
  ".pnpm-store",
  "coverage",
  "htmlcov",
  "node_modules",
  "target",
  "dist",
  "build",
  "out",
  "vendor",
  ".serverless",
  // Defect 17 — fixture repositories used by qualification are excluded at
  // every nesting depth without hiding legitimate evaluation harness code.
  "fixture-repos",
]);
export const SCAN_EXCLUSIONS = Object.freeze([...IGNORED].sort());
const IGNORED_FILE_NAMES = new Set([
  ".DS_Store",
  "Thumbs.db",
  // Blueprint's own generated docs. They must not be re-indexed or they
  // would perturb the graph's source hash between rebuilds.
  "product.md",
  "architecture.md",
]);
// Extensions that are tracked as file nodes without symbol/import extraction.
// These do NOT trigger `degraded` because the provider handles them honestly.
const FILE_ONLY_EXTENSIONS = new Set([
  "md", "markdown", "txt",
  "json", "jsonl", "yaml", "yml", "toml",
  "html", "css", "svg",
  "sql", "csv", "tsv",
  "xml",
  "template",
]);
const FILE_ONLY_FILE_NAMES = new Set([".dockerignore", ".gitattributes"]);
const OPAQUE_FILE_EXTENSIONS = new Set([
  "png", "jpg", "jpeg", "webp", "gif", "ico", "bmp", "avif",
  "ttf", "otf", "woff", "woff2",
  "wav", "mp3", "flac", "ogg", "mp4", "mov", "webm",
  "pdf", "zip", "gz", "tar", "dmg", "exe", "dll", "so", "dylib", "wasm", "pdb",
  "onnx", "safetensors", "gguf", "bin",
  "mtlx", "tres", "srt", "lock", "hash", "sha256", "gitignore",
]);
// Canonical catalog is JSON-backed & import-safe: status never loads WASM just
// to answer whether an extension has an AST provider.
const AST_LANGUAGE_EXTENSIONS = new Set(languageCapabilityRecords().flatMap((record) => record.extensions));
const PARSED_EXTENSIONS = [...new Set([...CODE_EXTENSIONS, ...AST_LANGUAGE_EXTENSIONS])].sort();
const SUPPORTED_EXTENSIONS = new Set([...PARSED_EXTENSIONS, ...FILE_ONLY_EXTENSIONS, ...OPAQUE_FILE_EXTENSIONS]);

function sourceExtension(path) {
  const dot = path.lastIndexOf(".");
  return dot < 0 ? "" : path.slice(dot + 1).toLowerCase();
}

function isFileOnlyPath(path) {
  return FILE_ONLY_FILE_NAMES.has(basename(path)) || FILE_ONLY_EXTENSIONS.has(sourceExtension(path));
}

export function sourceCapabilityCoverage(files) {
  const unsupportedExtensions = new Set();
  let unsupportedFileCount = 0;
  for (const file of files) {
    if (FILE_ONLY_FILE_NAMES.has(basename(file.path))) continue;
    const ext = sourceExtension(file.path);
    if (!ext || SUPPORTED_EXTENSIONS.has(ext) || !/^[a-z0-9_+\-.]+$/.test(ext)) continue;
    unsupportedExtensions.add(ext);
    unsupportedFileCount += 1;
  }
  return { unsupportedExtensions: [...unsupportedExtensions].sort(), unsupportedFileCount };
}

function canonicalRoot(value) {
  const root = resolve(value);
  try { return realpathSync(root); } catch { return root; }
}

export function scanSourcesPublic(root, fileLimit = 0, walkOptions = {}) {
  return scanSources(canonicalRoot(root), fileLimit, walkOptions);
}

export function scanSourceMetadataPublic(root, walkOptions = {}) {
  const canonical = canonicalRoot(root);
  const traversal = walk(canonical, { ...walkOptions, ignoredPrefixes: configuredIgnoredPrefixes(canonical, walkOptions) });
  const files = [];
  for (const absolutePath of traversal.paths) {
    try {
      const stat = statSync(absolutePath);
      files.push({
        path: normalizePath(relative(canonical, absolutePath)),
        size: stat.size,
        mtimeMs: stat.mtimeMs,
        identity: stat.dev !== undefined && stat.ino !== undefined ? `${stat.dev}:${stat.ino}` : null,
      });
    } catch {
      /* concurrent removal is represented by the missing path */
    }
  }
  files.sort((left, right) => compareRepoPaths(left.path, right.path));
  return { files, traversalTruncated: traversal.state.truncated, truncationReasons: [...traversal.reasons].sort() };
}

export function sourceHashPublic(files) {
  return sourceHash(files);
}

export function buildGraphGeneration(repoRoot, options = {}) {
  const root = canonicalRoot(repoRoot);
  const outDir = options.outDir ? resolve(root, options.outDir) : null;
  const sink = timingSink(options);
  const source = measureStage(sink, "source_scan", () => scanSources(root, options.fileLimit || 0, options));
  // B2 incremental: reuse per-file symbol extraction for byte-identical files.
  // Resolution (imports/calls/config) still runs globally over the union, so a
  // rename in a changed file correctly drops a stale edge from an unchanged one.
  const parseCache = outDir ? loadParseCache(outDir) : emptyCache();
  const { generation, parseCache: nextParseCache } = buildGenerationFromSources(root, source, { ...options, parseCache, timingSink: sink });
  if (outDir) {
    measureStage(sink, "parse_cache_persistence", () => writeParseCache(outDir, nextParseCache));
    if (options.persist) {
      inheritSourceObservation(outDir, generation);
      finalizeGenerationIdentity(generation);
      measureStage(sink, "graph_persistence", () => writeGeneration(outDir, generation));
    }
  }
  return generation;
}

// A low-level persisted rebuild is sometimes the final pass after the CLI has
// already attested the same source tree. Keep that commit epoch only when the
// exact source hash agrees; never copy an observation across source drift.
function inheritSourceObservation(outDir, generation) {
  if (generation.sourceObservation !== undefined) return;
  const dbPath = join(outDir, "graph", "graph.db");
  if (!existsSync(dbPath)) return;
  let db;
  try {
    db = openStoreReadOnly(dbPath);
    const envelope = getGenerationEnvelope(db);
    if (envelope?.sourceObservation?.head
      && envelope.manifest?.repo?.sourceHash === generation.manifest?.repo?.sourceHash) {
      generation.sourceObservation = envelope.sourceObservation;
    }
  } catch {
    // No prior sealed envelope is equivalent to a first low-level build.
  } finally {
    if (db) closeStore(db);
  }
}

/**
 * Tree-sitter AST layer, unioned onto a lexical generation and re-persisted.
 *
 * Kept SEPARATE from buildGraphGeneration on purpose. Tree-sitter's WASM API is
 * async, but buildGraphGeneration has 30+ synchronous callers (including test
 * helpers that take sync callbacks). Making a widely-called leaf async to serve
 * one top-level caller forces `await` through the entire tree for no benefit —
 * the async belongs at the build boundary, which is the only place that needs it.
 *
 * Degrades to lexical-only on any failure: the lexical graph is a correct, if
 * shallower, result, and `blueprint build` must not become dependent on a WASM
 * grammar load succeeding. Opt out with BLUEPRINT_TREESITTER=0.
 */
export async function augmentGenerationWithTreeSitter(generation, repoRoot, options = {}) {
  if (options.treesitter === false || process.env.BLUEPRINT_TREESITTER === "0") {
    return { state: "disabled" };
  }
  const root = canonicalRoot(repoRoot);
  const sink = timingSink(options);
  try {
    const tsModule = await import("./treesitter-provider.mjs");
    const { augmentGeneration, PROVIDER: TS_PROVIDER, SUPPORTED_EXTENSIONS: TS_EXTENSIONS } = tsModule;
    const { summary } = await measureStageAsync(sink, "treesitter_augmentation", () => augmentGeneration(generation, root));
    // PROVIDER SELECTION. Tree-sitter cleared every qualification gate the
    // lexical incumbent has (12/12 tasks, 6/6 gates on darwin AND win32), so it
    // is the SELECTED provider — `manifest.provider` names it, and every
    // consumer reading that field now sees AST precision rather than LEXICAL.
    //
    // It is not the ONLY provider, and cannot be today: tree-sitter has
    // registered extractors for 10 extensions, while the lexical layer parses
    // 30. Dropping the lexical layer would blind Blueprint to Swift, C/C++,
    // shell, SQL, PowerShell, Vue and Astro — i.e. to every iOS app in the
    // suite and every Windows installer script. So the lexical layer remains,
    // demoted to fallback for exactly the extensions tree-sitter cannot parse.
    //
    // `lexicalProvider` preserves the identity of the persisted lexical nodes.
    // That, not the selected provider, is what decides whether an existing
    // the persisted lexical provider record can still be trusted — see
    // graphStatus's providerMismatch.
    if (summary?.parsedFiles > 0) {
      const astExtensions = [...TS_EXTENSIONS];
      const astSet = new Set(astExtensions);
      generation.manifest.lexicalProvider = generation.manifest.provider;
      generation.manifest.provider = TS_PROVIDER;
      generation.manifest.providerComposition = {
        selected: TS_PROVIDER.id,
        layers: [
          { id: TS_PROVIDER.id, precisionTier: TS_PROVIDER.precisionTier, role: "primary", extensions: astExtensions },
          {
            id: PROVIDER.id,
            precisionTier: PROVIDER.precisionTier,
            role: "fallback",
            extensions: PARSED_EXTENSIONS.filter((ext) => !astSet.has(ext)),
          },
        ],
      };
      generation.provider = TS_PROVIDER;
      // The manifest's counts were computed BEFORE this augmentation, so they
      // describe the lexical layer alone — on this workspace that understated
      // the graph by ~47% (33,487 edges recorded against 62,743 actually
      // stored). Downstream consumers pin these numbers, so a manifest that
      // undercounts its own generation is a lie with a version stamp on it.
      generation.manifest.counts = {
        ...generation.manifest.counts,
        nodes: generation.nodes.length,
        edges: generation.edges.length,
      };
    }
    return { state: "ok", summary };
  } catch (err) {
    const failure = { provider: "blueprint-treesitter", state: "unavailable", reason: String(err?.message ?? err) };
    generation.augmentation = { treesitter: failure };
    return failure;
  }
}

export function graphStatus(repoRoot, outDir, options = {}) {
  const root = canonicalRoot(repoRoot);
  // The store IS the manifest now. "missing" means build has never run here,
  // which is a normal first-use state answered by building — not by reaching
  // for a second format.
  const manifestPath = join(resolve(root, outDir), "graph", "graph.db");
  if (!existsSync(manifestPath)) return { state: "missing", manifestPath };
  // D51: a corrupt store must surface as a TYPED state, never an untyped
  // crash. "file is not a database" is the classic hostile-input signature
  // (a repo ships garbage bytes in .agent/graph/graph.db); the graph status
  // ladder (missing|corrupt|degraded|...) already has the right vocabulary.
  let store;
  try {
    store = openStoreReadOnly(manifestPath);
  } catch (error) {
    return { state: "corrupt", manifestPath, storeError: String(error?.message ?? error) };
  }
  let manifest;
  let envelope;
  let recordedHashes = new Map();
  let ledgerPresent = false;
  let clocks = null;
  try {
    if (!hasGeneration(store)) return { state: "missing", manifestPath };
    envelope = getGenerationEnvelope(store);
    manifest = envelope.manifest;
    if (!manifest?.complete) return { state: "incomplete", manifestPath, manifest };
    recordedHashes = loadFileContentHashes(store, manifest.generationId ?? null);
    ledgerPresent = Boolean(store.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='generation_leaf'").get())
      && store.prepare("SELECT 1 FROM generation_leaf WHERE kind='file' LIMIT 1").get() !== undefined;
    const hasWatchState = Boolean(store.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='watch_state'").get());
    const watchRows = hasWatchState ? store.prepare("SELECT key,value FROM watch_state").all() : [];
    if (watchRows.length) {
      const watchState = Object.fromEntries(watchRows.map((row) => [row.key, row.value]));
      clocks = {
        sourceClock: Number(watchState.source_clock ?? 0),
        appliedClock: Number(watchState.applied_clock ?? 0),
        eventGap: watchState.event_gap === "1",
      };
    }
  } catch (error) {
    // A store that opens but fails on its first query is corrupt (hostile or
    // truncated bytes). Typed state, not a throw.
    return { state: "corrupt", manifestPath, storeError: String(error?.message ?? error) };
  } finally {
    closeStore(store);
  }
  // Staleness detection re-walks the source tree with the same fileLimit
  // the build used (default unlimited). If the walk itself OOMs or hits the
  // directory cap, we trust the manifest and surface scanTruncated so the
  // caller knows the comparison was skipped rather than silently trusting
  // a stale comparison.
  const manifestFileLimit = Number(manifest.fileLimit ?? 0);
  const rescanLimit = options.fileLimit ?? manifestFileLimit ?? 0;
  const sources = scanSources(root, rescanLimit, options);
  const scanned = sources.files.length > 0;
  const scanTruncated = Boolean(sources.traversalTruncated);
  const currentHash = scanned && !scanTruncated ? sourceHash(sources.files) : manifest.repo?.sourceHash;
  let ledgerDiff = null;
  if (ledgerPresent && scanned && !scanTruncated) {
    const ledger = openStoreReadOnly(manifestPath);
    try { ledgerDiff = diffLedgerAgainstTree(ledger, null, sources.files); } finally { closeStore(ledger); }
  }
  let coverage = { unsupportedExtensions: [], unsupportedFileCount: 0 };
  let dirtyOverlayFileCount = 0;
  if (scanned) {
    coverage = sourceCapabilityCoverage(sources.files);
    for (const file of sources.files) {
      const recorded = recordedHashes.get(file.path);
      if (recorded && recorded !== file.contentHash) {
        dirtyOverlayFileCount += 1;
      }
    }
  }
  // A promoted graph carries TWO layers, and a version bump in either one must
  // force a rebuild — otherwise a fixed extractor leaves every existing graph
  // silently stale. Pre-promotion manifests have no `lexicalProvider`, and their
  // `provider` IS the lexical record, so the fallback evaluates them exactly as
  // before.
  const lexical = manifest.lexicalProvider ?? manifest.provider;
  const lexicalMismatch = lexical?.id !== PROVIDER.id || lexical?.version !== PROVIDER.version;
  const astMismatch = manifest.lexicalProvider
    ? manifest.provider?.id !== TREESITTER_PROVIDER.id || manifest.provider?.version !== TREESITTER_PROVIDER.version
    : false;
  const providerMismatch = lexicalMismatch || astMismatch;
  const manifestDigestValid = !manifest.manifestDigest
    || manifest.manifestDigest === computeManifestDigest(manifest, envelope?.sourceObservation);
  const fresh = !providerMismatch && (ledgerDiff
    ? ledgerDiff.changed.length === 0 && ledgerDiff.added.length === 0 && ledgerDiff.removed.length === 0
    : scanned && !scanTruncated ? manifest.repo?.sourceHash === currentHash : true)
    && manifestDigestValid;
  return {
    state: scanTruncated ? "indeterminate" : fresh ? "fresh" : "stale",
    manifestPath,
    manifest,
    providerMismatch,
    manifestDigestValid,
    ledger: ledgerPresent,
    pendingPaths: ledgerDiff ? [...ledgerDiff.changed, ...ledgerDiff.added, ...ledgerDiff.removed].sort() : [],
    scanTruncated,
    truncationReasons: sources.truncationReasons,
    clocks,
    capabilities: {
      parsedExtensions: PARSED_EXTENSIONS,
      opaqueFileExtensions: [...OPAQUE_FILE_EXTENSIONS].sort(),
      unsupportedExtensions: coverage.unsupportedExtensions,
      unsupportedFileCount: coverage.unsupportedFileCount,
      dirtyOverlayFileCount,
    },
  };
}

/**
 * Read the persisted generation from the one store.
 *
 * Returns null when no graph has been built here, so callers distinguish "build
 * one" from "the graph is empty". There is deliberately no JSON path: if the
 * database is absent the answer is `blueprint build`, which is cheap relative to
 * carrying a second serialization format forever.
 */
export function readGeneration(repoRoot, outDir) {
  const dbPath = join(canonicalRoot(repoRoot), outDir, "graph", "graph.db");
  if (!existsSync(dbPath)) return null;
  const db = openStoreReadOnly(dbPath);
  try {
    return loadGeneration(db);
  } finally {
    closeStore(db);
  }
}

export function graphCapabilities(repoRoot, options = {}) {
  return {
    schemaVersion: 1,
    provider: PROVIDER,
    // blueprint B4 — the highest precision tier THIS provider can achieve.
    // AST/COMPILER tiers, when available, come from separate providers
    // (graph/treesitter-provider.mjs, graph/scip-provider.mjs) that augment
    // the same generation; see graphPrecisionProbe() for the combined view.
    precisionTier: PROVIDER.precisionTier,
    precisionTiers: {
      order: PRECISION_TIER_ORDER,
      current: PROVIDER.precisionTier,
    },
    // blueprint B3 — the confidence-tier vocabulary every edge is tagged
    // with. Exported so consumers can filter by minimum tier.
    edgeConfidenceTiers: {
      order: EDGE_CONFIDENCE_TIER_ORDER,
      descriptions: EDGE_CONFIDENCE_TIER_DESCRIPTIONS,
    },
    languageCoverage: {
      parsedExtensions: PARSED_EXTENSIONS,
      opaqueFileExtensions: [...OPAQUE_FILE_EXTENSIONS].sort(),
      parserMode: "registry-backed-lexical-ast-v1",
      fallback: "Text/config and known opaque assets are represented as file nodes; unsupported source languages do not get symbol/call extraction.",
      gitIgnoreMode: "tracked-plus-unignored",
      ignoredDirectories: [...IGNORED].sort(),
      maxFileBytes: 2 * 1024 * 1024,
    },
    outputs: ["graph", "flows", "docTruth", "mermaid", "ContextCandidateSet"],
    // Optional: when a repoRoot is supplied, fold in the live SCIP-tier probe
    // so the capabilities/probe surface reports precision-tier availability
    // end to end, not just this provider's own ceiling.
    scip: repoRoot ? probeScip(repoRoot, options) : undefined,
  };
}

// blueprint B4 — combined precision-tier probe across all three providers.
// Honest by construction: LEXICAL and AST are always "ok" (static-provider
// always runs; treesitter-provider's WASM grammars ship in the workspace and
// its own augment step reports its own degrade separately), COMPILER reflects
// whatever probeScip() actually finds on disk — never assumed available.
export function graphPrecisionProbe(repoRoot, options = {}) {
  const scip = probeScip(repoRoot, options);
  return {
    schemaVersion: 1,
    tiers: [
      { tier: PRECISION_TIERS.LEXICAL, provider: PROVIDER.id, state: "ok" },
      { tier: PRECISION_TIERS.AST, provider: "blueprint-treesitter", state: "ok" },
      {
        tier: PRECISION_TIERS.COMPILER,
        provider: scip.provider.id,
        state: scip.state,
        reason: scip.reason ?? null,
        degradesTo: scip.degradesTo ?? null,
      },
    ],
    highestAvailable: scip.state === "ok" ? PRECISION_TIERS.COMPILER : PRECISION_TIERS.AST,
  };
}

export function queryGraph(generation, options = {}) {
  const query = String(options.query ?? "").toLowerCase();
  const terms = queryTerms(query);
  const limit = Number(options.limit ?? 20);
  const nodes = generation.nodes
    .map((node) => ({ node, score: scoreNode(node, terms) }))
    .filter((item) => item.score > 0)
    .sort((left, right) => right.score - left.score || left.node.id.localeCompare(right.node.id))
    .slice(0, limit);
  return nodes.map(({ node }, index) => ({
    id: node.id,
    kind: node.kind,
    name: node.name,
    qualifiedName: node.qualifiedName,
    provider: generation.provider.id,
    confidence: node.confidence ?? 1,
    evidence: node.evidence,
    fresh: evidenceFresh(generation, node.evidence),
    rank: index + 1,
  }));
}

// Compatibility candidate adapter for callers that still hold an in-memory
// generation rather than using the resident service. Production service, CLI,
// and Membrane federation all use RecallCircuit. This adapter preserves the V1
// candidate protocol only; it does not define Blueprint's retrieval semantics.
// Build a ContextCandidateSet with real tiering, anchor reservation, and omission
// receipts. The previous version hardcoded protected:false, exact:true, and
// omissions:[] for every candidate — a schema-valid shape with no admission logic
// behind it. Honest tiers, in priority order:
//   protected — a file the caller explicitly anchored (options.anchors); reserved
//   exact     — a symbol whose name equals a query term
//   lexical   — a substring match (queryGraph's scoring)
// There is deliberately NO structural or semantic tier: queryGraph is lexical, and
// no embedding/graph-walk retrieval channel exists yet (B4-semantic), so labelling
// candidates "structural" would be a hollow claim. That gap is tracked in the
// inventory doc, not papered over here.
export function createContextCandidateSet(generation, options = {}) {
  const maxCandidates = Number(options.maxCandidates ?? 40);
  const maxEstimatedTokens = Number(options.maxEstimatedTokens ?? 8000);
  const anchors = new Set((options.anchors ?? []).map((path) => normalizePath(String(path))));
  const query = options.query ?? options.task ?? "";
  const terms = queryTerms(String(query).toLowerCase());
  // Retrieve a wider pool than the ceiling so overflow becomes real omission
  // receipts rather than a silently truncated list.
  const pool = queryGraph(generation, { query, limit: Math.max(maxCandidates * 3, maxCandidates + 20) });

  // Anchor reservation: an explicitly anchored file must be a candidate whether or
  // not it matched the query — that is what "anchor" means. Inject the anchored
  // files' nodes that the query pool missed, shaped like query results.
  if (anchors.size > 0) {
    const pooledIds = new Set(pool.map((result) => result.id));
    let syntheticRank = pool.length;
    for (const node of generation.nodes) {
      const nodePath = normalizePath(node.evidence?.[0]?.path ?? node.path ?? "");
      if (!anchors.has(nodePath) || pooledIds.has(node.id)) continue;
      if (!node.evidence?.[0]) continue;
      pooledIds.add(node.id);
      pool.push({
        id: node.id,
        kind: node.kind,
        name: node.name,
        qualifiedName: node.qualifiedName,
        provider: generation.provider.id,
        confidence: node.confidence ?? 1,
        evidence: node.evidence,
        fresh: evidenceFresh(generation, node.evidence),
        rank: ++syntheticRank,
      });
    }
  }

  const tierRank = { protected: 0, exact: 1, lexical: 2 };
  const classified = pool.map((result) => {
    const path = normalizePath(result.evidence[0]?.path ?? "");
    const name = String(result.qualifiedName ?? result.name ?? "").toLowerCase();
    const shortName = name.split(".").at(-1);
    const isAnchor = anchors.has(path);
    const isExact = terms.length > 0 && terms.some((term) => name === term || shortName === term);
    const tier = isAnchor ? "protected" : isExact ? "exact" : "lexical";
    return { result, isAnchor, isExact, tier };
  }).sort((a, b) => {
    const byTier = tierRank[a.tier] - tierRank[b.tier];
    if (byTier !== 0) return byTier;
    // Higher providerScore (lower rank) first; stable tiebreak on id.
    return a.result.rank - b.result.rank || a.result.id.localeCompare(b.result.id);
  });

  const candidates = [];
  const omissions = [];
  let tokenBudget = maxEstimatedTokens;
  for (const item of classified) {
    const ev = item.result.evidence[0];
    // Phase 7.4 — slice-time estimate derived from the shared helper (a pure
    // metadata computation, no file read). It is paired with `actualTokens`
    // = null here: the set is produced without reading source, so the
    // resolution-time budget is intentionally unset until a resolver
    // consumes this candidate and reads its bytes.
    const estimatedTokens = sliceTokens(ev);
    // Hard ceilings apply uniformly — including to anchors, so the set can never
    // exceed the canonical provider ceiling. Anchors win their slots by sorting
    // first, not by bypassing the cap; an anchor dropped past the cap is receipted.
    if (candidates.length >= maxCandidates) {
      omissions.push({ id: item.result.id, layer: 3, reason: item.isAnchor ? "anchor_over_candidate_ceiling" : "over_candidate_ceiling" });
      continue;
    }
    if (estimatedTokens > tokenBudget) {
      omissions.push({ id: item.result.id, layer: 3, reason: item.isAnchor ? "anchor_over_token_budget" : "over_token_budget" });
      continue;
    }
    tokenBudget -= estimatedTokens;
    const providerScore = Math.max(0, Math.min(1, 1 / item.result.rank));
    candidates.push({
      id: item.result.id,
      layer: 3,
      sourceKind: "repo_code",
      sourceRef: `${ev.path}:${ev.startLine}-${ev.endLine}`,
      sourceHash: `xxh128:${ev.contentHash}`,
      trustClass: "workspace_tracked",
      instructionPolicy: "data_only",
      providerScore,
      scoreComponents: item.isExact ? { exact: 1 } : { lexical: providerScore },
      // Phase 7.4 — two-budget tokens. `estimatedTokens` is the slice-time
      // metadata estimate (cheap, no file read); `actualTokens` is filled in
      // by a resolver that reads the source, paired with a `truncation`
      // receipt when bytes are dropped. Both null at slice time is a
      // contract — not a bug — and lets downstream consumers distinguish
      // "admitted but never resolved" from "resolved and trimmed".
      estimatedTokens,
      actualTokens: null,
      truncation: null,
      protected: item.isAnchor,
      exact: item.isExact,
      recoverable: true,
      resolver: `blueprint graph resolve --node ${item.result.id}`,
      text: item.result.qualifiedName || item.result.name,
    });
  }

  return {
    schemaVersion: 1,
    repoId: options.repoId ?? (generation.repoRoot ? repositoryIdentity(generation.repoRoot).repoId : null),
    repoRoot: options.repoRoot ?? generation.repoRoot ?? null,
    receiptId: options.receiptId ?? null,
    traceId: options.traceId ?? randomUUID(),
    task: String(options.task ?? options.query ?? "Blueprint graph retrieval"),
    mode: options.mode ?? "survey",
    provider: generation.provider.id,
    freshness: {
      revision: generation.manifest.generationId,
      manifestDigest: generation.manifest.manifestDigest ?? null,
      indexedAt: generation.manifest.generatedAt,
      stale: false,
    },
    providerCeiling: { maxCandidates, maxEstimatedTokens },
    candidates,
    omissions,
  };
}

// Normalize a git remote URL into host/owner/repo so the same repository
// produces the same identity regardless of where it is checked out. Accepts:
//   https://github.com/owner/repo(.git)
//   git@github.com:owner/repo(.git)
//   ssh://git@github.com/owner/repo(.git)
//   git://github.com/owner/repo(.git)
//   /local/path (treated as no remote — caller falls back to a local UUID).
export function normalizeGitOrigin(rawOrigin) {
  const origin = String(rawOrigin ?? "").trim();
  if (!origin) return null;
  const sshShort = origin.match(/^[a-zA-Z0-9_.-]+@([^:]+):(.+?)(?:\.git)?$/);
  if (sshShort) return { host: sshShort[1], owner: sshShort[2].split("/").slice(0, -1).join("/") || sshShort[2].split("/")[0], repo: sshShort[2].split("/").at(-1) };
  const cleaned = origin.replace(/^git\+/, "").replace(/\.git$/, "");
  try {
    const url = new URL(cleaned);
    const parts = url.pathname.replace(/^\//, "").split("/").filter(Boolean);
    if (parts.length < 2) return null;
    return { host: url.host, owner: parts.slice(0, -1).join("/"), repo: parts.at(-1) };
  } catch {
    return null;
  }
}

export function repositoryIdentity(repoRoot, options = {}) {
  const root = canonicalRoot(repoRoot).replaceAll("\\", "/");
  const installationId = options.installationId ?? process.env.BLUEPRINT_INSTALLATION_ID ?? null;
  const gitOrigin = spawnSync("git", ["-C", root, "config", "--get", "remote.origin.url"], { encoding: "utf8" });
  const origin = gitOrigin.status === 0 ? gitOrigin.stdout.trim() : "";
  const parsed = origin ? normalizeGitOrigin(origin) : null;
  // Phase 7.5 — repoId is now derived from normalized host/owner/repo, with
  // a local UUIDv5 fallback when no remote is configured. Path-dependent
  // identity (the old "xxh128(absolute_root + origin)") made the same repo
  // a different identity at every checkout path; consumers downstream
  // (federation cache, scope grants, orientation receipts) now treat the
  // GitHub repo "foo/bar" the same regardless of clone location. Keep
  // `repoRoot` as a SEPARATE field — it still describes WHERE the operation
  // is anchored, even though it no longer hashes into the id.
  let repoId;
  if (parsed) {
    const components = [parsed.host, parsed.owner, parsed.repo].map((part) => String(part).toLowerCase()).join("/");
    repoId = `xxh128:${xxh128(`host-owner-repo\n${components}`)}`;
  } else {
    // No remote — synthesize a stable local identity from the canonical root
    // and a process-stable UUID. The UUID is generated once per process
    // installation so a single checkout keeps a stable identity across
    // rebuilds; a fresh clone produces a fresh identity (which is the only
    // honest answer for a repo with no remote).
    const localKey = process.env.BLUEPRINT_LOCAL_REPO_ID ?? installationId ?? `local:${xxh128(root)}`;
    repoId = `xxh128:${xxh128(`local\n${localKey}`)}`;
  }
  return {
    repoId,
    repoRoot: root,
    originUrl: origin || null,
    originHost: parsed?.host ?? null,
    originOwner: parsed?.owner ?? null,
    originRepo: parsed?.repo ?? null,
    installationId,
    // Legacy identity (path-derived): retained so callers that compared
    // against an older receipt still resolve. Marked deprecated; new code
    // MUST read `repoId` and `repoRoot` independently.
    legacyRepoId: `xxh128:${xxh128(`${root}\n${origin}`)}`,
  };
}

export function resolveGraphNode(generation, nodeId) {
  const node = generation.nodes.find((item) => item.id === nodeId);
  if (!node) return null;
  return {
    schemaVersion: 1,
    provider: generation.provider.id,
    node: {
      ...node,
      fresh: evidenceFresh(generation, node.evidence),
    },
  };
}

export function graphNeighbors(generation, options = {}) {
  const nodeId = String(options.nodeId ?? "");
  const direction = options.direction ?? "both";
  const maxDepth = Math.max(1, Number(options.depth ?? 1));
  const seenNodes = new Set([nodeId]);
  const seenEdges = new Set();
  let frontier = new Set([nodeId]);
  for (let depth = 0; depth < maxDepth; depth += 1) {
    const next = new Set();
    for (const edge of generation.edges) {
      const out = direction !== "in" && frontier.has(edge.source);
      const incoming = direction !== "out" && frontier.has(edge.target);
      if (!out && !incoming) continue;
      seenEdges.add(edge.id);
      const other = out ? edge.target : edge.source;
      if (!seenNodes.has(other)) next.add(other);
      seenNodes.add(other);
    }
    frontier = next;
    if (!frontier.size) break;
  }
  return {
    schemaVersion: 1,
    provider: generation.provider.id,
    root: nodeId,
    nodes: generation.nodes.filter((node) => seenNodes.has(node.id)),
    edges: generation.edges.filter((edge) => seenEdges.has(edge.id)),
    truncated: false,
  };
}

export function graphPath(generation, options = {}) {
  const from = String(options.from ?? "");
  const to = String(options.to ?? "");
  const maxDepth = Math.max(1, Number(options.maxDepth ?? 5));
  const queue = [[from]];
  const visited = new Set([from]);
  while (queue.length) {
    const path = queue.shift();
    const current = path.at(-1);
    if (current === to) {
      return pathPayload(generation, path);
    }
    if (path.length > maxDepth) continue;
    for (const edge of generation.edges.filter((item) => item.source === current)) {
      if (visited.has(edge.target)) continue;
      visited.add(edge.target);
      queue.push([...path, edge.target]);
    }
  }
  return { schemaVersion: 1, provider: generation.provider.id, from, to, path: [], edges: [], found: false };
}

export function graphArchitecture(generation) {
  const nodes = generation.nodes;
  const incoming = new Set(generation.edges.map((edge) => edge.target));
  const outgoing = new Set(generation.edges.map((edge) => edge.source));
  const entryPoints = nodes.filter((node) => node.kind === "symbol" && outgoing.has(node.id) && !incoming.has(node.id));
  const terminals = nodes.filter((node) => incoming.has(node.id) && !outgoing.has(node.id));
  return {
    schemaVersion: 1,
    provider: generation.provider.id,
    summary: {
      files: nodes.filter((node) => node.kind === "file").length,
      symbols: nodes.filter((node) => node.kind === "symbol").length,
      edges: generation.edges.length,
    },
    entryPoints,
    terminals,
    edgeKinds: [...new Set(generation.edges.map((edge) => edge.kind))].sort(),
  };
}

export function graphImpact(generation, options = {}) {
  const nodeId = String(options.nodeId ?? "");
  const depth = Math.max(1, Number(options.depth ?? 3));
  const neighborhood = graphNeighbors(generation, { nodeId, direction: "in", depth });
  return {
    schemaVersion: 1,
    provider: generation.provider.id,
    target: generation.nodes.find((node) => node.id === nodeId) ?? { id: nodeId },
    direction: "upstream",
    impacted: neighborhood.nodes.filter((node) => node.id !== nodeId),
    edges: neighborhood.edges,
    truncated: neighborhood.truncated,
  };
}

export function graphMermaid(generation, options = {}) {
  const view = String(options.view ?? "architecture");
  const limit = Math.max(1, Number(options.limit ?? 60));
  let payload;
  if (view === "neighbors") {
    payload = graphNeighbors(generation, {
      nodeId: String(options.nodeId ?? ""),
      direction: options.direction ?? "both",
      depth: Number(options.depth ?? 1),
    });
  } else if (view === "path") {
    payload = graphPath(generation, {
      from: options.from,
      to: options.to,
      maxDepth: Number(options.maxDepth ?? 5),
    });
  } else if (view === "impact") {
    payload = graphImpact(generation, {
      nodeId: String(options.nodeId ?? ""),
      depth: Number(options.depth ?? 3),
    });
  } else {
    payload = {
      nodes: generation.nodes,
      edges: generation.edges,
      truncated: generation.nodes.length > limit,
    };
  }
  return renderMermaid(generation, payload, { view, limit });
}

function renderMermaid(generation, payload, options) {
  const limit = options.limit;
  const nodes = selectMermaidNodes(payload, limit);
  const nodeIds = new Set(nodes.map((node) => node.id));
  const edges = (payload.edges ?? generation.edges)
    .filter((edge) => nodeIds.has(edge.source) && nodeIds.has(edge.target))
    .sort((left, right) => `${left.source}:${left.target}:${left.kind}`.localeCompare(`${right.source}:${right.target}:${right.kind}`))
    .slice(0, Math.max(0, limit * 2));
  const aliases = new Map(nodes.map((node, index) => [node.id, `n${index}`]));
  const truncated = Boolean(payload.truncated) || nodes.length < (payload.nodes ?? generation.nodes).length || edges.length < (payload.edges ?? generation.edges).length;
  const lines = [
    "flowchart LR",
    `%% provider: ${generation.provider.id}`,
    `%% view: ${options.view}`,
    `%% truncated: ${truncated}`,
  ];
  for (const node of nodes) lines.push(`  ${aliases.get(node.id)}["${escapeMermaidLabel(nodeLabel(node))}"]`);
  for (const edge of edges) lines.push(`  ${aliases.get(edge.source)} -->|"${escapeMermaidLabel(edge.kind)}"| ${aliases.get(edge.target)}`);
  return `${lines.join("\n")}\n`;
}

function selectMermaidNodes(payload, limit) {
  const byId = new Map((payload.nodes ?? []).map((node) => [node.id, node]));
  const edgeNodes = [];
  for (const edge of payload.edges ?? []) {
    if (byId.has(edge.source)) edgeNodes.push(byId.get(edge.source));
    if (byId.has(edge.target)) edgeNodes.push(byId.get(edge.target));
  }
  const selected = dedupeBy([...edgeNodes, ...(payload.nodes ?? [])], (node) => node.id)
    .sort((left, right) => left.id.localeCompare(right.id))
    .slice(0, limit);
  return selected;
}

function nodeLabel(node) {
  const label = node.qualifiedName || node.name || node.path || node.id;
  return `${node.kind}:${String(label).slice(0, 80)}`;
}

function escapeMermaidLabel(value) {
  return String(value).replaceAll("\\", "/").replaceAll("\"", "'").replaceAll("\n", " ");
}

// Doc ↔ code join — emits deterministic edges joining doc/claim nodes (from
// `map.json`) to file/symbol nodes (from this generation). The join is reversible
// (every edge carries doc+file evidence + content hashes) and typed as
// supports / contradicts / supersedes. A claim with NO matching code node does
// not emit a join — it remains a doc-side finding, not a graph contradiction.
export function buildDocCodeJoins(generation, options = {}) {
  const repoRoot = generation?.repoRoot ? resolve(generation.repoRoot) : null;
  const map = readDocMap(repoRoot, options);
  if (!map) return { schemaVersion: 1, provider: generation.provider.id, joins: [], supersedes: [], truncated: false, sourceDocMap: { docs: 0, claims: 0, generatedAt: null, docPaths: [], claimPaths: [] } };
  const nodesById = new Map(generation.nodes.map((node) => [node.id, node]));
  const docById = new Map(map.nodes.filter((node) => node.kind === "doc").map((doc) => [doc.id, doc]));
  const claimById = new Map(map.nodes.filter((node) => node.kind === "claim").map((claim) => [claim.id, claim]));
  const claimsByDoc = new Map();
  for (const edge of map.edges) {
    if (edge.type === "contains" && claimById.has(edge.to)) {
      if (!claimsByDoc.has(edge.from)) claimsByDoc.set(edge.from, []);
      claimsByDoc.get(edge.from).push(edge.to);
    }
  }
  const codeRefByDoc = new Map();
  for (const edge of map.edges) {
    if (edge.type !== "mentions-code") continue;
    if (!codeRefByDoc.has(edge.from)) codeRefByDoc.set(edge.from, []);
    codeRefByDoc.get(edge.from).push(edge.to);
  }
  const codeRefsById = new Map(map.nodes.filter((node) => node.kind === "code_ref").map((node) => [node.id, node]));
  const joins = [];
  for (const [docId, claimIds] of claimsByDoc) {
    const doc = docById.get(docId);
    const codeRefIds = codeRefByDoc.get(docId) ?? [];
    for (const claimId of claimIds) {
      const claim = claimById.get(claimId);
      if (!doc || !claim) continue;
      for (const codeRefId of codeRefIds) {
        const codeRef = codeRefsById.get(codeRefId);
        if (!codeRef?.path) continue;
        const codeNode = nodesById.get(`file:${codeRef.path}`) ?? generation.nodes.find((node) => node.kind === "symbol" && node.path === codeRef.path);
        if (!codeNode) continue;
        const join = classifyJoin(claim, doc, codeRef, codeNode);
        if (join) joins.push(join);
      }
    }
  }
  const supersedes = buildSupersedesChain(map, joins);
  return {
    schemaVersion: 1,
    provider: generation.provider.id,
    joins,
    supersedes,
    truncated: false,
    sourceDocMap: {
      docs: docById.size,
      claims: claimById.size,
      generatedAt: map.generatedAt,
      // Phase 7.3 — the relational store needs every doc path (not just the
      // ones connected to joins) so `listDocuments` matches the original
      // `sourceDocMap.docs` count. Joining-onlies would undercount supersedable
      // docs with no claims, breaking `graph-query.test.mjs`'s assertion.
      docPaths: [...docById.values()].map((doc) => doc.path),
      claimPaths: [...claimById.values()].map((claim) => ({ path: claim.source, line: claim.line })),
    },
  };
}

function classifyJoin(claim, doc, codeRef, codeNode) {
  const status = claim.status;
  const text = stripInlineCode(claim.text);
  const isStale = status === "stale" || /\b(stale|contradict|drift|missing|not implemented|not built|not shipped|deprecated)\b/i.test(text);
  const isImplemented = status === "implemented" && !isStale;
  const isSupersedes = /\b(supersedes|superseded by|replaced by)\b/i.test(text);
  const baseEvidence = {
    docRef: { path: doc.path, line: claim.line ?? null, sha1: doc.sha1 ?? null },
    codeRef: { path: codeRef.path, exists: codeRef.exists !== false },
    codeNode: { id: codeNode.id, path: codeNode.path, contentHash: codeNode.evidence?.[0]?.contentHash ?? null },
  };
  if (isSupersedes) {
    return {
      kind: "supersedes",
      source: codeNode.id,
      target: `doc:${doc.path}`,
      confidence: 0.9,
      confidenceClass: "INFERRED",
      reason: "claim references supersedes/replaced",
      evidence: baseEvidence,
    };
  }
  if (isStale) {
    return {
      kind: "contradicts",
      source: `doc:${doc.path}`,
      target: codeNode.id,
      confidence: 0.85,
      confidenceClass: "EXTRACTED",
      reason: `claim status=${status || "claim"} mentions stale/drift/missing/contradict; code node exists`,
      evidence: baseEvidence,
    };
  }
  if (isImplemented) {
    return {
      kind: "supports",
      source: `doc:${doc.path}`,
      target: codeNode.id,
      confidence: 0.85,
      confidenceClass: "EXTRACTED",
      reason: `claim status=implemented; code node exists`,
      evidence: baseEvidence,
    };
  }
  return null;
}

function buildSupersedesChain(map, joins) {
  const lifecycleDocs = map.nodes.filter(
    (node) => node.kind === "doc" && node.lifecycle?.status === "superseded",
  );
  const out = lifecycleDocs.map((doc) => ({
    kind: "supersedes",
    source: doc.lifecycle.supersededBy ? { doc: doc.lifecycle.supersededBy } : { external: true },
    target: { doc: doc.path, supersededOn: doc.lifecycle.supersededOn ?? null },
    evidence: { sourceDoc: doc.path, targetMatch: doc.lifecycle.supersededBy ?? null },
  }));
  const supersedeClaims = map.nodes.filter(
    (node) => node.kind === "claim" && /\b(supersedes|replaced by|deprecated by)\b/i.test(stripInlineCode(node.text ?? "")),
  );
  for (const claim of supersedeClaims) {
    const doc = map.nodes.find((node) => node.kind === "doc" && node.id && map.edges.some((e) => e.type === "contains" && e.from === node.id && e.to === claim.id));
    if (!doc) continue;
    const target = extractSupersedeTarget(claim.text);
    if (!target) continue;
    out.push({
      kind: "supersedes",
      source: { doc: doc.path, line: claim.line, text: claim.text.slice(0, 240) },
      target,
      evidence: { sourceDoc: doc.path, targetMatch: target },
    });
  }
  return out;
}

function extractSupersedeTarget(text) {
  const match = text.match(/(?:supersedes|replaced by|deprecated by)\s+([^\s.]+(?:\.[^\s.]+)*)/i);
  return match ? match[1] : null;
}

function readDocMap(repoRoot, options) {
  const explicit = options?.docMap ?? null;
  if (explicit) return explicit;
  if (!repoRoot) return null;
  const candidates = [
    join(repoRoot, options?.outDir ?? ".agent", "map.json"),
    join(repoRoot, ".agent", "map.json"),
  ];
  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      try {
        return JSON.parse(readFileSync(candidate, "utf8"));
      } catch {
        return null;
      }
    }
  }
  return null;
}

function stripInlineCode(text) {
  return String(text ?? "").replace(/`[^`]*`/g, "");
}

export function graphFlowInventory(generation, options = {}) {
  const complete = Boolean(options.complete);
  const maxFlows = Number(options.maxFlows ?? (complete ? 5000 : 200));
  const outgoing = new Set(generation.edges.map((edge) => edge.source));
  const incoming = new Set(generation.edges.map((edge) => edge.target));
  const entryPoints = generation.nodes.filter((node) => node.kind === "symbol" && outgoing.has(node.id) && !incoming.has(node.id));
  // NOTE: the spec's third flow status, "unsupported", is intentionally NOT emitted
  // here. It is only honest once we can DETECT a flow crossing into untraceable
  // territory — an HTTP route, a Tauri IPC boundary, or an unparsed-language hop —
  // and those stack-specific boundary joins are not implemented yet (B3 partial).
  // With only structural CALLS/IMPORTS/CONFIGURES edges, any reachable node is a
  // terminal, so a flow is either complete or a genuine dangling "broken". Emitting
  // an "unsupported" bucket that can never populate would be a false spec claim.
  const flows = [];
  let truncated = false;
  for (const entry of entryPoints) {
    const terminalPaths = terminalPathsFrom(generation, entry.id, maxFlows - flows.length);
    if (!terminalPaths.length) {
      flows.push({ entry, status: "broken", path: [entry], missingHop: "no terminal reachable", evidence: entry.evidence });
      continue;
    }
    for (const path of terminalPaths) {
      flows.push({
        id: `flow:${entry.id}->${path.at(-1).id}`,
        entry,
        terminal: path.at(-1),
        status: "complete",
        path,
        evidence: path.flatMap((node) => node.evidence ?? []),
      });
      if (flows.length >= maxFlows) {
        truncated = true;
        break;
      }
    }
    if (truncated) break;
  }
  return {
    schemaVersion: 1,
    provider: generation.provider.id,
    generatedAt: generation.manifest?.generatedAt ?? `gen:${generation.manifest?.generationId?.replace(/^xxh128:/, "").slice(0, 16) ?? "0"}`,
    mode: complete ? "complete" : "bounded",
    maxFlows,
    entryPoints: entryPoints.length,
    flows,
    truncated,
    truncationReason: truncated ? `flow inventory hit maxFlows=${maxFlows}` : null,
  };
}

function terminalPathsFrom(generation, entryId, limit = 200) {
  const paths = [];
  const queue = [[entryId]];
  while (queue.length && paths.length < limit) {
    const ids = queue.shift();
    const current = ids.at(-1);
    const outgoing = generation.edges.filter((edge) => edge.source === current);
    if (!outgoing.length && ids.length > 1) {
      paths.push(ids.map((id) => generation.nodes.find((node) => node.id === id)).filter(Boolean));
      continue;
    }
    for (const edge of outgoing) {
      if (ids.includes(edge.target) || ids.length > 12) continue;
      queue.push([...ids, edge.target]);
    }
  }
  return paths;
}

function pathPayload(generation, nodeIds) {
  const edgeSteps = [];
  for (let index = 0; index < nodeIds.length - 1; index += 1) {
    edgeSteps.push(generation.edges.find((edge) => edge.source === nodeIds[index] && edge.target === nodeIds[index + 1]));
  }
  return {
    schemaVersion: 1,
    provider: generation.provider.id,
    found: true,
    path: nodeIds.map((id) => generation.nodes.find((node) => node.id === id)).filter(Boolean),
    edges: edgeSteps.filter(Boolean),
  };
}

// The generation is a SINGLE consolidated store at a STABLE path, `graph/graph.db`.
//
// The prior scheme wrote a content-addressed directory `generations/<hash>/` per
// build and pruned the rest. That was designed for a multi-generation cache that no
// longer exists (we keep exactly one), and its per-build hash-named directory was
// un-deltable in git — committing the graph would add the whole generation on every
// change instead of a small textual diff. A single stable file both fixes that and
// removes the dir/marker/prune machinery.
//
// The old JSON path used temp-then-rename atomicity. The SQLite path now relies on
// the store transaction: a reader sees the prior complete generation or the new
// complete generation, never a torn mixture. COMPLETE markers and content-addressed
// dirs are no longer needed to avoid torn reads.
// ONE store: graph.db. There is no graph.json and no fallback to one.
//
// graph.json was the original store and SQLite arrived beside it, so for one day
// the repo carried two artifacts holding the same data — one read, one written.
// Worse, graph.json was TRACKED IN GIT: 92 MB, six commits, roughly half the
// 589 MB .git directory, and the reason `build` refused to run on a dirty tree
// ("commit or restore them before rebuilding the committed graph"). A derived
// artifact was being versioned, which made the graph structurally stale exactly
// during active development and forced an overlay subsystem to compensate.
//
// The database is a local index. It is gitignored, it is rebuilt by `blueprint
// build`, and if it is absent or stale the answer is to build — never to fall
// back to a second format. SQLite's transaction gives atomicity directly, so the
// write-body-then-manifest ordering the JSON path needed is gone with it.
export { finalizeGenerationIdentity } from "./generation-identity.mjs";

function writeGeneration(outDir, generation) {
  const graphDir = join(outDir, "graph");
  mkdirSync(graphDir, { recursive: true });
  const dbPath = join(graphDir, "graph.db");
  // D51: a corrupt store ahead of a persist is typed, recoverable state. The
  // store is the BUILD's output and this write replaces it wholesale, so a
  // hostile/truncated prior store is removed (with its WAL/SHM sidecars)
  // rather than crashing the indexer. Non-corruption open failures still
  // surface as a typed error.
  if (existsSync(dbPath)) {
    try {
      const probe = openStoreReadOnly(dbPath);
      try {
        // A corrupt SQLite file can OPEN read-only without error; only the
        // first page read trips it. Probe with a trivial query so hostile
        // bytes are actually detected, not just opened.
        probe.prepare("SELECT count(*) AS n FROM sqlite_master").get();
      } finally {
        closeStore(probe);
      }
    } catch (error) {
      const message = String(error?.message ?? error);
      if (/not a database|file is encrypted|malformed/i.test(message)) {
        for (const suffix of ["", "-wal", "-shm"]) {
          const sidecar = dbPath + suffix;
          if (existsSync(sidecar)) rmSync(sidecar, { force: true });
        }
      } else {
        throw new Error(`graph_publication_failed: ${message}`);
      }
    }
  }
  const db = openStore(dbPath);
  try {
    saveGeneration(db, generation, { populateState: true });
    const persisted = loadGeneration(db);
    if (!persisted
      || persisted.manifest?.generationId !== generation.manifest?.generationId
      || persisted.manifest?.manifestDigest !== generation.manifest?.manifestDigest) {
      throw new Error("graph publication readback identity mismatch");
    }
  } finally {
    closeStore(db);
  }
  // Drop artifacts from the pre-SQLite layout so a rebuilt repo cannot keep
  // serving a stale JSON graph that nothing writes any more.
  rmSync(join(graphDir, "generations"), { recursive: true, force: true });
  rmSync(join(graphDir, "graph.json"), { force: true });
  rmSync(join(graphDir, "manifest.json"), { force: true });
}

function buildGenerationFromSources(root, source, options = {}) {
  const sink = timingSink(options);
  const nodes = [];
  const edges = [];
  const fileNodes = new Map();
  const addNode = (node) => {
    const normalized = {
      id: node.id,
      kind: node.kind,
      labels: node.labels ?? [],
      name: node.name,
      qualifiedName: node.qualifiedName ?? node.name,
      path: normalizePath(node.path),
      confidence: node.confidence ?? 1,
      evidence: node.evidence,
    };
    nodes.push(normalized);
    return normalized;
  };
  for (const file of source.files) {
    const node = addNode({
      id: `file:${file.path}`,
      kind: "file",
      labels: ["File"],
      name: file.path.split("/").at(-1),
      qualifiedName: file.path,
      path: file.path,
      evidence: [fileEvidence(file, 1, Math.max(1, file.lines.length))],
    });
    fileNodes.set(file.path, node);
  }
  const parseCache = options.parseCache ?? emptyCache();
  const nextRecords = [];
  measureStage(sink, "parse_cache_extraction", () => {
    for (const file of source.files.filter(isCodeFile)) {
      const cached = parseCache.records.get(file.path);
      if (cached && cached.contentHash === file.contentHash) {
        // Byte-identical file: reuse its already-extracted symbol nodes verbatim.
        for (const symbol of cached.symbols) nodes.push(symbol);
        nextRecords.push({ path: file.path, record: cached });
      } else {
        const collected = [];
        extractSymbols(file, (node) => {
          const normalized = addNode(node);
          collected.push(normalized);
          return normalized;
        });
        nextRecords.push({ path: file.path, record: { contentHash: file.contentHash, symbols: collected } });
      }
    }
  });
  measureStage(sink, "indexed_global_resolution", () => {
    const resolutionIndexes = createResolutionIndexes(source.files, nodes);
    for (const file of source.files.filter(isCodeFile)) {
    const sourceNode = fileNodes.get(file.path);
    if (!sourceNode) continue;
    for (const imported of extractImports(file, source.files)) {
      const targetNode = fileNodes.get(imported);
      // A resolved import specifier is deterministic path resolution — the
      // most certain tier we have (blueprint B3).
      if (targetNode) {
        edges.push(importEdgeRecord(sourceNode, targetNode, imported, true, [fileEvidence(file, 1, 1)]));
        const imports = resolutionIndexes.importsByFile.get(file.path) ?? new Set();
        imports.add(imported);
        resolutionIndexes.importsByFile.set(file.path, imports);
      }
    }
    // Unresolved relative imports must be tagged and kept, never dropped
    // (blueprint B3). extractImports() only returns hits; this is the miss list.
    for (const specifier of extractUnresolvedImportSpecifiers(file, source.files)) {
      edges.push(importEdgeRecord(sourceNode, null, specifier, false, [fileEvidence(file, 1, 1)]));
    }
    }
    // Symbols are complete only after extraction/cache reuse. Build resolver
    // indexes once over that complete universe, then share them across all
    // resolution passes so incremental rebuilds retain global re-resolution.
    populateSymbolResolutionIndexes(resolutionIndexes, nodes);
    resolutionIndexes.schemaNodes = nodes.filter((node) => node.labels?.some((label) => label.startsWith("GraphQL") || label.startsWith("Sql")));
    addSchemaReferenceEdges(source.files, nodes, edges, resolutionIndexes);
    addCallEdges(source.files, nodes, edges, { resolverIndexes: resolutionIndexes, work: options.resolverWork });
    addConfigEdges(source.files, nodes, edges, resolutionIndexes);
  });
  const factProvider = { id: "lexical", version: PROVIDER.version };
  const cleanNodes = dedupeBy(nodes, (node) => node.id).map((node) => ({ ...node, factProvider }));
  const rawEdges = dedupeBy(edges, (item) => `${item.kind}:${item.source}:${item.target ?? item.specifier ?? ""}:${item.evidence?.[0]?.path ?? ""}`)
    .map((edge) => ({ ...edge, factProvider }));
  let generation;
  measureStage(sink, "identity", () => {
    const candidateGeneration = { schemaVersion: 1, provider: PROVIDER, manifest: null, nodes: cleanNodes, edges: rawEdges, repoRoot: root };
    const docMap = readDocMap(root, null);
    const docTruth = docMap
      ? buildDocCodeJoins(candidateGeneration, { docMap })
      : { schemaVersion: 1, provider: PROVIDER.id, joins: [], supersedes: [], truncated: false, sourceDocMap: { docs: 0, claims: 0, generatedAt: null, docPaths: [], claimPaths: [] } };
    const cleanEdges = dedupeBy(rawEdges, (item) => `${item.kind}:${item.source}:${item.target ?? item.specifier ?? ""}:${item.evidence?.[0]?.path ?? ""}`);
    const sourceFingerprint = sourceHash(source.files);
    const generationId = computeGenerationId(cleanNodes, cleanEdges, sourceFingerprint);
    const manifest = {
    schemaVersion: 1,
    provider: PROVIDER,
    // Stable generation stamp derived from the generation id, so byte-identity
    // on unchanged rebuilds is preserved.
    generatedAt: `gen:${generationId.replace(/^xxh128:/, "").slice(0, 16)}`,
    generationId,
    complete: true,
    // The fileLimit the build was run with (0 = unlimited). graphStatus and
    // downstream consumers use this to scope their re-scan so a huge real
    // workspace (D:\Claude has 1M+ directories) does not OOM the doctor.
    fileLimit: Number(options.fileLimit ?? 0),
    truncated: Boolean(source.fileLimitReached || source.traversalTruncated),
    truncationReasons: source.truncationReasons,
    repo: {
      rootName: root.split(/[\\/]/).at(-1),
      sourceHash: sourceFingerprint,
      fileCount: source.files.length,
    },
    counts: {
      nodes: cleanNodes.length,
      edges: cleanEdges.length,
      joins: docTruth.joins.length,
      supersedes: docTruth.supersedes.length,
    },
    };
    generation = { schemaVersion: 1, provider: PROVIDER, manifest, nodes: cleanNodes, edges: cleanEdges, docTruth, repoRoot: root };
  });
  return { generation, parseCache: nextCache(nextRecords) };
}

export function parseFileFacts(root, file, options = {}) {
  const files = options.files ?? [file];
  const fileByPath = new Map(files.map((item) => [normalizePath(item.path), item]));
  const current = { ...file, path: normalizePath(file.path) };
  fileByPath.set(current.path, current);
  const nodes = [];
  const edges = [];
  const fileNode = {
    id: `file:${current.path}`,
    kind: "file",
    labels: ["File"],
    name: current.path.split("/").at(-1),
    qualifiedName: current.path,
    path: current.path,
    confidence: 1,
    evidence: [fileEvidence(current, 1, Math.max(1, current.lines?.length ?? 1))],
  };
  nodes.push(fileNode);
  if (isCodeFile(current)) extractSymbols(current, (node) => { nodes.push(node); return node; });
  for (const imported of extractImports(current, [...fileByPath.values()])) {
    const target = fileByPath.get(imported);
    if (target) {
      edges.push(importEdgeRecord(fileNode, {
        id: `file:${target.path}`,
        path: target.path,
      }, imported, true, [fileEvidence(current, 1, 1)]));
    }
  }
  for (const specifier of extractUnresolvedImportSpecifiers(current, [...fileByPath.values()])) {
    edges.push(importEdgeRecord(fileNode, null, specifier, false, [fileEvidence(current, 1, 1)]));
  }
  const resolutionNodes = [
    ...nodes,
    ...(options.symbols ?? [])
      .filter((symbol) => normalizePath(symbol.path) !== current.path)
      .map((symbol) => ({ ...symbol, kind: "symbol" })),
  ];
  addCallEdges([...fileByPath.values()], resolutionNodes, edges, { sourcePaths: new Set([current.path]) });
  const dependencies = extractImports(current, [...fileByPath.values()]).map((sourcePath) => ({
    dependentPath: current.path,
    sourcePath,
    reason: "import",
  }));
  const dedupe = (items, key) => [...new Map(items.map((item) => [key(item), item])).values()];
  return {
    nodes: dedupe(nodes, (node) => node.id).map((node) => ({ ...node, factProvider: { id: "lexical", version: PROVIDER.version } })),
    edges: dedupe(edges, (edge) => edge.id).map((edge) => ({ ...edge, factProvider: { id: "lexical", version: PROVIDER.version } })),
    dependencies: dedupe(dependencies, (item) => `${item.sourcePath}:${item.dependentPath}:${item.reason}`),
  };
}

function scanSources(root, fileLimit = 0, walkOptions = {}) {
  const files = [];
  const traversal = walk(root, { ...walkOptions, ignoredPrefixes: configuredIgnoredPrefixes(root, walkOptions) });
  let fileLimitReached = false;
  for (const absolutePath of traversal.paths) {
    let path;
    try {
      path = normalizePath(relative(root, absolutePath));
    } catch {
      continue;
    }
    const ext = sourceExtension(path);
    const isParsed = CODE_EXTENSIONS.has(ext) || AST_LANGUAGE_EXTENSIONS.has(ext) || isFileOnlyPath(path);
    let size;
    try {
      size = statSync(absolutePath).size;
    } catch {
      continue;
    }
    if (size > 2 * 1024 * 1024) continue;
    if (!isParsed) {
      files.push({
        absolutePath,
        path,
        contentHash: `size:${size}`,
        size,
        lines: [],
      });
      if (fileLimit > 0 && files.length >= fileLimit) {
        fileLimitReached = true;
        break;
      }
      continue;
    }
    let bytes;
    try {
      bytes = readFileSync(absolutePath);
    } catch {
      continue;
    }
    if (bytes.includes(0)) continue;
    const text = bytes.toString("utf8");
    let normalizedBytes = bytes;
    let normalizedText = text;
    if (path === "README.md") {
      normalizedText = text.replace(
        /\n?<!-- blueprint:docs:start -->[\s\S]*?<!-- blueprint:docs:end -->\n?/,
        "",
      );
      normalizedBytes = Buffer.from(normalizedText, "utf8");
    }
    files.push({
      absolutePath,
      path,
      text: normalizedText,
      lines: normalizedText.split(/\r?\n/),
      contentHash: xxh128(normalizedBytes),
      size: bytes.length,
    });
    if (fileLimit > 0 && files.length >= fileLimit) {
      fileLimitReached = true;
      break;
    }
  }
  files.sort((left, right) => compareRepoPaths(left.path, right.path));
  const truncationReasons = new Set(traversal.reasons);
  if (fileLimitReached) truncationReasons.add("file_limit");
  return {
    files,
    fileLimitReached,
    traversalTruncated: traversal.state.truncated,
    truncationReasons: [...truncationReasons].sort(),
  };
}

function configuredIgnoredPrefixes(root, options = {}) {
  if (Array.isArray(options.ignoredPrefixes)) return normalizeIgnoredPrefixes(options.ignoredPrefixes);
  try {
    const config = JSON.parse(readFileSync(join(root, ".agent", "config.json"), "utf8"));
    return normalizeIgnoredPrefixes(config?.ignoredPrefixes);
  } catch {
    return [];
  }
}

// Iterative directory walker. D:/Claude contains over a million directories,
// which overflows the JS call stack when walked recursively. An explicit stack
// keeps memory bounded and avoids the limit. Memory safety guarantees:
//   1. `maxDirs` cap is checked BEFORE any per-iteration work so a runaway
//      source tree cannot exceed the budget.
//   2. Directory entries are streamed and retained only up to
//      `maxEntriesPerDir + 1`, so an escaped cache cannot allocate an
//      unbounded Dirent array.
const MAX_ENTRIES_PER_DIR = 5000;

function gitEligiblePaths(root, options = {}) {
  // Portability contract: the persisted graph must be a pure function of the
  // COMMIT, not the working tree. Tracked-only (`--cached`, no `--others`) makes a
  // clean clone reproduce the graph byte-for-byte, which is what lets `.agent/`
  // travel through git instead of being regenerated on every machine. Untracked
  // files are surfaced separately as a dirty-tree warning (see uncommittedReport),
  // never silently indexed. `trackedOnly:false` keeps the legacy working-tree scan
  // for callers that explicitly want the local dirty view.
  const trackedOnly = options.trackedOnly !== false;
  if (!existsSync(join(root, ".git"))) return null;
  const lsFilesArgs = trackedOnly
    ? ["-C", root, "ls-files", "-z", "--cached", "--"]
    : ["-C", root, "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--"];
  const result = spawnSync("git", lsFilesArgs, { encoding: "buffer", maxBuffer: 512 * 1024 * 1024, windowsHide: true });
  if (result.status !== 0 || !Buffer.isBuffer(result.stdout)) return null;

  const files = new Set();
  const directories = new Set();
  for (const rawPath of result.stdout.toString("utf8").split("\0")) {
    const path = normalizePath(rawPath);
    if (!path) continue;
    files.add(path);
    const parts = path.split("/");
    parts.pop();
    let parent = "";
    for (const part of parts) {
      parent = parent ? `${parent}/${part}` : part;
      directories.add(parent);
    }
  }
  return { files, directories };
}

function walk(root, options = {}) {
  const maxDirs = Number(options.maxDirs ?? 50000);
  const maxEntriesPerDir = Number(options.maxEntriesPerDir ?? MAX_ENTRIES_PER_DIR);
  const gitEligible = gitEligiblePaths(root, options);
  const ignoredPrefixes = normalizeIgnoredPrefixes(options.ignoredPrefixes);
  const reasons = new Set();
  const state = { truncated: false };

  function* iteratePaths() {
    const visited = new Set();
    const stack = [root];
    let processedDirs = 0;
    while (stack.length > 0) {
      if (processedDirs >= maxDirs) {
        reasons.add("directory_limit");
        state.truncated = true;
        return;
      }
      const directory = stack.pop();
      const real = resolve(directory);
      let canonical = real;
      try {
        canonical = realpathSync(real);
      } catch {
        /* use resolved form */
      }
      if (visited.has(canonical)) continue;
      visited.add(canonical);
      processedDirs += 1;
      const entries = [];
      let dir;
      try {
        dir = opendirSync(directory);
        while (true) {
          const entry = dir.readSync();
          if (!entry) break;
          entries.push(entry);
          if (entries.length > maxEntriesPerDir) break;
        }
      } catch {
        continue;
      } finally {
        try {
          dir?.closeSync();
        } catch {
          /* already closed */
        }
      }
      if (entries.length > maxEntriesPerDir) {
        reasons.add("directory_entry_limit");
        state.truncated = true;
        continue;
      }
      entries.sort((left, right) => left.name.localeCompare(right.name));
      const parentName = basename(directory);
      const childDirectories = [];
      for (const entry of entries) {
        const absolutePath = join(directory, entry.name);
        const repoPath = normalizePath(relative(root, absolutePath));
        if (entry.isDirectory()) {
          if (IGNORED.has(entry.name) || isBlueprintOutputDir(entry.name)) continue;
          if (pathMatchesIgnoredPrefix(repoPath.endsWith("/") ? repoPath : `${repoPath}/`, ignoredPrefixes)) continue;
          if (gitEligible && !gitEligible.directories.has(repoPath)) continue;
          childDirectories.push(absolutePath);
        } else if (entry.isFile()) {
          if (IGNORED_FILE_NAMES.has(entry.name)) continue;
          if (pathMatchesIgnoredPrefix(repoPath, ignoredPrefixes)) continue;
          if (gitEligible && !gitEligible.files.has(repoPath)) continue;
          if ((entry.name === "product.md" || entry.name === "architecture.md") && parentName === "docs") continue;
          try {
            if (statSync(absolutePath).size <= 2 * 1024 * 1024) {
              yield absolutePath;
            }
          } catch {
            /* unreadable file: skip */
          }
        }
      }
      for (let index = childDirectories.length - 1; index >= 0; index -= 1) {
        stack.push(childDirectories[index]);
      }
    }
  }

  return { paths: iteratePaths(), state, reasons };
}

function createResolutionIndexes(files, nodes) {
  const sourceFilesByPath = new Map(files.map((file) => [file.path, file]));
  const filesByExtension = new Map();
  for (const file of files) {
    const extension = file.path.split(".").at(-1)?.toLowerCase() ?? "";
    const matches = filesByExtension.get(extension) ?? [];
    matches.push(file);
    filesByExtension.set(extension, matches);
  }
  return {
    sourceFilesByPath,
    filesByPath: new Map(nodes.filter((node) => node.kind === "file").map((node) => [node.path, node])),
    filesByExtension,
    symbolsByName: new Map(),
    symbolsByInsensitiveName: new Map(),
    importsByFile: new Map(),
  };
}

function populateSymbolResolutionIndexes(indexes, nodes) {
  for (const node of nodes) {
    if (node.kind !== "symbol" || !["Method", "Function"].some((label) => node.labels.includes(label))) continue;
    const name = node.qualifiedName.split(".").at(-1);
    let group = indexes.symbolsByName.get(name);
    if (!group) {
      group = { name, targets: [], order: indexes.symbolsByName.size };
      indexes.symbolsByName.set(name, group);
      const folded = name.toLowerCase();
      const insensitive = indexes.symbolsByInsensitiveName.get(folded) ?? [];
      insensitive.push(group);
      indexes.symbolsByInsensitiveName.set(folded, insensitive);
    }
    group.targets.push(node);
  }
}

function addCallEdges(files, nodes, edges, options = {}) {
  const indexes = options.resolverIndexes ?? createResolutionIndexes(files, nodes);
  if (indexes.symbolsByName.size === 0) populateSymbolResolutionIndexes(indexes, nodes);
  const targets = [...indexes.symbolsByName.values()].flatMap((group) => group.targets);
  const sources = nodes.filter((node) => node.kind === "symbol"
    && (!options.sourcePaths || options.sourcePaths.has(node.path))
    && ["Method", "Function", "Test"].some((label) => node.labels.includes(label)));
  const importsByPath = indexes.importsByFile.size > 0 ? indexes.importsByFile : importsByEdges(edges);
  for (const source of sources) {
    const file = indexes.sourceFilesByPath.get(source.path);
    if (!file) continue;
    const ev = source.evidence[0];
    const body = file.lines.slice(ev.startLine - 1, ev.endLine).join("\n");
    // Harvest every called name from this body ONCE (extractCallNames is the proven
    // inverse of containsCall, 302K-check equivalence), then look up only matching
    // candidate groups. This replaces the full target-name scan per source while
    // retaining old target-group order for byte-identical edge output.
    const harvest = extractCallNames(file, body);
    const candidateGroups = candidateGroupsForCalls(harvest, indexes);
    if (options.work) {
      options.work.fullCandidateInspections = (options.work.fullCandidateInspections ?? 0) + targets.length;
      options.work.candidateNameLookups = (options.work.candidateNameLookups ?? 0) + harvest.names.length;
      options.work.candidateInspections = (options.work.candidateInspections ?? 0)
        + candidateGroups.reduce((count, group) => count + group.targets.length, 0);
    }
    for (const { name: callName, targets: namedTargets } of candidateGroups) {
      const kind = source.labels.includes("Test") ? "TESTS" : "CALLS";
      const sameFile = namedTargets.filter((target) => target.path === source.path && target.id !== source.id);
      const importedPaths = importsByPath.get(source.path) ?? new Set();
      const imported = namedTargets.filter((target) => importedPaths.has(target.path) && target.id !== source.id);
      const uniqueFallback = namedTargets.length === 1 && namedTargets[0].id !== source.id ? namedTargets : [];
      // Tier is DERIVED from which resolution strategy actually produced the
      // match — not hardcoded — per blueprint B3. Same-file name match is
      // bounded (no cross-module collision possible); an import-linked or
      // repo-wide-unique match crosses file boundaries on a heuristic.
      const [resolvedTargets, tier] = sameFile.length > 0
        ? [sameFile, EDGE_CONFIDENCE_TIERS.SAME_FILE_LEXICAL]
        : imported.length > 0
          ? [imported, EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC]
          : uniqueFallback.length > 0
            ? [uniqueFallback, EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC]
            : [[], null];
      for (const target of resolvedTargets) {
        edges.push(edge(kind, source, target, source.evidence, tier));
      }
      // Ambiguous call: 2+ OTHER (non-self) candidate symbols share this
      // name repo-wide and none is in the same file or an imported file, so
      // none can be honestly preferred. Tagged UNRESOLVED and kept — never
      // silently dropped, never promoted by guessing one (blueprint B3).
      //
      // Excludes self on purpose: containsCall/extractCallNames' regex can
      // match a function's OWN declaration line as if it called itself
      // (`function sharedName() {` looks like a call to a naive `name(`
      // pattern). When that leaves exactly one OTHER candidate, this is that
      // pre-existing lexical-extraction artifact, not real ambiguity — always
      // true, so reporting it as "N candidates, none preferred" would be a
      // false claim. It stays silent rather than fabricating a tag.
      const otherCandidates = namedTargets.filter((target) => target.id !== source.id);
      if (resolvedTargets.length === 0 && otherCandidates.length > 1) {
        edges.push(unresolvedCallEdge(kind, source, callName, otherCandidates.length, source.evidence));
      }
    }
  }
}

function importsByEdges(edges) {
  const importsByPath = new Map();
  for (const importEdge of edges) {
    if (importEdge.kind !== "IMPORTS" || importEdge.target === null) continue;
    const sourcePath = importEdge.source.replace(/^file:/, "");
    const targetPath = importEdge.target.replace(/^file:/, "");
    const imported = importsByPath.get(sourcePath) ?? new Set();
    imported.add(targetPath);
    importsByPath.set(sourcePath, imported);
  }
  return importsByPath;
}

function candidateGroupsForCalls(harvest, indexes) {
  const groups = new Map();
  for (const name of harvest.names) {
    const matches = harvest.caseInsensitive
      ? indexes.symbolsByInsensitiveName.get(name.toLowerCase()) ?? []
      : [indexes.symbolsByName.get(name)].filter(Boolean);
    for (const group of matches) groups.set(group.name, group);
  }
  // Old resolution visited target-name groups in first-symbol order, not call
  // occurrence order. Keep that order byte-identical while avoiding its full
  // candidate-name scan for every source symbol.
  return [...groups.values()].sort((left, right) => left.order - right.order);
}

function addConfigEdges(files, nodes, edges, indexes = createResolutionIndexes(files, nodes)) {
  const filesByPath = indexes.filesByPath;
  for (const source of nodes.filter((node) => node.labels?.includes("Const"))) {
    const file = indexes.sourceFilesByPath.get(source.path);
    if (!file) continue;
    const line = file.lines[source.evidence[0].startLine - 1] ?? "";
    for (const match of line.matchAll(/["']([^"']+\.(?:json|yaml|yml|toml|sqlite|db))["']/g)) {
      const target = filesByPath.get(normalizePath(match[1]));
      // A string-literal-to-filename match is a heuristic, not a resolved
      // binding — it crosses file boundaries on text pattern alone.
      if (target) edges.push(edge("CONFIGURES", source, target, source.evidence, EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC));
    }
  }
}

function edge(kind, source, target, evidence, tier) {
  if (!tier) throw new Error(`edge() requires an explicit confidenceTier (kind=${kind})`);
  return {
    id: `edge:${kind}:${source.id}->${target.id}`,
    kind,
    source: source.id,
    target: target.id,
    confidence: tierConfidence(tier),
    confidenceTier: tier,
    resolved: true,
    specifier: null,
    evidence,
  };
}

function unresolvedCallEdge(kind, source, callName, candidateCount, evidence) {
  return {
    id: `edge:${kind}:${source.id}->unresolved:${callName}`,
    kind,
    source: source.id,
    target: null,
    confidence: tierConfidence(EDGE_CONFIDENCE_TIERS.UNRESOLVED),
    confidenceTier: EDGE_CONFIDENCE_TIERS.UNRESOLVED,
    resolved: false,
    specifier: callName,
    reason: `${candidateCount} candidate symbol(s) named "${callName}" exist repo-wide; none is in the same file or an imported file`,
    evidence,
  };
}

function importEdgeRecord(sourceNode, targetNode, specifier, resolved, evidence) {
  const tier = resolved ? EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION : EDGE_CONFIDENCE_TIERS.UNRESOLVED;
  return {
    id: `edge:IMPORTS:${sourceNode.id}->${resolved ? targetNode.id : `unresolved:${specifier}`}`,
    kind: "IMPORTS",
    source: sourceNode.id,
    target: resolved ? targetNode.id : null,
    confidence: tierConfidence(tier),
    confidenceTier: tier,
    resolved,
    specifier: resolved ? null : specifier,
    evidence,
  };
}

function fileEvidence(file, startLine, endLine) {
  return { path: file.path, startLine, endLine, contentHash: file.contentHash };
}

function evidenceFresh(generation, evidence) {
  return evidence.every((item) => {
    const file = generation.nodes.find((node) => node.kind === "file" && node.path === item.path);
    return file?.evidence?.[0]?.contentHash === item.contentHash;
  });
}

function scoreNode(node, terms) {
  if (!terms.length) return 1;
  const haystack = `${node.kind} ${node.name} ${node.qualifiedName} ${node.path}`.toLowerCase();
  return terms.reduce((sum, term) => sum + (haystack.includes(term) ? 1 : 0), 0);
}

function queryTerms(query) {
  const stop = new Set(["a", "an", "and", "for", "in", "of", "the", "to", "where"]);
  return [...new Set(String(query).replace(/([a-z0-9])([A-Z])/g, "$1 $2").toLowerCase().split(/[^a-z0-9_]+/).filter((term) => term.length > 1 && !stop.has(term)))];
}

function generationId(nodes, edges, files) {
  return computeGenerationId(nodes, edges, sourceHash(files));
}

// Blueprint's own generated docs (docs/product.md, docs/architecture.md) carry a
// generation header as their first line. They are OUTPUTS of a build, not source
// truth, and they embed the generationId — so every build rewrites them and, if
// they were hashed, the graph would be stale the instant it finished. The build
// then throws "generated graph is stale immediately after build" and every query
// fails `graph_rebuild_required`.
//
// This was latent for as long as `build` refused to run on a dirty tree; removing
// that refusal (the graph is a local index now, not a committed artifact) made it
// reachable on any repo where the generated docs are tracked. Excluding them here
// applies the rule README.md already had — its generated pointer block is stripped
// before hashing for exactly this reason.
//
// Excluded from the HASH only: the docs remain scanned, remain graph nodes, and
// still take part in doc↔code joins. A real source edit still moves the hash.
const GENERATED_DOC_MARKER = "<!-- generated by blueprint";

function isGeneratedDoc(file) {
  return typeof file.lines?.[0] === "string" && file.lines[0].startsWith(GENERATED_DOC_MARKER);
}

function sourceHash(files) {
  return `xxh128:${xxh128(
    files
      .filter((file) => !isGeneratedDoc(file))
      .map((file) => `${file.path}:${file.contentHash}`)
      .join("\n"),
  )}`;
}

function estimateTokens(evidence) {
  return Math.max(1, Number(evidence.endLine) - Number(evidence.startLine) + 1);
}

function isCodeFile(file) {
  return CODE_EXTENSIONS.has(file.path.split(".").at(-1)?.toLowerCase());
}

function normalizePath(value) {
  return String(value).replaceAll("\\", "/").replace(/^\.\//, "");
}

// XXH3-128 (via hash-wasm) — content hashing for change detection and
// dedup only (file bytes, source-tree fingerprint, generation id). No trust
// boundary depends on collision resistance here: the blueprint→Cortex
// federation provider reads only the generation identity, never validates it
// cryptographically (membrane/engine/federation/providers/blueprint.py).
// The hasher's construction is async (WASM init); init()/update()/digest()
// are synchronous once it exists, so the async cost is paid exactly once at
// module load (top-level await) and every call site below stays synchronous
// — no per-call Promise, no blocking busy-wait. Kept as a local copy (not a
// shared import) to match this file's existing "manually kept in sync"
// duplication pattern (see the IGNORED_DIRS comment in sources/_shared.mjs).
const xxhasher = await createXXHash128();

function xxh128(value) {
  xxhasher.init();
  xxhasher.update(value);
  return xxhasher.digest("hex");
}

function writeJson(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

function dedupeBy(items, keyFn) {
  const seen = new Set();
  return items.filter((item) => {
    const key = keyFn(item);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function escapeRegExp(value) {
  return String(value).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
