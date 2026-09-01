// Adapter 3 — Exact paths and symbols via Blueprint resolve.
//
// Resolves named paths and symbols against the existing Blueprint graph.
// This adapter does NOT regenerate the graph; it consumes a `generation`
// (or lazily rebuilds via the portable contract) and uses Blueprint's
// `resolveGraphNode` / `queryGraph` to produce exact candidates.
//
// Resolution rules:
//   - If the task names a path that exists in the graph as a `file:*` node,
//     emit a `graph_resolve_file` candidate with full evidence.
//   - If the task names a symbol via "path#symbol" or a bare symbol token
//     that matches `symbol:path::qualName`, emit a `graph_resolve_symbol`
//     candidate with start/end lines from the graph evidence.
//   - For symbol-only tokens, fall back to `queryGraph` with the symbol
//     as the query (limit ≤ 5) and emit only those whose qualifiedName
//     matches the token exactly.
//
// If no Blueprint graph is available (no `.agent/manifest.json`, no
// `.agent/manifest.json`), this adapter emits a single omission with
// reason `graph_unavailable` and returns. It does not synthesise candidates
// from raw source scans — that is the live-overlay adapter's job.

import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { openStoreReadOnly, closeStore, loadGeneration as loadStoredGeneration } from "../graph/store-sqlite.mjs";
import {
  SCOPE_PROVIDER,
  isSupportedPath,
  makeCandidate,
  normalizePath,
  safeResolve,
  xxh3Hex,
} from "./_shared.mjs";
import {
  sliceTokens,
  resolveTokens,
  trimToSymbol,
  applyTrimToSymbol,
  truncationReceipt,
} from "../lib/token-budget.mjs";

// Phase 7.4 — wraps `_shared.makeCandidate` so every emitted candidate
// carries both budgets and a truncation receipt (when actual bytes were
// trimmed). Falls through unchanged when bytes are not supplied, so
// adapters that still prefer the line-count proxy lose nothing.
//
// Inputs:
//   actualText: bytes the resolver actually retained (post-trim).
//   originalText: bytes the resolver would have emitted if no trim fired.
//                  Used to drive the receipt's `originalBytes` so the
//                  planner can see the full byte delta, not just the
//                  kept count.
function makeResolvedCandidate({
  id,
  layer,
  sourceKind,
  sourcePath,
  trustClass,
  providerScore,
  scoreComponents,
  text,
  startLine,
  endLine,
  bodyHash,
  estimatedTokens,
  resolver,
  actualText = null,
  originalText = null,
  actualStartLine = null,
  actualEndLine = null,
  truncationReason = null,
  symbolBoundary = false,
}) {
  const baseEstimate = typeof estimatedTokens === "number" && estimatedTokens > 0
    ? estimatedTokens
    : sliceTokens({ startLine, endLine });
  const result = makeCandidate({
    id,
    layer,
    sourceKind,
    sourcePath,
    trustClass,
    providerScore,
    scoreComponents,
    text,
    startLine,
    endLine,
    bodyHash,
    estimatedTokens: baseEstimate,
    resolver,
  });
  if (actualText == null) {
    result.actualTokens = null;
    result.truncation = null;
    return result;
  }
  const resolved = resolveTokens(actualText, {
    startLine: actualStartLine ?? startLine,
    endLine: actualEndLine ?? endLine,
  });
  // Two distinct budgets, both surfaced. The resolved estimate is the
  // byte-based one; the slice estimate is the metadata one. They can
  // disagree when trim-to-symbol rescales the span or a byte cap dropped
  // bytes — that disagreement IS the receipt the planner audits against.
  result.actualTokens = resolved.tokens;
  if (truncationReason && originalText != null) {
    const original = resolveTokens(originalText, {
      startLine: actualStartLine ?? startLine,
      endLine: actualEndLine ?? endLine,
    });
    try {
      result.truncation = truncationReceipt(resolved, original, {
        reason: truncationReason,
        symbolBoundary,
      });
    } catch {
      // Caller did not actually drop bytes (e.g. snap-to-symbol without
      // byte cap). Surface null instead of fabricating a misleading
      // receipt.
      result.truncation = null;
    }
  } else {
    result.truncation = null;
  }
  return result;
}

