// Adapter 6 — Live files newer than the graph.
//
// Detects files whose working-tree mtime or content hash has moved past
// the Blueprint graph generation timestamp. Per dispatch overlay ownership:
// G1 detects, G3 ranks, G5 (this lane) produces typed candidates.
//
// Strategy:
//   - Read the graph's `manifest.generatedAt` (or fall back to the source
//     file count + sourceHash timestamp recorded in `.agent/graph/graph.json`).
//   - Walk the supported source tree using the same IGNORED rules as the
//     static provider.
//   - For every file whose mtime > graph.generatedAt AND whose path is NOT
//     in the recorded per-file hashes from the graph's `graph.json`,
//     emit a `live_overlay` candidate with the file body (bounded).
//   - For files whose recorded contentHash differs from the live contentHash,
//     also emit — the graph knows about the file but its body has drifted.
//
// This adapter DOES NOT replace `dirty-files`. They are complementary:
//   - `dirty-files` = `git status` overlay (tracked-status-aware)
//   - `live-overlay` = mtime/hash drift against the graph generation,
//     including untracked files not yet noticed by git, and files that
//     moved past the most recent Blueprint rebuild.
//
// Both emit at Layer 1 with providerScore 0.85–0.95 depending on signal
// strength. The planner picks one (or none) per file via its dedupe rule.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { openStoreReadOnly, closeStore, loadGeneration } from "../graph/store-sqlite.mjs";
import {
  IGNORED_DIRS,
  SCOPE_PROVIDER,
  isSupportedPath,
  makeCandidate,
  normalizePath,
  readFileBounded,
  safeResolve,
  xxh3Hex,
  tryGit,
} from "./_shared.mjs";

const ADAPTER_ID = "membrane-sources/live-overlay";
const ADAPTER_LAYER = 1; // Layer 1 — live working-tree truth
const DEFAULT_BODY_BYTE_CAP = 2000;
const TRUNCATION_MARKER = "\n\n[…file body truncated; per-file byte cap applied…]";

function loadGenerationMeta(repoRoot) {
  // The graph lives in graph.db. `.agent/index.jsonl` is not a second copy
  // of it — it is the lighter bootstrap index, present when a repo has been
  // indexed but never fully built, so it stays as a distinct source.
  const dbPath = join(repoRoot, ".agent", "graph", "graph.db");
  if (existsSync(dbPath)) {
    try {
      const db = openStoreReadOnly(dbPath);
      try {
        const generation = loadGeneration(db);
        if (generation) {
          const recordedHashes = new Map();
          for (const node of generation.nodes ?? []) {
            const evidence = node.evidence?.[0];
            if (evidence?.path && evidence?.contentHash) recordedHashes.set(evidence.path, evidence.contentHash);
          }
          return { indexedAt: generation.manifest?.generatedAt ?? null, recordedHashes };
        }
      } finally {
        closeStore(db);
      }
    } catch { /* unreadable store — fall through to the bootstrap index */ }
  }
  const candidates = [
    join(repoRoot, ".agent", "index.jsonl"),
  ];
  for (const path of candidates) {
    if (!existsSync(path)) continue;
    try {
      if (path.endsWith(".jsonl")) {
        const first = readFileSync(path, "utf8").split(/\r?\n/).find(Boolean);
        if (!first) continue;
        const node = JSON.parse(first);
        return {
          indexedAt: node.indexedAt ?? null,
          recordedHashes: new Map(),
          portable: true,
        };
      }
      const raw = JSON.parse(readFileSync(path, "utf8"));
      const manifest = raw?.manifest ?? {};
      const recordedHashes = new Map();
      for (const node of raw?.nodes ?? []) {
        const ev = node.evidence?.[0];
        if (ev?.path && ev?.contentHash) recordedHashes.set(ev.path, ev.contentHash);
      }
      return {
        indexedAt: manifest.generatedAt ?? null,
        recordedHashes,
        portable: false,
      };
    } catch {
      continue;
    }
  }
  return null;
}

