#!/usr/bin/env node
// MBR-903: reproducible multi-platform release orchestration.
//
// This is the manually invoked "release command for a tagged source
// revision" MBR-903's acceptance names. It does NOT build, sign, notarize,
// harden, upload, or publish anything -- that is RightKit's job, already
// wired into apps/membrane-hub via `pnpm release:doctor` / `release:build:mac`
// / `release:build:win` (see apps/membrane-hub/right-release.config.mjs).
// This script only:
//
//   1. resolves the exact source identity (commit, tree) for the tagged
//      revision, read-only, via git rev-parse (never a mutating git call);
//   2. computes one deterministic "release generation" digest binding
//      product + CRYPT_VECTOR_DISPATCH_V2 + commit + tree + version + the
//      ordered target set (identity.mjs's computeReleaseGeneration);
//   3. for each of the four logical targets (release/contracts/platforms.v1.json),
//      reads whatever RightKit has already sealed on THIS machine for THIS
//      commit (sealed-platforms.mjs, itself reusing scripts/release/macos/contract.mjs
//      and scripts/release/windows/contract.mjs -- never re-implementing them);
//   4. writes ONE immutable, cross-platform record binding all of that to
//      evidence/releases/<releaseId>/release-generation.json.
//
// Reproducibility is provable, not asserted: given the same commit, tree,
// version, and target set, step 2's digest is byte-identical on any
// machine, and step 4's write is idempotent -- writing the identical plan
// twice succeeds silently; writing a DIFFERENT plan to the same releaseId
// (the drift case: e.g. a Windows sealed manifest built from a commit the
// Mac side did not intend) throws and refuses to overwrite. See
// pipeline.test.mjs and docs/release/pipeline.md.
//
// Adrian's real workflow: finish on the Mac (seals `.right-release/sealed/<releaseId>/mac/`),
// pull to Windows, build there (seals `.../windows/`). This script is run
// once per machine, after each platform's RightKit build; each run reads
// what exists so far and (re)writes the same immutable record, so the
// second (Windows) run's output for shared fields is identical to the
// first (Mac) run's, and only the platform stage that machine can see
// changes from "pending" to "verified" or "not-buildable".
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { VECTOR_DISPATCH, HEX40, HEX64, SEMVER, TAG, computeReleaseGeneration, resolveGitSourceIdentity } from "./identity.mjs";
import { targetsInOrder, PIPELINE } from "./platform-targets.mjs";
import { readAllSealedStages } from "./sealed-platforms.mjs";

const PRODUCT = PIPELINE.product ?? "Membrane";
const SCHEMA = "orthic.membrane.release-generation.v1";
const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };

export function releaseId({ app, version, commit }) {
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(app ?? "")) fail("app must be a safe release-id component");
  if (!SEMVER.test(version ?? "")) fail("version must be semver X.Y.Z");
  if (!HEX40.test(commit ?? "")) fail("commit must be a 40-character git SHA");
  return `${app}-${version}-${commit.slice(0, 8)}`;
}

/**
 * Builds the full multi-platform release plan for one tagged revision.
 * Never builds/signs/publishes; only resolves identity and reads sealed
 * state already on disk. `targets` defaults to every logical target in
 * release/contracts/platforms.v1.json, in its declared order, so the
 * emitted release_generation binds the same ordered set every time.
 */
export function buildMultiPlatformPlan({ repoRoot, app, version, tag, commit, tree, targets = targetsInOrder() }) {
  if (!TAG.test(tag ?? "")) fail("tag must be immutable semver tag vX.Y.Z");
  if (!SEMVER.test(version ?? "")) fail("version must be semver X.Y.Z");
  if (!HEX40.test(commit ?? "")) fail("commit must be a 40-character git SHA");
  if (!HEX40.test(tree ?? "") && !HEX64.test(tree ?? "")) fail("tree must be a lowercase git SHA");
  if (!Array.isArray(targets) || targets.length === 0) fail("targets must be a non-empty array");

  const release_generation = computeReleaseGeneration({
    product: PRODUCT, vectorDispatch: VECTOR_DISPATCH, commit, tree, version, targets,
  });
  const id = releaseId({ app, version, commit });
  const platformStages = readAllSealedStages({ repoRoot, app, version, commit }, targets);

  return {
    schema: SCHEMA,
    product: PRODUCT,
    vector_dispatch: VECTOR_DISPATCH,
    publish: false,
    release: { app, version, tag, commit, tree, release_generation, releaseId: id },
    targets: platformStages,
  };
}

