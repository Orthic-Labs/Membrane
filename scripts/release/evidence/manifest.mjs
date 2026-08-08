#!/usr/bin/env node
// MBR-807: binds one release_generation (MBR-903's immutable per-release
// identity, evidence/releases/<releaseId>/release-generation.json) plus
// this task's own real SBOM/provenance/toolchain evidence into an
// orthic.membrane.release-evidence.v1 manifest -- the schema and verifier
// scripts/release/verify-release-evidence.mjs already defines and enforces
// (commit 98233f35, pre-existing; imported here, never forked or modified).
//
// This module never builds, signs, notarizes, tests, or installs anything.
// For one releaseId + target it:
//   1. reads evidence/releases/<releaseId>/release-generation.json as the
//      SOLE source of release identity (tag, commit, tree, release_generation,
//      per-target sealed-artifact status). It never recomputes a second,
//      parallel release_generation -- doing so would let this manifest's
//      identity silently drift from MBR-903's, exactly the failure mode
//      MBR-903 itself exists to catch across machines.
//   2. generates this task's own real evidence -- the SBOM (sbom.mjs,
//      derived only from committed lockfiles) and read-only source
//      provenance/toolchain records (git show/package.json/Cargo.toml,
//      never an invoked compiler or package manager) -- and writes them
//      under that same evidence/releases/<releaseId>/ directory, hash-bound
//      exactly the way verify-release-evidence.mjs recomputes hashes.
//   3. reports, per releaseId+target, everything this task cannot produce
//      without a real macOS/Windows build+sign+test+install run (a sealed
//      artifact for a not-yet-verified target, test receipts, ed25519 /
//      apple-notary / windows-authenticode signatures, MBR-806 installed
//      receipts, sealed-vs-legacy-unsealed event history) as an explicit
//      `gaps` entry -- never a fabricated placeholder -- and marks the
//      manifest `status: "incomplete"` until every one of those exists.
//
// assembleReleaseEvidenceManifest() is the pure composition step used once
// all of the above genuinely exists (at the Book 3 final phased gate, on
// the machines that produced it): it builds the orthic.membrane.release-evidence.v1
// object and the caller runs the pre-existing verifyReleaseEvidence() over
// it before trusting it -- this module does not re-implement that check.
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { DEFAULT_LOCKFILES, generateSbom } from "./sbom.mjs";
import { verifyReleaseEvidence } from "../verify-release-evidence.mjs";

const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };
const RELEASE_GENERATION_SCHEMA = "orthic.membrane.release-generation.v1";
export const RELEASE_EVIDENCE_SCHEMA = "orthic.membrane.release-evidence.v1";

function git(cwd, args) {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  if (result.status !== 0) fail(`git ${args.join(" ")} failed: ${(result.stderr ?? "").trim() || `exit ${result.status}`}`);
  return result.stdout.trim();
}

function writeEvidenceFile(dir, name, content) {
  mkdirSync(dir, { recursive: true });
  const path = resolve(dir, name);
  writeFileSync(path, content);
  return { path, sha256: createHash("sha256").update(content).digest("hex") };
}

/**
 * Reads MBR-903's immutable evidence/releases/<releaseId>/release-generation.json.
 * This is the only place release identity comes from in this module.
 */
export function readReleaseGeneration({ repoRoot, releaseId, evidenceRoot }) {
  const root = evidenceRoot ?? resolve(repoRoot, "evidence", "releases");
  const path = resolve(root, releaseId, "release-generation.json");
  if (!existsSync(path)) fail(`no release-generation.json for ${releaseId}; run scripts/release/pipeline/multi-platform-release.mjs --write first (MBR-903)`);
  const plan = JSON.parse(readFileSync(path, "utf8"));
  if (plan.schema !== RELEASE_GENERATION_SCHEMA) fail(`${path} is not a ${RELEASE_GENERATION_SCHEMA} record`);
  return plan;
}

/**
 * Read-only git source provenance for one commit: tree, parent(s),
 * author/committer identity+date, subject line. Every subcommand is
 * informational (`git show -s --format=...`), never a mutating git call.
 */
