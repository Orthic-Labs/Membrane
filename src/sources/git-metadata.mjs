// Adapter 4 — Git branch / commit / status metadata.
//
// Emits ONE candidate per session summarising the working repository's
// Git state. This is the "where am I working" anchor the planner uses to
// rank dirty overlays and scope decisions. The candidate text is a small
// structured blob — never raw log/diff content — and the body is bounded
// so it cannot grow with repository size.
//
// Fields collected (best-effort; missing git returns no candidates):
//   - branch (current)
//   - head (short SHA)
//   - upstream + ahead/behind counts
//   - worktree status (clean / dirty / merged)
//   - most recent commit subject + author + relative time
//   - tracked dirty count vs HEAD
//
// If the repository is detached-HEAD, the branch field is recorded as
// "detached@<sha>" and the candidate carries `trustClass=workspace_tracked`
// but `instructionPolicy=data_only`.

import { execFileSync } from "node:child_process";
import {
  SCOPE_PROVIDER,
  isGitRepo,
  makeCandidate,
  xxh3Hex,
  tryGit,
} from "./_shared.mjs";

const ADAPTER_ID = "membrane-sources/git-metadata";
const ADAPTER_LAYER = 1; // Layer 1 — authoritative repository state
const DEFAULT_BODY_BYTE_CAP = 1200;
const TRUNCATION_MARKER = "\n\n[…git metadata truncated…]";

function execGit(repoRoot, args) {
  try {
    return String(execFileSync("git", args, {
      cwd: repoRoot,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      maxBuffer: 4 * 1024 * 1024,
      windowsHide: true,
    }) ?? "");
  } catch {
    return "";
  }
}

function singleLine(value) {
  return String(value ?? "").replace(/\s+/g, " ").trim();
}

function collect(repoRoot, bodyByteCap) {
  const branchRaw = execGit(repoRoot, ["rev-parse", "--abbrev-ref", "HEAD"]).trim();
  const detached = branchRaw === "HEAD";
  const branch = detached
    ? `detached@${execGit(repoRoot, ["rev-parse", "--short", "HEAD"]).trim()}`
    : branchRaw;
  const headSha = execGit(repoRoot, ["rev-parse", "--short", "HEAD"]).trim();
  const upstreamRef = branch && !detached
    ? execGit(repoRoot, ["rev-parse", "--abbrev-ref", "--symbolic-full-name", `${branch}@{upstream}`]).trim()
    : "";
  let ahead = 0;
  let behind = 0;
  if (upstreamRef && !upstreamRef.startsWith("fatal:")) {
    const countsRaw = execGit(repoRoot, ["rev-list", "--left-right", "--count", `${upstreamRef}...HEAD`]).trim();
    const m = countsRaw.match(/^(\d+)\s+(\d+)$/);
    if (m) {
      ahead = Number(m[2]);
      behind = Number(m[1]);
    }
  }
  const statusPorcelain = execGit(repoRoot, ["status", "--porcelain", "--untracked-files=all", "--ignored=no"]);
  const dirtyEntries = statusPorcelain.split(/\r?\n/).filter((line) => line.trim().length >= 4);
  const dirtyCount = dirtyEntries.length;
  const worktreeStatus = dirtyCount === 0 ? "clean" : "dirty";
  const logRaw = execGit(repoRoot, [
    "log",
    "-1",
    "--pretty=format:%h%x09%an%x09%ad%x09%s",
    "--date=iso-strict",
    "HEAD",
  ]);
  const [logShort, logAuthor, logDate, logSubject] = logRaw.split("\t");
  const lines = [
    `# Git metadata`,
    `branch: ${branch || "<unknown>"}`,
    `head: ${headSha || "<unknown>"}`,
    `upstream: ${upstreamRef && !upstreamRef.startsWith("fatal:") ? upstreamRef : "<none>"}`,
    `ahead/behind: ${ahead}/${behind}`,
    `worktree: ${worktreeStatus}${dirtyCount ? ` (${dirtyCount} entries)` : ""}`,
    `last_commit: ${logShort ?? "?"} ${logAuthor ?? "?"} @ ${logDate ?? "?"}`,
    `last_subject: ${singleLine(logSubject ?? "")}`,
  ];
  let text = lines.join("\n");
  if (text.length > bodyByteCap) text = text.slice(0, bodyByteCap) + TRUNCATION_MARKER;
  return { text, branch, headSha, dirtyCount, worktreeStatus };
}

/**
 * @param {string} task
 * @param {{ repoRoot: string, bodyByteCap?: number }} scope
 */
export function produce(task, scope) {
  const repoRoot = scope?.repoRoot;
  if (!repoRoot || !isGitRepo(repoRoot)) {
    return { candidates: [], omissions: [] };
  }
  const result = tryGit(repoRoot, ["status", "--porcelain"]);
  if (!result.ok) return { candidates: [], omissions: [] };
  const bodyByteCap = Number(scope?.bodyByteCap) > 0 ? Number(scope.bodyByteCap) : DEFAULT_BODY_BYTE_CAP;
  const meta = collect(repoRoot, bodyByteCap);
  const id = `${ADAPTER_ID}:${meta.branch || "unknown"}:${meta.headSha || "unknown"}`;
  const candidate = makeCandidate({
    id,
    layer: ADAPTER_LAYER,
    sourceKind: "git_metadata",
    sourceRef: `git:${meta.branch || "detached"}@${meta.headSha || "unknown"}`,
    sourcePath: null,
    trustClass: "workspace_tracked",
    providerScore: 0.6,
    scoreComponents: { provenance: 1.0, structural: 0.5 },
    text: meta.text,
    startLine: 1,
    endLine: meta.text.split(/\r?\n/).length,
    bodyHash: xxh3Hex(meta.text),
    estimatedTokens: Math.max(1, Math.round(meta.text.length / 3.5)),
    resolver: `git status && git log -1 && git rev-parse --abbrev-ref HEAD`,
  });
  return { candidates: [candidate], omissions: [] };
}

export const adapterInfo = {
  id: ADAPTER_ID,
  layer: ADAPTER_LAYER,
  provider: SCOPE_PROVIDER,
  description: "Git branch, commit, status, and upstream metadata for the working tree.",
};

export const _internals = { collect };