const IMMUTABLE_COMPARE_KEYS = ["schema", "product", "vector_dispatch", "publish", "release", "targets"];

function stableForCompare(plan) {
  const picked = {};
  for (const key of IMMUTABLE_COMPARE_KEYS) picked[key] = plan[key];
  return JSON.stringify(picked);
}

export class ImmutableReleaseConflictError extends Error {
  constructor(message, path) {
    super(message);
    this.name = "ImmutableReleaseConflictError";
    this.path = path;
  }
}

/**
 * Writes evidence/releases/<releaseId>/release-generation.json. Write-once
 * per releaseId: an identical plan written twice succeeds idempotently
 * (proves reproducibility across repeated runs / across the Mac-then-Windows
 * workflow); a plan that differs from what is already on disk for the same
 * releaseId throws ImmutableReleaseConflictError instead of overwriting
 * (drift is caught, never silently accepted).
 */
export function writeImmutableReleaseGeneration({ repoRoot, plan, evidenceRoot }) {
  const root = evidenceRoot ?? resolve(repoRoot, "evidence", "releases");
  const dir = resolve(root, plan.release.releaseId);
  const file = resolve(dir, "release-generation.json");
  const next = stableForCompare(plan);
  if (existsSync(file)) {
    const existing = JSON.parse(readFileSync(file, "utf8"));
    if (stableForCompare(existing) !== next) {
      throw new ImmutableReleaseConflictError(
        `FAIL CLOSED: release generation already recorded at ${file} and differs from this run's plan; a sealed release identity must never be rewritten`,
        file,
      );
    }
    return { path: file, written: false };
  }
  mkdirSync(dir, { recursive: true });
  writeFileSync(file, `${JSON.stringify(plan, null, 2)}\n`);
  return { path: file, written: true };
}

function parseArgs(argv) {
  const args = { repoRoot: process.cwd(), write: false, app: "membrane-hub" };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--tag") args.tag = argv[++i];
    else if (arg === "--app") args.app = argv[++i];
    else if (arg === "--version") args.version = argv[++i];
    else if (arg === "--commit") args.commit = argv[++i];
    else if (arg === "--tree") args.tree = argv[++i];
    else if (arg === "--repo-root") args.repoRoot = argv[++i];
    else if (arg === "--write") args.write = true;
    else if (arg === "-h" || arg === "--help") { args.help = true; }
    else fail(`unknown argument: ${arg}`);
  }
  return args;
}

function usage() {
  console.log(
    "usage: multi-platform-release.mjs --tag vX.Y.Z --version X.Y.Z [--app membrane-hub] [--commit SHA --tree SHA] [--repo-root PATH] [--write]\n\n" +
    "Prints the multi-platform release plan for the given tag. Without --commit/--tree,\n" +
    "resolves them from the current git HEAD (read-only). Without --write, the plan is\n" +
    "printed only; with --write, an immutable evidence/releases/<releaseId>/release-generation.json\n" +
    "is written (or verified unchanged if it already exists). This command never builds,\n" +
    "signs, notarizes, uploads, or publishes -- run pnpm release:doctor / release:build:mac /\n" +
    "release:build:win from apps/membrane-hub for that, on each platform's own machine.",
  );
}

function cli() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help || !args.tag || !args.version) { usage(); process.exit(args.help ? 0 : 2); }
  try {
    const repoRoot = resolve(args.repoRoot);
    const { commit, tree } = args.commit && args.tree ? { commit: args.commit, tree: args.tree } : resolveGitSourceIdentity({ cwd: repoRoot });
    const plan = buildMultiPlatformPlan({ repoRoot, app: args.app, version: args.version, tag: args.tag, commit, tree });
    if (args.write) {
      const result = writeImmutableReleaseGeneration({ repoRoot, plan });
      console.error(`[multi-platform-release] ${result.written ? "wrote" : "unchanged (already recorded)"}: ${result.path}`);
    }
    process.stdout.write(`${JSON.stringify(plan, null, 2)}\n`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) cli();
