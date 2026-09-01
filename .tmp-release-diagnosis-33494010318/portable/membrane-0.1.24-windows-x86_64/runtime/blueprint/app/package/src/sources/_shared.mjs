// Shared helpers for the G5 Lane C source adapters.
//
// Every adapter in this directory exports a `produce(task, scope)` returning
// an array of `ContextCandidate` records. Adapters MUST NOT perform final
// ranking, budgeting, or contradiction resolution — those belong to the
// planner (G3) and the integration owner. Adapters DO produce evidence,
// provenance, scope bounds, and a per-record providerScore so the planner
// has everything it needs to rank downstream.
//
// All paths emitted as `sourceRef` are repo-relative, forward-slash,
// normalized, and bounded by the supplied `scope.repoRoot`. Adapters must
// refuse out-of-scope paths so the planner's `ScopeGrant` denial test
// cannot be defeated by an adapter that smuggles a foreign root in.

import { randomUUID } from "node:crypto";
import { createXXHash128 } from "hash-wasm";
import { execFileSync } from "node:child_process";
import {
  existsSync,
  readFileSync,
  statSync,
} from "node:fs";
import { isAbsolute, relative, resolve, sep } from "node:path";

// Mirrors `static-provider.mjs` IGNORED set. Kept in sync manually — the
// adapter package does not depend on the static provider's internal symbols,
// but the spirit of the exclusion is identical: no caches, no build outputs,
// no portable-contract artifacts, no worktree metadata.
const IGNORED_DIRS = new Set([
  ".agent",
  ".audit",
  ".cache",
  ".git",
  ".next",
  ".nuxt",
  ".output",
  ".parcel-cache",
  ".pytest_cache",
  ".svelte-kit",
  ".turbo",
  ".vercel",
  ".worktrees",
  "node_modules",
  "target",
  "dist",
  "build",
  "out",
  "coverage",
]);

export { IGNORED_DIRS };

const FILE_ONLY_EXTENSIONS = new Set([
  "md", "markdown", "txt",
  "json", "jsonl", "yaml", "yml", "toml",
  "html", "css", "svg",
  "sh", "ps1", "bat",
  "sql", "csv", "tsv",
  "xml",
]);

export const SUPPORTED_EXTENSIONS = new Set([
  "ts", "tsx", "js", "jsx", "mjs", "cjs",
  ...FILE_ONLY_EXTENSIONS,
]);

export const SCOPE_PROVIDER = "membrane-sources";

// Default byte cap applied to any single rule/document candidate. The plan
// locks this at 1500; long rules still surface as candidates but the body is
// truncated to keep the packet honest about its own size.
export const DEFAULT_RULE_BYTE_CAP = 1500;

export const TRUNCATION_MARKER = "\n\n[…truncated; per-rule byte cap applied…]";