export function buildProvenance({ repoRoot, commit }) {
  const FIELD_SEP = "\x1f";
  const format = ["%H", "%T", "%P", "%an <%ae>", "%aI", "%cn <%ce>", "%cI", "%s"].join(FIELD_SEP);
  const raw = git(repoRoot, ["show", "-s", `--format=${format}`, commit]);
  const [hash, tree, parents, author, authorDate, committer, committerDate, subject] = raw.split(FIELD_SEP);
  if (hash !== commit) fail(`git show returned commit ${hash}, expected ${commit}`);
  return {
    schema: "orthic.membrane.provenance.v1",
    commit: hash,
    tree,
    parents: parents ? parents.split(" ").filter(Boolean) : [],
    author,
    authorDate,
    committer,
    committerDate,
    subject,
  };
}

/**
 * Declared toolchain constraints already committed to the repo -- package.json
 * `engines`/`packageManager` and the hub's Cargo edition. Never an invoked
 * compiler/runtime version: this task must not run cargo, pnpm, or Tauri.
 */
export function buildToolchain({ repoRoot }) {
  const rootPkg = JSON.parse(readFileSync(resolve(repoRoot, "package.json"), "utf8"));
  const hubPkgPath = resolve(repoRoot, "apps/membrane-hub/package.json");
  const hubPkg = existsSync(hubPkgPath) ? JSON.parse(readFileSync(hubPkgPath, "utf8")) : null;
  const hubCargoPath = resolve(repoRoot, "apps/membrane-hub/src-tauri/Cargo.toml");
  const hubCargoText = existsSync(hubCargoPath) ? readFileSync(hubCargoPath, "utf8") : null;
  const hubCargoEdition = hubCargoText ? hubCargoText.match(/\nedition = "([^"]+)"/)?.[1] ?? null : null;
  return {
    schema: "orthic.membrane.toolchain.v1",
    node: { engines: rootPkg.engines ?? null, packageManager: rootPkg.packageManager ?? null },
    hub: hubPkg ? { packageManager: hubPkg.packageManager ?? null, engines: hubPkg.engines ?? null } : null,
    rust: { edition: hubCargoEdition, rustToolchainToml: existsSync(resolve(repoRoot, "rust-toolchain.toml")) },
  };
}

/**
 * The real work this task can do without a build/sign/test/install run:
 * reads MBR-903's release_generation for releaseId+target, generates and
 * writes the SBOM/provenance/toolchain evidence for it, and reports every
 * remaining requirement of orthic.membrane.release-evidence.v1 as an
 * explicit gap. Never returns `status: "complete"` -- that requires the
 * caller to supply the real artifact/tests/signatures/install_receipts/
 * event_history this task cannot produce, then call
 * assembleReleaseEvidenceManifest + verifyReleaseEvidence.
 */
export function buildReleaseGenerationEvidence({ repoRoot, releaseId, target, sbomLockfiles = DEFAULT_LOCKFILES, evidenceRoot }) {
  if (!repoRoot) fail("repoRoot is required");
  if (!releaseId) fail("releaseId is required");
  if (!target) fail("target is required");
  const root = evidenceRoot ?? resolve(repoRoot, "evidence", "releases");
  const plan = readReleaseGeneration({ repoRoot, releaseId, evidenceRoot: root });
  const targetStage = plan.targets.find((entry) => entry.target === target);
  if (!targetStage) fail(`release-generation.json for ${releaseId} has no target ${target}`);

  const evidenceDir = resolve(root, releaseId);
  const gaps = [];

  const sbom = generateSbom({ repoRoot, lockfiles: sbomLockfiles });
  const sbomFile = writeEvidenceFile(evidenceDir, "sbom.json", `${JSON.stringify(sbom, null, 2)}\n`);
  if (sbom.gaps.length) for (const gap of sbom.gaps) gaps.push({ kind: "sbom-component", ...gap });

  const provenance = buildProvenance({ repoRoot, commit: plan.release.commit });
  const provenanceFile = writeEvidenceFile(evidenceDir, "provenance.json", `${JSON.stringify(provenance, null, 2)}\n`);

  const toolchain = buildToolchain({ repoRoot });
  const toolchainFile = writeEvidenceFile(evidenceDir, "toolchain.json", `${JSON.stringify(toolchain, null, 2)}\n`);

  if (targetStage.status !== "verified") {
    gaps.push({ kind: "artifact", target, status: targetStage.status, reason: targetStage.reason ?? `no sealed, verified artifact for ${target} yet (MBR-901/MBR-902 build/sign not run on this machine)` });
  } else if (!targetStage.artifact?.path) {
    gaps.push({ kind: "artifact-path", target, reason: "sealed target has no locally recorded artifact path (see scripts/release/pipeline/sealed-platforms.mjs)" });
  }
  gaps.push({ kind: "tests", reason: "release-evidence.v1 requires >=1 real test receipt; no full test run was executed by this task" });
  gaps.push({ kind: "signatures", reason: "release-evidence.v1 requires an ed25519 artifact signature receipt; no signing was performed by this task" });
  gaps.push({ kind: "platform_trust", reason: `release-evidence.v1 requires ${target.startsWith("mac-") ? "an apple-notary" : "a windows-authenticode"} trust receipt for ${target}; not produced by this task` });
  gaps.push({ kind: "install_receipts", reason: "release-evidence.v1 requires installed macOS and Windows vector-dispatch receipts (MBR-806); not produced by this task" });
  gaps.push({ kind: "event_history", reason: "explicit sealed-vs-legacy-unsealed event-history receipt not produced by this task" });

  return {
    releaseId,
    target,
    release: { tag: plan.release.tag, commit: plan.release.commit, tree: plan.release.tree, generation: plan.release.release_generation, vector_dispatch: plan.vector_dispatch },
    sbom: sbomFile,
    provenance: provenanceFile,
    toolchain: toolchainFile,
    artifact: targetStage.artifact ? { name: targetStage.artifact.name, sha256: targetStage.artifact.sha256, path: targetStage.artifact.path ?? null } : null,
    status: "incomplete",
    gaps,
  };
}

