// MBR-502: tiny JS bridge that captures + records a provenance row by
// driving the same three git subcommands the Rust `capture_working_tree`
// uses. The bridge accepts an injected `gitCommand` so the test runner
// can drive the bridge without a real `git` binary on the box; the
// production caller passes the default `spawnSyncGit`.
// Production code must call the Rust `observe()` entry point directly
// for the canonical writer; this bridge exists only to exercise the
// observation contract in pure JS.

import { spawnSync } from "node:child_process";
import { mkdirSync, appendFileSync } from "node:fs";
import { dirname } from "node:path";

import { provenanceRowFromJs } from "../../mcp/adapters/provenance/index.mjs";

/**
 * Default git invocation. Mirrors the Rust `default_git_command`:
 * forces `GIT_TERMINAL_PROMPT=0` and `GIT_OPTIONAL_LOCKS=0` so a
 * credential or editor prompt can never block the runtime.
 */
export const spawnSyncGit = (workspace, args) => {
  const result = spawnSync("git", args, {
    cwd: workspace,
    encoding: "utf8",
    env: { ...process.env, GIT_TERMINAL_PROMPT: "0", GIT_OPTIONAL_LOCKS: "0" },
  });
  return {
    status: result.status,
    stdout: `${result.stdout || ""}`,
    stderr: `${result.stderr || ""}`,
  };
};

/**
 * Run `git rev-parse --verify HEAD` against `workspace`. Returns `null`
 * on an unborn HEAD so the snapshot can distinguish "no commit yet"
 * from "git failed".
 */
export function headSnapshot(workspace, gitCommand = spawnSyncGit) {
  const result = gitCommand(workspace, ["rev-parse", "--verify", "HEAD"]);
  if (result.status !== 0) {
    const stderr = result.stderr || "";
    if (
      stderr.includes("unknown revision") ||
      stderr.includes("ambiguous argument 'HEAD'") ||
      stderr.includes("does not have any commits yet") ||
      stderr.includes("not a git repository")
    ) {
      return null;
    }
    throw new Error(`git rev-parse failed: ${stderr.trim()}`);
  }
  const trimmed = (result.stdout || "").trim();
  return trimmed.length === 0 ? null : trimmed.split(/\s+/)[0];
}

/**
 * Run `git status --porcelain` and return the dirty paths. Never reads
 * file bodies; only the path string after the two-character status.
 */
export function statusPaths(workspace, gitCommand = spawnSyncGit) {
  const result = gitCommand(workspace, ["status", "--porcelain"]);
  if (result.status !== 0) {
    throw new Error(`git status failed: ${(result.stderr || "").trim()}`);
  }
  return (result.stdout || "")
    .split("\n")
    .filter((line) => line.length >= 3)
    .map((line) => line.slice(3).trim())
    .filter((line) => line.length > 0);
}

/**
 * Run `git diff --numstat` and sum the added/removed columns. Binary
 * files report `-` `-`; those map to zero.
 */
export function diffNumstat(workspace, gitCommand = spawnSyncGit) {
  const result = gitCommand(workspace, ["diff", "--numstat"]);
  if (result.status !== 0) {
    throw new Error(`git diff failed: ${(result.stderr || "").trim()}`);
  }
  let added = 0;
  let removed = 0;
  for (const line of (result.stdout || "").split("\n")) {
    if (line.length === 0) continue;
    const [a, b] = line.split("\t");
    if (a === undefined || b === undefined) continue;
    added += a === "-" ? 0 : Number.parseInt(a, 10) || 0;
    removed += b === "-" ? 0 : Number.parseInt(b, 10) || 0;
  }
  return { added, removed };
}

/**
 * Bundle the three git captures into a WorkingTreeSnapshotV1-shaped
 * object. Same field names as the Rust enum, so the JSONL output is
 * wire-compatible.
 */
export function captureWorkingTreeJs(workspace, nowUnixMs, gitCommand = spawnSyncGit) {
  const gitHead = headSnapshot(workspace, gitCommand);
  const dirtyPaths = statusPaths(workspace, gitCommand);
  const { added, removed } = diffNumstat(workspace, gitCommand);
  return {
    schemaVersion: 1,
    workspaceRoot: workspace,
    gitHead,
    dirtyPaths,
    diffAdded: added,
    diffRemoved: removed,
    capturedAtUnixMs: nowUnixMs,
  };
}

/**
 * Append one row to the JSONL journal. Creates the parent directory
 * on first write so a fresh install does not need an explicit mkdir.
 */
export function recordProvenanceJs(row, storePath) {
  mkdirSync(dirname(storePath), { recursive: true });
  appendFileSync(storePath, `${JSON.stringify(row)}\n`, "utf8");
}

/**
 * Single entry point: capture + record + return. Mirrors the Rust
 * `observe()` shape so the runtime can swap implementations without
 * changing the call site.
 */
export function observeJs({
  operation,
  clientId = null,
  scopeGrant = null,
  workspace,
  installationId,
  storePath,
  nowUnixMs,
  gitCommand = spawnSyncGit,
}) {
  const workingTree = captureWorkingTreeJs(workspace, nowUnixMs, gitCommand);
  const row = provenanceRowFromJs({
    operation,
    clientId,
    scopeGrant,
    workingTree,
    installationId,
    recordedAtUnixMs: nowUnixMs,
  });
  recordProvenanceJs(row, storePath);
  return row;
}
