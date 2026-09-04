// Per-file parse cache — the mechanism behind B2 incremental indexing.
//
// The expensive part of a graph build is PARSING: reading each file's bytes and
// running the language extractors over them. Edge RESOLUTION (imports, calls,
// config wiring) is cheap map lookups once the per-file facts exist. So the graph
// can be kept fresh incrementally by caching parse facts keyed by content hash and
// semantic extractor fingerprint — while resolution still runs globally over the
// union, which is what prevents ghost edges.
//
// The cache is machine-local regenerable state (.agent/graph/parse-cache/), never
// a source of truth. A byte-identical file is reusable only when the code/config
// that can change its extracted facts is also byte-identical.

import { createHash } from "node:crypto";
import { createXXHash128 } from "hash-wasm";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { STATIC_PROVIDER } from "./provider-identity.mjs";

// Shape-only version. Extractor semantic changes no longer rely on a human
// remembering to bump this number; semantic inputs are fingerprinted below.
export const PARSE_CACHE_VERSION = 2;

const HERE = dirname(fileURLToPath(import.meta.url));
const BLUEPRINT_ROOT = resolve(HERE, "..", "..");
const SEMANTIC_INPUT_PATHS = Object.freeze([
  join(HERE, "language-extractors.mjs"),
  join(HERE, "schema-extractors.mjs"),
  join(HERE, "language-registry.mjs"),
  join(BLUEPRINT_ROOT, "grammars", "catalog.json"),
  join(BLUEPRINT_ROOT, "grammars", "manifest.json"),
]);

function semanticInputDigest() {
  const hash = createHash("sha256");
  for (const path of SEMANTIC_INPUT_PATHS) {
    hash.update(path.slice(BLUEPRINT_ROOT.length).replaceAll("\\", "/"));
    hash.update("\0");
    hash.update(readFileSync(path));
    hash.update("\0");
  }
  return hash.digest("hex");
}

// Evaluated from the actual extractor sources/config on process start. A parser
// fix therefore changes cache validity automatically even when source bytes and
// PARSE_CACHE_VERSION are unchanged.
const DEFAULT_SEMANTIC_INPUT_DIGEST = semanticInputDigest();

export function extractorFingerprintForPath(path, options = {}) {
  const semanticDigest = options.semanticInputDigest ?? DEFAULT_SEMANTIC_INPUT_DIGEST;
  const providerId = options.providerId ?? STATIC_PROVIDER.id;
  const providerVersion = options.providerVersion ?? STATIC_PROVIDER.version;
  const semanticSalt = options.semanticSalt ?? "";
  const extension = extname(String(path)).replace(/^\./, "").toLowerCase() || "<none>";
  return createHash("sha256")
    .update(`parse-cache:${PARSE_CACHE_VERSION}\n`)
    .update(`provider:${providerId}@${providerVersion}\n`)
    .update(`extension:${extension}\n`)
    .update(`semantic-inputs:${semanticDigest}\n`)
    .update(`semantic-salt:${semanticSalt}\n`)
    .digest("hex");
}

// XXH3-128 (via hash-wasm) — used only for a temp-filename salt here, not for
// integrity. The WASM hasher's own constructor is async, but init()/update()/
// digest() are synchronous once it exists, so we pay the async cost exactly
// once at module load (top-level await) and every call site stays sync.
const xxhasher = await createXXHash128();

function xxh128(text) {
  xxhasher.init();
  xxhasher.update(text);
  return xxhasher.digest("hex");
}

function cachePath(outDir) {
  return join(outDir, "graph", "parse-cache", "records.json");
}

export function loadParseCache(outDir, fingerprintOptions = {}) {
  const path = cachePath(outDir);
  if (!existsSync(path)) return emptyCache();
  let parsed;
  try {
    parsed = JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return emptyCache();
  }
  if (!parsed || parsed.version !== PARSE_CACHE_VERSION || typeof parsed.records !== "object") {
    return emptyCache();
  }
  const records = new Map();
  for (const [recordPath, record] of Object.entries(parsed.records)) {
    if (!record || typeof record !== "object") continue;
    const expected = extractorFingerprintForPath(recordPath, fingerprintOptions);
    if (record.extractorFingerprint !== expected) continue;
    records.set(recordPath, record);
  }
  return { version: PARSE_CACHE_VERSION, records };
}

export function emptyCache() {
  return { version: PARSE_CACHE_VERSION, records: new Map() };
}

// Atomic publish: temp sibling + rename, same volume. A reader never sees a
// half-written cache — it sees the old file or the new one.
export function writeParseCache(outDir, cache) {
  const path = cachePath(outDir);
  mkdirSync(dirname(path), { recursive: true });
  const serializable = { version: PARSE_CACHE_VERSION, records: Object.fromEntries(cache.records) };
  const tmp = `${path}.${process.pid}.${xxh128(path).slice(0, 8)}.tmp`;
  writeFileSync(tmp, JSON.stringify(serializable));
  renameSync(tmp, path);
}

// Diff the current file set against the cache. Returns which files can reuse a
// cached record (byte-identical AND extractor-identical) and which must be
// reparsed. Deleted files simply never appear in `files`.
export function diffFiles(files, cache, fingerprintOptions = {}) {
  const reused = [];
  const changed = [];
  for (const file of files) {
    const record = cache.records.get(file.path);
    const expectedFingerprint = extractorFingerprintForPath(file.path, fingerprintOptions);
    if (record
      && record.contentHash === file.contentHash
      && record.extractorFingerprint === expectedFingerprint) {
      reused.push({ file, record });
    } else {
      changed.push(file);
    }
  }
  return { reused, changed };
}

// Build the next cache from the records we are keeping plus freshly parsed
// records. The fingerprint is stamped here, centrally, so every caller gets the
// same semantic validity contract without remembering to add metadata itself.
export function nextCache(entries, fingerprintOptions = {}) {
  const records = new Map();
  for (const { path, record } of entries) {
    records.set(path, {
      ...record,
      extractorFingerprint: extractorFingerprintForPath(path, fingerprintOptions),
    });
  }
  return { version: PARSE_CACHE_VERSION, records };
}