/**
 * Pure composition of the final orthic.membrane.release-evidence.v1 object
 * from already-real, already hash-bound pieces (this task's release/sbom/
 * provenance/toolchain/compatibility, plus the caller-supplied tests/
 * signatures/platform_trust/install_receipts/event_history that only exist
 * after a real build+sign+test+install run). This function does not
 * validate hash-binding itself -- call verifyReleaseEvidence(manifest, root)
 * (imported above, unmodified) on the result before trusting it.
 */
export function assembleReleaseEvidenceManifest({ release, artifact, sbom, provenance, toolchain, tests, signatures, platform_trust, compatibility, install_receipts, event_history }) {
  for (const [name, value] of Object.entries({ release, artifact, sbom, provenance, toolchain, tests, signatures, platform_trust, compatibility, install_receipts, event_history })) {
    if (value === undefined) fail(`assembleReleaseEvidenceManifest: missing ${name}`);
  }
  return { schema: RELEASE_EVIDENCE_SCHEMA, release, artifact, sbom, provenance, toolchain, tests, signatures, platform_trust, compatibility, install_receipts, event_history };
}

function parseArgs(argv) {
  const args = { repoRoot: process.cwd() };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--release-id") args.releaseId = argv[++i];
    else if (arg === "--target") args.target = argv[++i];
    else if (arg === "--repo-root") args.repoRoot = argv[++i];
    else if (arg === "--evidence-root") args.evidenceRoot = argv[++i];
    else if (arg === "-h" || arg === "--help") args.help = true;
    else fail(`unknown argument: ${arg}`);
  }
  return args;
}

function usage() {
  console.log(
    "usage: manifest.mjs --release-id ID --target TARGET [--repo-root PATH]\n\n" +
    "Reads evidence/releases/<releaseId>/release-generation.json (MBR-903's output),\n" +
    "generates and writes this task's real SBOM/provenance/toolchain evidence next to\n" +
    "it, and prints a report of what is real today versus what remains a gap (tests,\n" +
    "signatures, platform trust, installed receipts, event history, and any\n" +
    "not-yet-verified target artifact) until a real build/sign/test/install run\n" +
    "produces them. Never builds, signs, tests, or installs anything itself.",
  );
}

function cli() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help || !args.releaseId || !args.target) { usage(); process.exit(args.help ? 0 : 2); }
  try {
    const repoRoot = resolve(args.repoRoot);
    const evidenceRoot = args.evidenceRoot ? resolve(args.evidenceRoot) : undefined;
    const report = buildReleaseGenerationEvidence({ repoRoot, releaseId: args.releaseId, target: args.target, evidenceRoot });
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) cli();