function walkFiles(root, out) {
  let entries;
  try {
    entries = readdirSync(root, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    if (entry.isDirectory()) {
      if (IGNORED_DIRS.has(entry.name)) continue;
      walkFiles(join(root, entry.name), out);
    } else if (entry.isFile()) {
      out.push(join(root, entry.name));
    }
  }
}

function liveBody(absolutePath, byteCap) {
  const body = readFileBounded(absolutePath, byteCap);
  if (!body) return null;
  return body;
}

function gitIgnored(repoRoot, relPath) {
  // Cheap check: ask git. This keeps the adapter honest about user intent
  // even if the file's mtime is past the graph timestamp.
  const result = tryGit(repoRoot, ["check-ignore", "--", relPath]);
  if (!result.ok) return false;
  return result.stdout.trim().length > 0;
}

/**
 * @param {string} task
 * @param {{ repoRoot: string, bodyByteCap?: number, generation?: object, maxOverlay?: number }} scope
 */
export function produce(task, scope) {
  const repoRoot = scope?.repoRoot;
  if (!repoRoot) return { candidates: [], omissions: [] };
  const meta = scope?.generationMeta ?? loadGenerationMeta(repoRoot);
  if (!meta) {
    return { candidates: [], omissions: [{ id: `${ADAPTER_ID}:graph`, layer: ADAPTER_LAYER, reason: "graph_unavailable" }] };
  }
  const indexedAt = meta.indexedAt ? new Date(meta.indexedAt).getTime() : null;
  const byteCap = Number(scope?.bodyByteCap) > 0 ? Number(scope.bodyByteCap) : DEFAULT_BODY_BYTE_CAP;
  const maxOverlay = Number(scope?.maxOverlay) > 0 ? Math.floor(Number(scope.maxOverlay)) : 25;

  const candidates = [];
  const omissions = [];
  const filePaths = [];
  walkFiles(repoRoot, filePaths);
  let scannedCount = 0;
  let surfaced = 0;

  for (const absolute of filePaths) {
    if (surfaced >= maxOverlay) break;
    const rel = normalizePath(relative(repoRoot, absolute));
    const safe = safeResolve(repoRoot, rel);
    if (!safe) {
      omissions.push({ id: `${ADAPTER_ID}:${rel}`, layer: ADAPTER_LAYER, reason: "overlay_outside_scope" });
      continue;
    }
    if (!isSupportedPath(safe)) continue;
    scannedCount += 1;
    let stat;
    try { stat = statSync(absolute); } catch { continue; }
    if (stat.size > 2 * 1024 * 1024) continue;
    if (gitIgnored(repoRoot, safe)) continue;

    const recordedHash = meta.recordedHashes?.get(safe) ?? null;
    let liveSha = null;
    if (recordedHash) {
      try {
        liveSha = xxh3Hex(readFileSync(absolute));
      } catch {
        continue;
      }
      if (liveSha === recordedHash) continue; // graph is current with the file
    } else if (indexedAt) {
      if (stat.mtimeMs <= indexedAt) continue; // file unchanged since graph generation
    } else {
      continue; // no generation metadata — cannot establish drift
    }

    const body = liveBody(absolute, byteCap);
    if (!body) continue;
    surfaced += 1;
    candidates.push(
      makeCandidate({
        id: `${ADAPTER_ID}:${safe}`,
        layer: ADAPTER_LAYER,
        sourceKind: "live_overlay",
        sourcePath: safe,
        trustClass: "workspace_tracked",
        providerScore: recordedHash ? 0.9 : 0.85,
        scoreComponents: {
          overlay: recordedHash ? 1.0 : 0.85,
          freshness: 1.0,
        },
        text: body.text,
        startLine: 1,
        endLine: body.lines,
        bodyHash: body.bodyHash,
        estimatedTokens: Math.max(1, body.lines),
        resolver: `blueprint graph overlay --check ${safe}`,
      }),
    );
  }

  return { candidates, omissions, scannedCount, surfacedCount: surfaced };
}

export const adapterInfo = {
  id: ADAPTER_ID,
  layer: ADAPTER_LAYER,
  provider: SCOPE_PROVIDER,
  description: "Live files whose content or mtime has moved past the Blueprint graph generation.",
};

export const _internals = { loadGenerationMeta, walkFiles, gitIgnored };