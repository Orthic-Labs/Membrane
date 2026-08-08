// MBR-903: pure release-identity primitives for the multi-platform pipeline.
//
// This module never builds, signs, or publishes anything. It only (a)
// canonically hashes a release identity so the same inputs always produce
// the same "release generation" digest, byte for byte, and (b) reads the
// current source commit/tree with read-only `git` calls (no mutation).
//
// "Reproducible" for MBR-903 means: given the same product, vector-dispatch
// marker, commit, tree, version, and target set, computeReleaseGeneration
// returns the identical digest every time, on either machine. Any drift in
// any one of those inputs -- e.g. a Windows checkout that has not pulled the
// commit the macOS build sealed -- changes the digest, and callers
// (multi-platform-release.mjs) fail closed on that mismatch instead of
// silently accepting mismatched platform artifacts as "the same release."
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";

export const VECTOR_DISPATCH = "CRYPT_VECTOR_DISPATCH_V2";
export const HEX40 = /^[0-9a-f]{40}$/;
export const HEX64 = /^[0-9a-f]{64}$/;
export const SEMVER = /^\d+\.\d+\.\d+$/;
export const TAG = /^v\d+\.\d+\.\d+$/;

const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };

// Deterministic canonical JSON: recursively sorts object keys so the same
// logical value always serializes to the same bytes, regardless of
// construction order. Arrays keep their given order (order is meaningful
// for `targets`).
export function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value && typeof value === "object") {
    const sorted = {};
    for (const key of Object.keys(value).sort()) sorted[key] = canonicalize(value[key]);
    return sorted;
  }
  return value;
}

export function canonicalJson(value) {
  return JSON.stringify(canonicalize(value));
}

/**
 * Binds product + vector-dispatch marker + source commit/tree + version +
 * the exact ordered target set into one sha256 digest: the "release
 * generation." Two calls with identical inputs always return the identical
 * digest (reproducibility). Changing any one input -- most importantly
 * `tree`, which is what actually diverges when a machine has not pulled the
 * intended commit -- changes the digest (drift is observable, not hidden).
 */
export function computeReleaseGeneration({ product, vectorDispatch, commit, tree, version, targets }) {
  if (typeof product !== "string" || !product) fail("product is required");
  if (vectorDispatch !== VECTOR_DISPATCH) fail(`vectorDispatch must be ${VECTOR_DISPATCH}`);
  if (!HEX40.test(commit ?? "")) fail("commit must be a 40-character lowercase git SHA");
  if (!HEX40.test(tree ?? "") && !HEX64.test(tree ?? "")) fail("tree must be a lowercase git SHA");
  if (!SEMVER.test(version ?? "")) fail("version must be semver X.Y.Z");
  if (!Array.isArray(targets) || targets.length === 0) fail("targets must be a non-empty array");
  for (const target of targets) if (typeof target !== "string" || !target) fail("every target must be a non-empty string");
  const payload = canonicalJson({ product, vectorDispatch, commit, tree, version, targets });
  return createHash("sha256").update(payload).digest("hex");
}

function git(cwd, args) {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  if (result.status !== 0) fail(`git ${args.join(" ")} failed: ${(result.stderr ?? "").trim() || `exit ${result.status}`}`);
  return result.stdout.trim();
}

/**
 * Read-only source identity for the current worktree: HEAD commit, the tree
 * that commit records, and whether the worktree is clean. Every `git`
 * subcommand here is informational (rev-parse, status --porcelain); none
 * mutate history, refs, the index, or the working tree.
 */
export function resolveGitSourceIdentity({ cwd = process.cwd() } = {}) {
  const commit = git(cwd, ["rev-parse", "HEAD"]);
  const tree = git(cwd, ["rev-parse", "HEAD^{tree}"]);
  const status = git(cwd, ["status", "--porcelain=v1", "--untracked-files=all"]);
  if (!HEX40.test(commit)) fail("git rev-parse HEAD did not return a 40-character SHA");
  if (!HEX40.test(tree)) fail("git rev-parse HEAD^{tree} did not return a 40-character SHA");
  return { commit, tree, dirty: status.length > 0 };
}
