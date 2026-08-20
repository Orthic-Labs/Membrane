// Adapter 1 — Explicit task anchors.
//
// The user's task string often names paths, symbols, or line ranges
// directly: "fix src/routes/place.ts#onCall", "audit scanSources in
// static-provider.mjs", "review docs/plans/2026-07-12-*.md". These named
// anchors outrank every fuzzy candidate downstream because they are explicit
// user intent.
//
// This adapter ONLY emits candidates for anchors that resolve inside the
// granted scope. Anything that points outside the scope is recorded as an
// `omission` returned to the planner — never silently demoted, never
// re-rooted.
//
// Anchor kinds recognised here:
//   - workspace-relative paths ("src/foo.ts", "tools/lib/x.mjs", "docs/...")
//   - absolute paths inside the granted root
//   - "path:line-line" ranges ("src/foo.ts:10-24")
//   - "path#symbol" names ("static-provider.mjs#scanSources")
//   - bare trailing tokens like ".ts", ".mjs", ".md" that the path
//     regex would otherwise miss
//
// This adapter does NOT call into Cortex; it only inspects the task
// string and the filesystem. Symbol resolution is handled by `graph-resolve.mjs`.

import { existsSync, statSync } from "node:fs";
import { resolve } from "node:path";
import {
  SCOPE_PROVIDER,
  isSupportedPath,
  makeCandidate,
  normalizePath,
  readFileBounded,
  safeResolve,
  xxh3Hex,
} from "./_shared.mjs";

const ADAPTER_ID = "membrane-sources/task-anchor";
const ADAPTER_LAYER = 1; // Layer 1 — explicit user intent (anchors)

const PATH_LIKE_RE =
  /(?:^|[\s,(])([A-Za-z0-9_./-]+?\.(?:ts|tsx|js|jsx|mjs|cjs|json|jsonl|ya?ml|toml|md|markdown|html|css|svg|sh|ps1|bat|sql|csv|tsv|xml))(?:[:#](\S+))?/g;
const RANGE_RE = /^(\d+)-(\d+)$/;
const SYMBOL_RE = /^([A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*)$/;

function uniqueOrdered(items) {
  const seen = new Set();
  const out = [];
  for (const item of items) {
    const key = `${item.path}|${item.startLine}|${item.endLine}|${item.kind}|${item.symbol ?? ""}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(item);
  }
  return out;
}

function parseAnchors(task) {
  if (!task) return [];
  const matches = [...String(task).matchAll(PATH_LIKE_RE)];
  const anchors = [];
  for (const match of matches) {
    const rawPath = match[1];
    const tail = match[2] ?? null;
    let kind = "file";
    let startLine = 1;
    let endLine = null;
    let symbol = null;
    if (tail) {
      if (tail.includes("#")) {
        const [range, sym] = tail.split("#", 2);
        symbol = sym || null;
        const rangeMatch = range.match(RANGE_RE);
        if (rangeMatch) {
          startLine = Number(rangeMatch[1]);
          endLine = Number(rangeMatch[2]);
          kind = "range";
        }
      } else {
        const rangeMatch = tail.match(RANGE_RE);
        if (rangeMatch) {
          startLine = Number(rangeMatch[1]);
          endLine = Number(rangeMatch[2]);
          kind = "range";
        } else if (SYMBOL_RE.test(tail)) {
          symbol = tail;
          kind = "symbol";
        }
      }
    }
    anchors.push({
      path: normalizePath(rawPath),
      kind,
      startLine,
      endLine,
      symbol,
    });
  }
  return uniqueOrdered(anchors);
}

function anchoredBody(repoRoot, anchor) {
  const abs = resolve(repoRoot, anchor.path);
  if (!existsSync(abs)) return null;
  let stat;
  try {
    stat = statSync(abs);
  } catch {
    return null;
  }
  if (!stat.isFile() || stat.size > 2 * 1024 * 1024) return null;
  const fullText = readFileBounded(abs, 2 * 1024 * 1024);
  if (!fullText) return null;
  const lines = fullText.text.split(/\r?\n/);
  const startIdx = Math.max(0, anchor.startLine - 1);
  const endIdx = anchor.endLine ? Math.min(lines.length, anchor.endLine) : Math.min(lines.length, startIdx + 80);
  const snippet = lines.slice(startIdx, endIdx).join("\n");
  const header = anchor.symbol ? `// anchor symbol: ${anchor.symbol}\n` : "";
  return {
    text: header + snippet,
    startLine: anchor.startLine,
    endLine: anchor.endLine ?? Math.min(lines.length, startIdx + 80),
    bytes: fullText.bytes,
  };
}

/**
 * @param {string} task - free-form task string
 * @param {{ repoRoot: string }} scope - granted scope
 * @returns {{ candidates: object[], omissions: object[] }}
 */
export function produce(task, scope) {
  const repoRoot = scope?.repoRoot;
  if (!repoRoot) return { candidates: [], omissions: [] };
  const anchors = parseAnchors(task);
  const candidates = [];
  const omissions = [];

  for (const anchor of anchors) {
    if (!isSupportedPath(anchor.path)) continue;
    const safe = safeResolve(repoRoot, anchor.path);
    if (!safe) {
      omissions.push({
        id: `${ADAPTER_ID}:${anchor.path}`,
        layer: ADAPTER_LAYER,
        reason: "anchor_outside_scope",
      });
      continue;
    }
    const resolved = resolve(repoRoot, safe);
    if (!existsSync(resolved)) {
      omissions.push({
        id: `${ADAPTER_ID}:${safe}`,
        layer: ADAPTER_LAYER,
        reason: "anchor_missing",
      });
      continue;
    }
    const body = anchoredBody(repoRoot, anchor);
    if (!body) {
      omissions.push({
        id: `${ADAPTER_ID}:${safe}`,
        layer: ADAPTER_LAYER,
        reason: "anchor_unreadable",
      });
      continue;
    }
    candidates.push(
      makeCandidate({
        id: `${ADAPTER_ID}:${safe}:${body.startLine}-${body.endLine}${anchor.symbol ? `#${anchor.symbol}` : ""}`,
        layer: ADAPTER_LAYER,
        sourceKind: anchor.kind === "symbol" ? "task_anchor_symbol" : "task_anchor_path",
        sourcePath: safe,
        trustClass: "user_direct",
        providerScore: 1.0,
        scoreComponents: { anchor: 1.0, explicit: 1.0 },
        text: body.text,
        startLine: body.startLine,
        endLine: body.endLine,
        bodyHash: xxh3Hex(body.text),
        estimatedTokens: Math.max(1, body.endLine - body.startLine + 1),
        resolver: `membrane sources read --anchor ${safe}`,
      }),
    );
  }

  return { candidates, omissions };
}

export const adapterInfo = {
  id: ADAPTER_ID,
  layer: ADAPTER_LAYER,
  provider: SCOPE_PROVIDER,
  description: "Explicit user task anchors: paths, ranges, and symbols named in the task string.",
};

export const _internals = { parseAnchors, anchoredBody };