// Phase 7.4 — apply trim-to-symbol when a path is supplied. Reads the file,
// snaps the requested span to the enclosing brace boundaries, and reports
// the kept span as the resolution-time span. `maxBytes = null` disables the
// byte-cap (use only when you intend to send the whole symbol body).
function resolveContentByPath(repoRoot, sourcePath, requestedStart, requestedEnd, options = {}) {
  const maxBytes = options.maxBytes ?? null;
  const absolute = join(repoRoot, sourcePath);
  let bytes;
  try {
    bytes = readFileSync(absolute, "utf8");
  } catch {
    return null;
  }
  const lines = bytes.split(/\r?\n/);
  if (maxBytes == null) {
    const snapped = trimToSymbol(lines, requestedStart, requestedEnd);
    const text = lines.slice(snapped.startLine - 1, snapped.endLine).join("\n");
    return {
      text,
      originalText: text,
      startLine: snapped.startLine,
      endLine: snapped.endLine,
      truncated: snapped.trimmed,
      receipt: null,
      reason: snapped.reason,
    };
  }
  // applyTrimToSymbol returns a pre-built receipt only when byte cap
  // dropped content. When the symbol already fits, `out.receipt` is null
  // but `out.text` and `out.resolve` reflect the snap-to-symbol result.
  const out = applyTrimToSymbol(lines, requestedStart, requestedEnd, { maxBytes });
  return {
    text: out.text,
    originalText: out.receipt ? out.originalText : out.text,
    startLine: out.startLine,
    endLine: out.endLine,
    truncated: Boolean(out.receipt),
    receipt: out.receipt,
    reason: out.receipt ? "byte_cap" : "no_trim_needed",
  };
}

const ADAPTER_ID = "membrane-sources/graph-resolve";
const ADAPTER_LAYER = 3; // Layer 3 — graph-backed file/symbol evidence

