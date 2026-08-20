// Pure release-identity primitives shared by preserved OCI/npm consumers.
// This module never builds, signs, uploads, or writes release evidence.
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";

export const VECTOR_DISPATCH = "CORTEX_VECTOR_DISPATCH_V2";
export const HEX40 = /^[0-9a-f]{40}$/;
export const HEX64 = /^[0-9a-f]{64}$/;
export const SEMVER = /^\d+\.\d+\.\d+$/;
export const TAG = /^v\d+\.\d+\.\d+$/;

const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };

export function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value && typeof value === "object") {
    const sorted = {};
    for (const key of Object.keys(value).sort()) sorted[key] = canonicalize(value[key]);
    return sorted;
  }
  return value;
}

export function canonicalJson(value) { return JSON.stringify(canonicalize(value)); }

export function computeReleaseGeneration({ product, vectorDispatch, commit, tree, version, targets }) {
  if (typeof product !== "string" || !product) fail("product is required");
  if (vectorDispatch !== VECTOR_DISPATCH) fail(`vectorDispatch must be ${VECTOR_DISPATCH}`);
  if (!HEX40.test(commit ?? "")) fail("commit must be a 40-character lowercase git SHA");
  if (!HEX40.test(tree ?? "") && !HEX64.test(tree ?? "")) fail("tree must be a lowercase git SHA");
  if (!SEMVER.test(version ?? "")) fail("version must be semver X.Y.Z");
  if (!Array.isArray(targets) || targets.length === 0) fail("targets must be a non-empty array");
  for (const target of targets) if (typeof target !== "string" || !target) fail("every target must be a non-empty string");
  return createHash("sha256").update(canonicalJson({ product, vectorDispatch, commit, tree, version, targets })).digest("hex");
}

export function releaseId({ app, version, commit }) {
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(app ?? "")) fail("app must be a safe release-id component");
  if (!SEMVER.test(version ?? "")) fail("version must be semver X.Y.Z");
  if (!HEX40.test(commit ?? "")) fail("commit must be a 40-character git SHA");
  return `${app}-${version}-${commit.slice(0, 8)}`;
}

function git(cwd, args) {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  if (result.status !== 0) fail(`git ${args.join(" ")} failed: ${(result.stderr ?? "").trim() || `exit ${result.status}`}`);
  return result.stdout.trim();
}

export function resolveGitSourceIdentity({ cwd = process.cwd() } = {}) {
  const commit = git(cwd, ["rev-parse", "HEAD"]);
  const tree = git(cwd, ["rev-parse", "HEAD^{tree}"]);
  const status = git(cwd, ["status", "--porcelain=v1", "--untracked-files=all"]);
  if (!HEX40.test(commit)) fail("git rev-parse HEAD did not return a 40-character SHA");
  if (!HEX40.test(tree)) fail("git rev-parse HEAD^{tree} did not return a 40-character SHA");
  return { commit, tree, dirty: status.length > 0 };
}
