import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { computeReleaseGeneration, VECTOR_DISPATCH } from "../pipeline/identity.mjs";
import { verifyReleaseEvidence } from "../verify-release-evidence.mjs";
import {
  assembleReleaseEvidenceManifest,
  buildProvenance,
  buildReleaseGenerationEvidence,
  buildToolchain,
  readReleaseGeneration,
  RELEASE_EVIDENCE_SCHEMA,
} from "./manifest.mjs";

const REPO_ROOT = resolve(fileURLToPath(import.meta.url), "../../../..");
const sha = (value) => createHash("sha256").update(value).digest("hex");

// Read-only: `git rev-parse` never mutates. No test in this file runs `git
// init`/`add`/`commit`/any other mutating git command, in this repo or any
// other -- this task's hard rule against mutating git commands is absolute,
// not scoped to "the real repo only". Everything that needs a real git
// commit to exist reads it read-only from this actual checkout's current
// HEAD instead of fabricating a throwaway repo.
function git(args) {
  const result = spawnSync("git", args, { cwd: REPO_ROOT, encoding: "utf8" });
  if (result.status !== 0) throw new Error(`git ${args.join(" ")} failed: ${result.stderr}`);
  return result.stdout.trim();
}

test("readReleaseGeneration fails closed when MBR-903's file is missing, and rejects the wrong schema", () => {
  const dir = mkdtempSync(join(tmpdir(), "membrane-evidence-rg-"));
  try {
    assert.throws(() => readReleaseGeneration({ repoRoot: REPO_ROOT, releaseId: "membrane-hub-9.9.9-deadbeef", evidenceRoot: dir }), /no release-generation\.json/);
    mkdirSync(resolve(dir, "membrane-hub-9.9.9-deadbeef"), { recursive: true });
    writeFileSync(resolve(dir, "membrane-hub-9.9.9-deadbeef/release-generation.json"), JSON.stringify({ schema: "not-the-right-schema" }));
    assert.throws(() => readReleaseGeneration({ repoRoot: REPO_ROOT, releaseId: "membrane-hub-9.9.9-deadbeef", evidenceRoot: dir }), /is not a orthic\.membrane\.release-generation\.v1 record/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("buildProvenance reads this repository's real current HEAD commit read-only", () => {
  const commit = git(["rev-parse", "HEAD"]);
  const provenance = buildProvenance({ repoRoot: REPO_ROOT, commit });
  assert.equal(provenance.schema, "orthic.membrane.provenance.v1");
  assert.equal(provenance.commit, commit);
  assert.match(provenance.tree, /^[0-9a-f]{40}$/);
  assert.match(provenance.author, /<.+@.+>/);
  assert.match(provenance.authorDate, /^\d{4}-\d{2}-\d{2}T/);
  assert.match(provenance.subject, /\S/);
});

test("buildProvenance fails closed on a commit git cannot resolve", () => {
  assert.throws(() => buildProvenance({ repoRoot: REPO_ROOT, commit: "0".repeat(40) }), /git show .* failed/);
});

test("buildToolchain reads this repository's real, declared toolchain constraints without invoking a compiler", () => {
  const toolchain = buildToolchain({ repoRoot: REPO_ROOT });
  assert.equal(toolchain.schema, "orthic.membrane.toolchain.v1");
  assert.equal(toolchain.node.packageManager, "pnpm@11.18.0");
  assert.equal(toolchain.hub.packageManager, "pnpm@11.18.0");
  assert.equal(toolchain.rust.edition, "2021");
});

test("buildReleaseGenerationEvidence binds to MBR-903's real release_generation, writes real SBOM/provenance/toolchain evidence derived from this repo's real lockfiles, and reports every remaining requirement as a gap instead of fabricating it", () => {
  const evidenceRoot = mkdtempSync(join(tmpdir(), "membrane-evidence-releases-"));
  try {
    const commit = git(["rev-parse", "HEAD"]);
    const tree = git(["rev-parse", "HEAD^{tree}"]);
    const targets = ["mac-arm64"];
    // The real MBR-903 digest algorithm over this repo's real, current
    // source identity -- not a hand-typed placeholder.
    const releaseGeneration = computeReleaseGeneration({ product: "Membrane", vectorDispatch: VECTOR_DISPATCH, commit, tree, version: "0.9.0", targets });
    const releaseId = `membrane-hub-0.9.0-${commit.slice(0, 8)}`;
    const plan = {
      schema: "orthic.membrane.release-generation.v1",
      product: "Membrane",
      vector_dispatch: VECTOR_DISPATCH,
      publish: false,
      release: { app: "membrane-hub", version: "0.9.0", tag: "v0.9.0", commit, tree, release_generation: releaseGeneration, releaseId },
      targets: [{ target: "mac-arm64", status: "pending" }],
    };
    mkdirSync(resolve(evidenceRoot, releaseId), { recursive: true });
    writeFileSync(resolve(evidenceRoot, releaseId, "release-generation.json"), JSON.stringify(plan, null, 2));

    const report = buildReleaseGenerationEvidence({ repoRoot: REPO_ROOT, releaseId, target: "mac-arm64", evidenceRoot });

    // The core binding property: this task's manifest binds the SAME
    // release_generation MBR-903 computed -- it never recomputes a second one.
    assert.equal(report.release.generation, releaseGeneration);
    assert.equal(report.release.commit, commit);
    assert.equal(report.release.tree, tree);
    assert.equal(report.release.tag, "v0.9.0");
    assert.equal(report.status, "incomplete");

    // Real files were actually written under the (isolated) evidence root,
    // hash-bound to their own real bytes -- never this repo's tracked
    // evidence/releases/ directory, which is outside this task's allowed paths.
    for (const file of [report.sbom, report.provenance, report.toolchain]) {
      assert.ok(file.path.startsWith(evidenceRoot), `${file.path} must be written under the isolated evidence root, not the real repo`);
      assert.ok(existsSync(file.path), `${file.path} must exist`);
      assert.equal(sha(readFileSync(file.path)), file.sha256);
    }
    const sbomOnDisk = JSON.parse(readFileSync(report.sbom.path, "utf8"));
    assert.ok(sbomOnDisk.componentCount > 400, "SBOM must be the real component set derived from this repo's real membrane-hub lockfiles");
    assert.deepEqual(sbomOnDisk.gaps, []);

    // Nothing this task cannot produce is fabricated -- it is named as a gap.
    const gapKinds = report.gaps.map((gap) => gap.kind);
    assert.ok(gapKinds.includes("artifact"), "an unverified target's missing artifact must be a named gap");
    for (const required of ["tests", "signatures", "platform_trust", "install_receipts", "event_history"]) {
      assert.ok(gapKinds.includes(required), `${required} must be a named gap, not silently absent`);
    }
    assert.equal(report.artifact, null);
  } finally {
    rmSync(evidenceRoot, { recursive: true, force: true });
  }
});

test("assembleReleaseEvidenceManifest fails closed on a missing required field", () => {
  assert.throws(() => assembleReleaseEvidenceManifest({ release: {}, artifact: {} }), /missing sbom/);
});

test("a manifest assembled with a real MBR-903-style release_generation passes the pre-existing verifier unmodified", () => {
  const dir = mkdtempSync(join(tmpdir(), "membrane-evidence-assemble-"));
  try {
    const file = (name, content = name) => { writeFileSync(join(dir, name), content); return { path: name, sha256: sha(content) }; };
    const commit = "b".repeat(40);
    const tree = "c".repeat(40);
    const targets = ["mac-arm64"];
    // The real MBR-903 digest algorithm -- computeReleaseGeneration -- not a
    // hand-typed placeholder hash, proving this manifest binds to that
    // identity rather than inventing a parallel one.
    const generation = computeReleaseGeneration({ product: "Membrane", vectorDispatch: VECTOR_DISPATCH, commit, tree, version: "1.2.3", targets });
    const artifact = file("artifact");
    const trust = (kind) => ({ kind, identity: `${kind}-identity`, subject_sha256: artifact.sha256, receipt: file(`${kind}.receipt`) });
    const platformAcceptance = (os) => {
      const base = { schema: "orthic.membrane.platform-acceptance.v1", receiptId: `${os}-1`, mode: "clean-vm", commit, releaseGeneration: generation, version: "1.2.3", platform: os, artifact: { name: `${os}.artifact`, sha256: "d".repeat(64) }, trust: os === "macos" ? { codesign: "pass", notarization: "pass", staple: "pass", gatekeeper: "pass" } : { authenticode: "pass", publicTrust: "pass", rfc3161: "pass" }, lifecycle: { install: "pass", startup: "pass", update: "pass", uninstall: "pass" }, environment: { clean: true, machineDigest: "e".repeat(64), bypassWarnings: false } };
      const contract = file(`${os}.contract`, JSON.stringify({ schema: base.schema, commit: base.commit, releaseGeneration: base.releaseGeneration, version: base.version, platform: base.platform, artifact: base.artifact }));
      return { os, vector_dispatch: VECTOR_DISPATCH, contract, receipt: file(`${os}.receipt`, JSON.stringify(base)) };
    };
    const manifest = assembleReleaseEvidenceManifest({
      release: { tag: "v1.2.3", commit, tree, generation, target: "mac-arm64", artifact_sha256: artifact.sha256, vector_dispatch: VECTOR_DISPATCH },
      artifact,
      sbom: file("sbom"),
      provenance: file("provenance"),
      toolchain: file("toolchain"),
      tests: [file("test")],
      signatures: [trust("ed25519")],
      platform_trust: trust("apple-notary"),
      compatibility: file("compatibility"),
      install_receipts: [platformAcceptance("macos"), platformAcceptance("windows")],
      event_history: { status: "sealed", receipt: file("history") },
    });
    assert.equal(manifest.schema, RELEASE_EVIDENCE_SCHEMA);
    const result = verifyReleaseEvidence(manifest, dir);
    assert.equal(result.verified, true);
    assert.equal(result.release.generation, generation);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
