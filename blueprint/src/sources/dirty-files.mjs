// Adapter 2 — Current dirty files and diffs.
//
// Reads `git status --porcelain` and `git diff` to surface the working-tree
// overlay that Blueprint (a generation artifact) does not know about yet.
// Per the dispatch overlay ownership: G1 detects, G3 ranks, G5 (this lane)
// produces typed candidates; this adapter never ranks — `providerScore`
// is a uniform 0.95 for tracked-overlay files and 0.7 for untracked files.
//
// The diff body is bounded (default 4000 bytes per file) so a giant WIP
// rewrite cannot inflate the packet. The truncation marker preserves
// provenance but signals honesty about the bound.
//
// Per-port porcelain status codes:
//   " M" = modified, unstaged
//   "M " = modified, staged
//   "MM" = modified in both
//   "A " = added, staged
//   "??" = untracked
//   "D " = deleted, staged
//   "R " = renamed, staged (XY form is "<orig>\0<new>")
//   "C " = copied, staged
//
// Only "diff" against HEAD is read so the candidate body reflects what the
// user actually changed versus the committed baseline, not relative to the
// index. Staged-but-not-committed and unstaged-but-tracked both surface.

import { execFileSync } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import { resolve } from "node:path";
import {
  IGNORED_DIRS,
  SCOPE_PROVIDER,
  isSupportedPath,
  makeCandidate,
  normalizePath,
  safeResolve,
  xxh3Hex,
  tryGit,
} from "./_shared.mjs";

const ADAPTER_ID = "membrane-sources/dirty-files";
const ADAPTER_LAYER = 1; // Layer 1 — live working-tree truth beats stale graph
const DEFAULT_DIFF_BYTE_CAP = 4000;
const TRUNCATION_MARKER = "\n\n[…diff truncated; per-file byte cap applied…]";

function parsePorcelain(porcelain) {
  const entries = [];
  for (const line of porcelain.split(/\r?\n/)) {
    if (!line) continue;
    if (line.length < 4) continue;
    const x = line[0];
    const y = line[1];
    let rest = line.slice(3);
    if (x === "R" || x === "C") {
      const sep = rest.indexOf("\0");
      if (sep >= 0) rest = rest.slice(sep + 1);
    }
    const path = normalizePath(rest.trim());
    if (!path) continue;
    let status;
    if (x === "?" && y === "?") status = "untracked";
    else if (x === "D" || y === "D") status = "deleted";
    else if (x === "R") status = "renamed";
    else if (x === "A") status = "added";
    else if (x === "C") status = "copied";
    else if (x === "M" && y === "M") status = "modified_both";
    else if (x === "M") status = "staged";
    else if (y === "M") status = "modified";
    else status = "other";
    entries.push({ path, status });
  }
  return entries;
}

function diffForFile(repoRoot, path, byteCap) {
  try {
    const out = execFileSync(
      "git",
      ["diff", "--no-color", "--no-ext-diff", "--", path],
      { cwd: repoRoot, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 16 * 1024 * 1024, windowsHide: true },
    );
    const text = String(out ?? "");
    if (!text) return null;
    const truncated = text.length > byteCap;
    return {
      text: truncated ? text.slice(0, byteCap) + TRUNCATION_MARKER : text,
      truncated,
      bytes: text.length,
      bodyHash: xxh3Hex(text),
    };
  } catch {
    return null;
  }
}

function scoreForStatus(status) {
  if (status === "modified" || status === "modified_both" || status === "staged") return 0.95;
  if (status === "added" || status === "renamed") return 0.92;
  if (status === "copied") return 0.85;
  if (status === "untracked") return 0.7;
  if (status === "deleted") return 0.6;
  return 0.5;
}

/**
 * @param {string} task
 * @param {{ repoRoot: string, diffByteCap?: number }} scope
 */