const SYMBOL_TOKEN_RE = /\b([A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*){0,3})\b/g;
const PATH_LIKE_RE =
  /(?:^|[\s,(`])([A-Za-z0-9_./-]+?\.(?:ts|tsx|js|jsx|mjs|cjs|json|jsonl|ya?ml|toml|md|markdown|html|css|svg|sh|ps1|bat|sql|csv|tsv|xml))(?:[:#](\S+))?/g;
const RANGE_RE = /^(\d+)-(\d+)$/;

function loadGeneration(repoRoot) {
  // Blueprint's native generation lives in the store. (The previous path here,
  // `.agent/graph.json`, was wrong even before the migration — the generation
  // was always at `.agent/graph/graph.json` — so this branch never fired and
  // every caller silently fell through to the bootstrap index.)
  const dbPath = join(repoRoot, ".agent", "graph", "graph.db");
  if (existsSync(dbPath)) {
    try {
      const db = openStoreReadOnly(dbPath);
      try {
        const generation = loadStoredGeneration(db);
        if (generation?.nodes && generation?.manifest) return generation;
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
      const raw = JSON.parse(readFileSync(path, "utf8"));
      // .agent/graph.json holds the full generation.
      if (raw?.nodes && raw?.manifest) {
        return raw;
      }
      // .agent/index.jsonl holds one node per line; assemble lazily.
      if (path.endsWith("index.jsonl")) {
        const lines = readFileSync(path, "utf8").split(/\r?\n/).filter(Boolean);
        const nodes = lines.map((line) => JSON.parse(line));
        return { nodes, edges: [], manifest: { generationId: "blueprint-portable", provider: { id: "blueprint-portable" }, generatedAt: null }, portable: true };
      }
    } catch {
      continue;
    }
  }
  return null;
}

function findFileNode(generation, repoPath) {
  const wanted = `file:${repoPath}`;
  return generation.nodes.find((node) => node.id === wanted) ?? null;
}

function findSymbolNode(generation, repoPath, symbol) {
  const needle = `symbol:${repoPath}::${symbol}`;
  return generation.nodes.find((node) => node.id === needle)
    ?? generation.nodes.find((node) => node.kind !== "file" && node.path === repoPath && node.qualifiedName === symbol)
    ?? null;
}

function lexTokens(task) {
  const seen = new Set();
  const out = [];
  for (const match of String(task ?? "").matchAll(SYMBOL_TOKEN_RE)) {
    const token = match[1];
    if (token.length < 3) continue;
    if (/^[A-Z]+$/.test(token)) continue; // skip all-caps acronyms
    if (seen.has(token)) continue;
    seen.add(token);
    out.push(token);
  }
  return out;
}

function pathAndSymbol(task) {
  const items = [];
  for (const match of String(task ?? "").matchAll(PATH_LIKE_RE)) {
    const path = normalizePath(match[1]);
    const tail = match[2] ?? null;
    let symbol = null;
    let startLine = 1;
    let endLine = null;
    if (tail && tail.includes("#")) {
      const [range, sym] = tail.split("#", 2);
      symbol = sym || null;
      const m = range.match(RANGE_RE);
      if (m) {
        startLine = Number(m[1]);
        endLine = Number(m[2]);
      }
    } else if (tail && SYMBOL_TOKEN_RE.test(tail)) {
      symbol = tail;
    }
    items.push({ path, symbol, startLine, endLine });
  }
  return items;
}

/**
 * @param {string} task
 * @param {{ repoRoot: string, generation?: object, freshGeneration?: () => object, maxSymbolHits?: number }} scope
 */
export function produce(task, scope) {
  const repoRoot = scope?.repoRoot;
  if (!repoRoot) return { candidates: [], omissions: [] };
  const generation = scope?.generation ?? (scope?.freshGeneration ? scope.freshGeneration() : null) ?? loadGeneration(repoRoot);
  if (!generation) {
    return {
      candidates: [],
      omissions: [{ id: `${ADAPTER_ID}:graph`, layer: ADAPTER_LAYER, reason: "graph_unavailable" }],
    };
  }

  const candidates = [];
  const omissions = [];
  const used = new Set();
  const maxHits = Math.max(1, Math.min(20, Number(scope?.maxSymbolHits ?? 5)));
  const items = pathAndSymbol(task);

  for (const item of items) {
    if (!isSupportedPath(item.path)) continue;
    const safe = safeResolve(repoRoot, item.path);
    if (!safe) {
      omissions.push({ id: `${ADAPTER_ID}:${item.path}`, layer: ADAPTER_LAYER, reason: "graph_resolve_outside_scope" });
      continue;
    }
    if (item.symbol) {
      const node = findSymbolNode(generation, safe, item.symbol);
      if (node) {
        const ev = node.evidence?.[0] ?? {};
        const startLine = ev.startLine ?? 1;
        const endLine = ev.endLine ?? 1;
        // Phase 7.4 — resolve the actual source bytes for this candidate.
        // Trim-to-symbol snaps the requested span to brace boundaries, and
        // any byte-cap trim carries a `truncation` receipt. When the file
        // is not readable (typical for graph-only fixtures), we fall back
        // to the slice-time-only candidate, never fabricating a receipt.
        // `scope.maxBytes` lets tests / callers force a byte cap to prove
        // the truncation receipt fires — but defaults to none so the
        // adapter never silently truncates real traffic.
        const resolution = resolveContentByPath(repoRoot, safe, startLine, endLine, {
          maxBytes: scope?.maxBytes ?? null,
        });
        candidates.push(
          makeResolvedCandidate({
            id: `${ADAPTER_ID}:${node.id}`,
            layer: ADAPTER_LAYER,
            sourceKind: "graph_resolve_symbol",
            sourcePath: safe,
            trustClass: "workspace_tracked",
            providerScore: 0.97,
            scoreComponents: { graph: 1.0, exact: 1.0 },
            text: node.qualifiedName ?? node.name ?? item.symbol,
            startLine,
            endLine,
            bodyHash: ev.contentHash ?? xxh3Hex(node.qualifiedName ?? item.symbol),
            estimatedTokens: Math.max(1, endLine - startLine + 1),
            resolver: `blueprint graph resolve --node ${node.id}`,
            actualText: resolution?.text ?? null,
            originalText: resolution?.originalText ?? null,
            actualStartLine: resolution?.startLine ?? null,
            actualEndLine: resolution?.endLine ?? null,
            truncationReason: resolution?.receipt ? resolution.reason : null,
            symbolBoundary: resolution?.truncated ?? false,
          }),
        );
        used.add(node.id);
      } else {
        omissions.push({
          id: `${ADAPTER_ID}:${safe}#${item.symbol}`,
          layer: ADAPTER_LAYER,
          reason: "graph_symbol_not_found",
        });
      }
    }
    const fileNode = findFileNode(generation, safe);
    if (fileNode) {
      const ev = fileNode.evidence?.[0] ?? {};
      const startLine = ev.startLine ?? 1;
      const endLine = ev.endLine ?? 1;
      candidates.push(
        makeResolvedCandidate({
          id: `${ADAPTER_ID}:${fileNode.id}`,
          layer: ADAPTER_LAYER,
          sourceKind: "graph_resolve_file",
          sourcePath: safe,
          trustClass: "workspace_tracked",
          providerScore: 0.9,
          scoreComponents: { graph: 1.0, exact: 1.0 },
          text: fileNode.qualifiedName ?? safe,
          startLine,
          endLine,
          bodyHash: ev.contentHash ?? xxh3Hex(safe),
          estimatedTokens: Math.max(1, endLine - startLine + 1),
          resolver: `blueprint graph resolve --node ${fileNode.id}`,
        }),
      );
      used.add(fileNode.id);
    } else {
      omissions.push({
        id: `${ADAPTER_ID}:file:${safe}`,
        layer: ADAPTER_LAYER,
        reason: "graph_file_not_found",
      });
    }
  }

  // Token-driven symbol query for symbols named without a path.
  const tokens = lexTokens(task);
  for (const token of tokens) {
    if (used.has(`symbol::${token}`)) continue;
    const hits = generation.nodes
      .filter((node) => node.kind !== "file" && (node.qualifiedName === token || node.name === token))
      .slice(0, maxHits);
    if (!hits.length) continue;
    for (const node of hits) {
      if (used.has(node.id)) continue;
      used.add(node.id);
      const ev = node.evidence?.[0] ?? {};
      const safe = safeResolve(repoRoot, node.path ?? "");
      if (!safe) {
        omissions.push({ id: `${ADAPTER_ID}:${node.id}`, layer: ADAPTER_LAYER, reason: "graph_resolve_outside_scope" });
        continue;
      }
      const startLine = ev.startLine ?? 1;
      const endLine = ev.endLine ?? 1;
      candidates.push(
        makeResolvedCandidate({
          id: `${ADAPTER_ID}:${node.id}`,
          layer: ADAPTER_LAYER,
          sourceKind: "graph_resolve_symbol",
          sourcePath: safe,
          trustClass: "workspace_tracked",
          providerScore: 0.85,
          scoreComponents: { graph: 0.9, lexical: 0.8 },
          text: node.qualifiedName ?? node.name ?? token,
          startLine,
          endLine,
          bodyHash: ev.contentHash ?? xxh3Hex(node.qualifiedName ?? token),
          estimatedTokens: Math.max(1, endLine - startLine + 1),
          resolver: `blueprint graph resolve --node ${node.id}`,
        }),
      );
    }
  }

  return { candidates, omissions };
}

export const adapterInfo = {
  id: ADAPTER_ID,
  layer: ADAPTER_LAYER,
  provider: SCOPE_PROVIDER,
  description: "Exact path and symbol resolution through Blueprint's static provider.",
};

export const _internals = { findFileNode, findSymbolNode, lexTokens, pathAndSymbol, loadGeneration, resolveContentByPath };