export function normalizePath(value) {
  return String(value ?? "").replaceAll("\\", "/").replace(/^\.\//, "");
}

export function safeResolve(repoRoot, candidate) {
  if (!candidate) return null;
  const absolute = isAbsolute(candidate)
    ? resolve(candidate)
    : resolve(repoRoot, candidate);
  const rel = normalizePath(relative(repoRoot, absolute));
  if (rel.startsWith("..") || isAbsolute(rel)) return null;
  return rel;
}

export function withinScope(repoRoot, candidate) {
  if (!candidate) return false;
  const rel = safeResolve(repoRoot, candidate);
  return Boolean(rel);
}

export function isSupportedPath(path) {
  if (!path || path.endsWith("/")) return false;
  const top = path.split("/")[0];
  if (IGNORED_DIRS.has(top)) return false;
  const dot = path.lastIndexOf(".");
  if (dot < 0) return false;
  const ext = path.slice(dot + 1).toLowerCase();
  return SUPPORTED_EXTENSIONS.has(ext);
}

// XXH3-128 (via hash-wasm) — content hashing for candidate dedup/provenance
// only, not integrity. hash-wasm's own constructor is async (WASM init), but
// init()/update()/digest() are synchronous once the hasher exists, so the
// async cost is paid exactly once at module load (top-level await) and every
// call site below stays synchronous — no per-call Promise, no blocking
// busy-wait.
//
// The hash here is XXH3-128 and the names now say so. They previously read
// `sha256Hex`/`bodySha256` long after the algorithm had moved, because the task
// that migrated the algorithm owned only this file and could not rename the six
// importing siblings. Names that lie about their algorithm are how a digest
// mismatch becomes invisible, so the rename was completed across every call
// site (2026-07-26). The self-describing `xxh128:` tag on `sourceHash` below is
// what consumers actually branch on; the contract schema accepts both that and
// the legacy `sha256:` form, so older artifacts still validate.
const xxhasher = await createXXHash128();

export function xxh3Hex(value) {
  xxhasher.init();
  xxhasher.update(String(value ?? ""));
  return xxhasher.digest("hex");
}

export function traceId() {
  return randomUUID();
}

/**
 * Build a `ContextCandidate` record. `id` is namespaced by the adapter so the
 * planner can attribute provenance and the test suite can assert ownership.
 */
export function makeCandidate({
  id,
  layer,
  sourceKind,
  sourceRef,
  sourcePath,
  trustClass,
  providerScore,
  scoreComponents,
  text,
  estimatedTokens,
  protected: isProtected = false,
  exact = true,
  recoverable = true,
  resolver,
  instructionPolicy = "data_only",
  startLine = 1,
  endLine,
  bodyHash,
  evidenceKind = "file",
}) {
  const computedEndLine = endLine ?? Math.max(1, (text?.split(/\r?\n/).length ?? 1));
  const computedTokens = estimatedTokens ?? Math.max(1, computedEndLine - startLine + 1);
  const computedBodySha = bodyHash ?? xxh3Hex(text ?? sourceRef);
  return {
    id,
    layer,
    sourceKind,
    sourceRef: sourcePath
      ? `${sourcePath}:${startLine}-${computedEndLine}`
      : sourceRef,
    sourceHash: `xxh128:${computedBodySha}`,
    trustClass,
    instructionPolicy,
    providerScore: Math.max(0, Math.min(1, Number(providerScore ?? 0))),
    scoreComponents: scoreComponents ?? {},
    estimatedTokens: computedTokens,
    protected: Boolean(isProtected),
    exact: Boolean(exact),
    recoverable: Boolean(recoverable),
    resolver: resolver ?? `membrane sources resolve --id ${id}`,
    text: text ?? "",
  };
}

export function readFileBounded(absolutePath, byteCap) {
  if (!existsSync(absolutePath)) return null;
  const stat = statSync(absolutePath);
  if (stat.size > 2 * 1024 * 1024) return null;
  const body = readFileSync(absolutePath, "utf8");
  const truncated = body.length > byteCap;
  const text = truncated ? body.slice(0, byteCap) + TRUNCATION_MARKER : body;
  return {
    text,
    truncated,
    bytes: stat.size,
    lines: text.split(/\r?\n/).length,
    bodyHash: xxh3Hex(text),
  };
}

export function tryGit(repoRoot, args) {
  try {
    const out = execFileSync("git", args, {
      cwd: repoRoot,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      maxBuffer: 16 * 1024 * 1024,
      windowsHide: true,
    });
    return { ok: true, stdout: String(out ?? "") };
  } catch (error) {
    return { ok: false, error: error?.message ?? String(error), stdout: "" };
  }
}

export function gitPath(repoRoot) {
  return resolve(repoRoot, ".git");
}

export function isGitRepo(repoRoot) {
  if (!repoRoot) return false;
  try {
    return existsSync(gitPath(repoRoot));
  } catch {
    return false;
  }
}

/**
 * Token estimate: a token is roughly four source characters for prose, but
 * source code and JSON are denser. Use a conservative 3.5 char/token estimate.
 */
export function estimateTokens(text) {
  const chars = String(text ?? "").length;
  return Math.max(1, Math.round(chars / 3.5));
}

export function joinPaths(parts) {
  return parts.filter(Boolean).join(sep).replaceAll("\\", "/");
}

/**
 * The per-rule byte cap can be overridden on a scope basis. Default is the
 * plan-locked 1500; tests and the integration owner may widen or narrow.
 */
export function resolveRuleByteCap(scope) {
  const raw = scope?.ruleByteCap ?? scope?.rule?.byteCap;
  const parsed = Number(raw);
  if (Number.isFinite(parsed) && parsed >= 64) return Math.floor(parsed);
  return DEFAULT_RULE_BYTE_CAP;
}