// Per-file parse cache — the mechanism behind B2 incremental indexing.
//
// The expensive part of a graph build is PARSING: reading each file's bytes and
// running the language extractors over them. Edge RESOLUTION (imports, calls,
// config wiring) is cheap map lookups once the per-file facts exist. So the graph
// can be kept fresh incrementally by caching parse facts keyed by content hash and
// re-parsing only the files whose bytes changed — while resolution still runs
// globally over the union, which is what prevents ghost edges (an unchanged file's
// edge into a symbol that a changed file renamed away is dropped because resolution
// re-runs against the current symbol universe every time).
//
// The cache is a pure derivative of file bytes: a record is valid iff its stored
// contentHash equals the current file's contentHash. It is machine-local
// regenerable state (.agent/graph/parse-cache/), never a source of truth.

import { createXXHash128 } from "hash-wasm";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

// Bump when the SHAPE of a cached record changes (fields, extractor semantics).
// A mismatch invalidates every record — a stale-shape hit is a correctness bug,
// not a slow path, so we fail closed to a full reparse.
export const PARSE_CACHE_VERSION = 1;

// XXH3-128 (via hash-wasm) — used only for a temp-filename salt here, not for
// integrity. The WASM hasher's own constructor is async, but init()/update()/
// digest() are synchronous once it exists, so we pay the async cost exactly
// once at module load (top-level await) and every call site stays sync — no
// per-call Promise, no blocking busy-wait.
const xxhasher = await createXXHash128();

function xxh128(text) {
  xxhasher.init();
  xxhasher.update(text);
  return xxhasher.digest("hex");
}

// One deterministic file per repo, holding all records. A single file (vs one per
// source file) keeps writes atomic and the on-disk footprint bounded — this is the
// same single-artifact discipline the generation store now follows.
function cachePath(outDir) {
  return join(outDir, "graph", "parse-cache", "records.json");
}

export function loadParseCache(outDir) {
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
  return { version: PARSE_CACHE_VERSION, records: new Map(Object.entries(parsed.records)) };
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
// cached record (byte-identical) and which must be reparsed (new or changed).
// Deleted files simply never appear in `files`, so their records are dropped when
// the fresh cache is rebuilt from `files` alone.
export function diffFiles(files, cache) {
  const reused = [];
  const changed = [];
  for (const file of files) {
    const record = cache.records.get(file.path);
    if (record && record.contentHash === file.contentHash) {
      reused.push({ file, record });
    } else {
      changed.push(file);
    }
  }
  return { reused, changed };
}

// Build the next cache from the records we are keeping (reused) plus freshly parsed
// ones. Keyed by path; deleted files fall out because they were never added.
export function nextCache(entries) {
  const records = new Map();
  for (const { path, record } of entries) records.set(path, record);
  return { version: PARSE_CACHE_VERSION, records };
}