export function produce(task, scope) {
  const repoRoot = scope?.repoRoot;
  if (!repoRoot) return { candidates: [], omissions: [] };
  const byteCap = Number(scope?.diffByteCap) > 0 ? Number(scope.diffByteCap) : DEFAULT_DIFF_BYTE_CAP;
  const status = tryGit(repoRoot, ["status", "--porcelain", "--untracked-files=all", "--ignored=no"]);
  if (!status.ok) return { candidates: [], omissions: [] };
  const entries = parsePorcelain(status.stdout);
  const candidates = [];
  const omissions = [];

  for (const entry of entries) {
    const top = entry.path.split("/")[0];
    if (IGNORED_DIRS.has(top)) continue;
    if (!isSupportedPath(entry.path)) continue;
    const safe = safeResolve(repoRoot, entry.path);
    if (!safe) {
      omissions.push({ id: `${ADAPTER_ID}:${entry.path}`, layer: ADAPTER_LAYER, reason: "dirty_outside_scope" });
      continue;
    }
    const abs = resolve(repoRoot, safe);
    if (!existsSync(abs) && entry.status !== "deleted") {
      omissions.push({ id: `${ADAPTER_ID}:${safe}`, layer: ADAPTER_LAYER, reason: "dirty_missing" });
      continue;
    }
    let size = 0;
    try { size = statSync(abs).size; } catch { /* deleted or unreadable; still emit a thin diff */ }
    let body = null;
    if (entry.status !== "untracked" && entry.status !== "added" && entry.status !== "deleted") {
      body = diffForFile(repoRoot, safe, byteCap);
    }
    let text;
    let startLine = 1;
    let endLine = 1;
    let bytes = 0;
    if (body) {
      text = body.text;
      const lineCount = body.text.split(/\r?\n/).length;
      startLine = 1;
      endLine = lineCount;
      bytes = body.bytes;
    } else if (entry.status === "deleted") {
      text = `// dirty status: deleted\n// path: ${safe}`;
    } else if (entry.status === "added" || entry.status === "untracked") {
      text = `// dirty status: ${entry.status}\n// path: ${safe}\n// size: ${size} bytes\n// (no diff vs HEAD — file is new in the working tree)`;
    } else {
      text = `// dirty status: ${entry.status}\n// path: ${safe}`;
    }
    candidates.push(
      makeCandidate({
        id: `${ADAPTER_ID}:${safe}`,
        layer: ADAPTER_LAYER,
        sourceKind: "dirty_overlay",
        sourcePath: safe,
        trustClass: "workspace_tracked",
        providerScore: scoreForStatus(entry.status),
        scoreComponents: {
          overlay: scoreForStatus(entry.status),
          freshness: 1.0,
        },
        text,
        startLine,
        endLine,
        bodyHash: xxh3Hex(text),
        estimatedTokens: Math.max(1, bytes > 0 ? Math.round(bytes / 3.5) : endLine),
        resolver: `git status --porcelain -- ${safe}`,
      }),
    );
  }

  return { candidates, omissions };
}

export const adapterInfo = {
  id: ADAPTER_ID,
  layer: ADAPTER_LAYER,
  provider: SCOPE_PROVIDER,
  description: "Working-tree dirty files and per-file diffs versus HEAD.",
};

// Names-only summary of what the tracked-only graph omits, for the CLI dirty-tree
// warning. Shares the SAME porcelain reader/parser as produce() so the warning and
// the overlay candidates can never disagree about what "dirty" means.
//   dirtyTracked — modified/staged/renamed/copied/deleted tracked files
//   untracked    — new files not yet added (excluded by tracked-only indexing)
// A file is in exactly one bucket. Ignored files are never listed.
export function workingTreeSummary(repoRoot) {
  const status = tryGit(repoRoot, ["status", "--porcelain", "--untracked-files=all", "--ignored=no"]);
  if (!status.ok) return { available: false, dirtyTracked: [], untracked: [] };
  const dirtyTracked = [];
  const untracked = [];
  for (const entry of parsePorcelain(status.stdout)) {
    (entry.status === "untracked" ? untracked : dirtyTracked).push(entry.path);
  }
  return { available: true, dirtyTracked: dirtyTracked.sort(), untracked: untracked.sort() };
}

export const _internals = { parsePorcelain, diffForFile, scoreForStatus };