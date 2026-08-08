// MBR-903: deterministic tests for the multi-platform release pipeline.
//
// These tests never build, sign, notarize, or publish anything, and never
// run a mutating git command. They exercise pure functions plus read-only
// filesystem fixtures written under a throwaway temp directory (never the
// real repository's .right-release/ or evidence/releases/).
//
// What they prove, concretely:
//  - computeReleaseGeneration is deterministic: identical inputs -> the
//    identical digest, every time (reproducibility, not asserted).
//  - the digest changes when the source tree drifts, and readSealedPlatformStage
//    fails closed (ReleaseDriftError) when a sealed platform artifact names
//    a different commit than the release expects (drift is caught).
//  - buildMultiPlatformPlan never fabricates a "verified" target: with no
//    sealed output, buildable targets are "pending" and the two targets
//    RightKit's current apps/membrane-hub config cannot produce are
//    honestly "not-buildable", never silently promoted.
//  - writeImmutableReleaseGeneration is idempotent for the same plan and
//    refuses to overwrite a different plan under the same releaseId.
import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { VECTOR_DISPATCH, computeReleaseGeneration } from "./identity.mjs";
import { targetsInOrder, describeTarget } from "./platform-targets.mjs";
import { readSealedPlatformStage, ReleaseDriftError } from "./sealed-platforms.mjs";
import {
  releaseId, buildMultiPlatformPlan, writeImmutableReleaseGeneration, ImmutableReleaseConflictError,
} from "./multi-platform-release.mjs";

const PRODUCT = "Membrane";
const COMMIT_A = "a".repeat(40);
const COMMIT_B = "b".repeat(40);
const TREE_A = "c".repeat(40);
const TREE_B = "d".repeat(40);
const VERSION = "0.1.1";
const TAG = "v0.1.1";
const APP = "membrane-hub";

function tempRepoRoot() {
  return mkdtempSync(join(tmpdir(), "mbr-903-pipeline-"));
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// -- identity.mjs -------------------------------------------------------

test("computeReleaseGeneration is deterministic for identical inputs", () => {
  const input = { product: PRODUCT, vectorDispatch: VECTOR_DISPATCH, commit: COMMIT_A, tree: TREE_A, version: VERSION, targets: targetsInOrder() };
  const first = computeReleaseGeneration({ ...input });
  const second = computeReleaseGeneration({ ...input });
  assert.equal(first, second);
  assert.match(first, /^[a-f0-9]{64}$/);
});

test("computeReleaseGeneration changes when the source tree drifts", () => {
  const base = { product: PRODUCT, vectorDispatch: VECTOR_DISPATCH, commit: COMMIT_A, version: VERSION, targets: targetsInOrder() };
  const withTreeA = computeReleaseGeneration({ ...base, tree: TREE_A });
  const withTreeB = computeReleaseGeneration({ ...base, tree: TREE_B });
  assert.notEqual(withTreeA, withTreeB);
});

test("computeReleaseGeneration is order-sensitive on targets but insensitive to object key order", () => {
  const a = computeReleaseGeneration({ product: PRODUCT, vectorDispatch: VECTOR_DISPATCH, commit: COMMIT_A, tree: TREE_A, version: VERSION, targets: ["mac-arm64", "windows-x64"] });
  const b = computeReleaseGeneration({ vectorDispatch: VECTOR_DISPATCH, version: VERSION, commit: COMMIT_A, targets: ["mac-arm64", "windows-x64"], tree: TREE_A, product: PRODUCT });
  const reordered = computeReleaseGeneration({ product: PRODUCT, vectorDispatch: VECTOR_DISPATCH, commit: COMMIT_A, tree: TREE_A, version: VERSION, targets: ["windows-x64", "mac-arm64"] });
  assert.equal(a, b);
  assert.notEqual(a, reordered);
});

test("computeReleaseGeneration fails closed on invalid identity fields", () => {
  const valid = { product: PRODUCT, vectorDispatch: VECTOR_DISPATCH, commit: COMMIT_A, tree: TREE_A, version: VERSION, targets: ["mac-arm64"] };
  assert.throws(() => computeReleaseGeneration({ ...valid, vectorDispatch: "old" }), /vectorDispatch/);
  assert.throws(() => computeReleaseGeneration({ ...valid, commit: "not-a-sha" }), /commit must be/);
  assert.throws(() => computeReleaseGeneration({ ...valid, tree: "short" }), /tree must be/);
  assert.throws(() => computeReleaseGeneration({ ...valid, version: "main" }), /version must be semver/);
  assert.throws(() => computeReleaseGeneration({ ...valid, targets: [] }), /targets must be a non-empty array/);
});

// -- platform-targets.mjs ------------------------------------------------

test("platform-targets names all four release/contracts/platforms.v1.json targets in order", () => {
  assert.deepEqual(targetsInOrder(), ["mac-arm64", "mac-x64", "windows-x64", "windows-arm64"]);
});

test("platform-targets is honest about the two targets RightKit cannot build today", () => {
  assert.equal(describeTarget("mac-arm64").buildable, true);
  assert.equal(describeTarget("windows-x64").buildable, true);
  assert.equal(describeTarget("mac-x64").buildable, false);
  assert.equal(describeTarget("windows-arm64").buildable, false);
  assert.throws(() => describeTarget("linux-x64"), /unknown release target/);
});

// -- sealed-platforms.mjs -------------------------------------------------

test("readSealedPlatformStage reports not-buildable targets without touching disk", () => {
  const stage = readSealedPlatformStage({ repoRoot: "/does/not/exist", app: APP, version: VERSION, commit: COMMIT_A }, "mac-x64");
  assert.deepEqual(stage, { target: "mac-x64", status: "not-buildable", reason: describeTarget("mac-x64").reason });
});

test("readSealedPlatformStage reports pending when nothing is sealed yet", () => {
  const repoRoot = tempRepoRoot();
  try {
    assert.equal(readSealedPlatformStage({ repoRoot, app: APP, version: VERSION, commit: COMMIT_A }, "mac-arm64").status, "pending");
    assert.equal(readSealedPlatformStage({ repoRoot, app: APP, version: VERSION, commit: COMMIT_A }, "windows-x64").status, "pending");
  } finally {
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

function writeMacSealedFixture({ repoRoot, app, version, commit, dmgBytes = Buffer.from("fake dmg bytes") }) {
  const releaseIdValue = `${app}-${version}-${commit.slice(0, 8)}`;
  const sealedDir = resolve(repoRoot, ".right-release", "sealed", releaseIdValue, "mac");
  mkdirSync(sealedDir, { recursive: true });
  const dmgName = `Membrane Hub_${version}_aarch64.dmg`;
  writeFileSync(resolve(sealedDir, dmgName), dmgBytes);
  const manifest = {
    schema: 1, releaseId: releaseIdValue, app, version, commit, platform: "mac",
    files: [{ role: "installer", name: dmgName, sha256: sha256(dmgBytes), sizeBytes: dmgBytes.length }],
    checkpoints: ["preflight_complete", "build_complete", "signed", "hardened", "sealed"],
  };
  writeFileSync(resolve(sealedDir, "release-manifest.json"), JSON.stringify(manifest));
  return { sealedDir, dmgName };
}

function writeWindowsSealedFixture({ repoRoot, app, version, commit, exeBytes = Buffer.from("fake installer bytes") }) {
  const releaseIdValue = `${app}-${version}-${commit.slice(0, 8)}`;
  const sealedDir = resolve(repoRoot, ".right-release", "sealed", releaseIdValue, "windows");
  mkdirSync(sealedDir, { recursive: true });
  const exeName = `Membrane_${version}_x64-setup.exe`;
  const manifest = {
    schema: 1, releaseId: releaseIdValue, app, version, commit, platform: "win",
    files: [{ role: "installer", name: exeName, sha256: sha256(exeBytes), sizeBytes: exeBytes.length }],
    checkpoints: ["preflight_complete", "build_complete", "signed", "hardened", "sealed"],
  };
  writeFileSync(resolve(sealedDir, "release-manifest.json"), JSON.stringify(manifest));
  return { sealedDir, exeName };
}

test("readSealedPlatformStage verifies a matching sealed mac artifact against its recomputed hash", () => {
  const repoRoot = tempRepoRoot();
  try {
    const { dmgName } = writeMacSealedFixture({ repoRoot, app: APP, version: VERSION, commit: COMMIT_A });
    const stage = readSealedPlatformStage({ repoRoot, app: APP, version: VERSION, commit: COMMIT_A }, "mac-arm64");
    assert.equal(stage.status, "verified");
    assert.equal(stage.artifact.name, dmgName);
    assert.match(stage.artifact.sha256, /^[a-f0-9]{64}$/);
  } finally {
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

test("readSealedPlatformStage verifies a matching sealed windows artifact against RightKit's real manifest shape", () => {
  const repoRoot = tempRepoRoot();
  try {
    const { exeName } = writeWindowsSealedFixture({ repoRoot, app: APP, version: VERSION, commit: COMMIT_A });
    const stage = readSealedPlatformStage({ repoRoot, app: APP, version: VERSION, commit: COMMIT_A }, "windows-x64");
    assert.equal(stage.status, "verified");
    assert.equal(stage.artifact.name, exeName);
  } finally {
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

test("readSealedPlatformStage fails closed with ReleaseDriftError when a sealed artifact names a different commit (mac)", () => {
  const repoRoot = tempRepoRoot();
  try {
    // A sealed manifest present at COMMIT_A's releaseId path but whose recorded
    // commit is actually COMMIT_B -- the exact shape of "Windows built before
    // pulling the Mac's commit," or a stale sealed dir left from another branch.
    const releaseIdA = `${APP}-${VERSION}-${COMMIT_A.slice(0, 8)}`;
    const sealedDirA = resolve(repoRoot, ".right-release", "sealed", releaseIdA, "mac");
    mkdirSync(sealedDirA, { recursive: true });
    const dmgBytes = Buffer.from("drifted dmg");
    const dmgName = `Membrane Hub_${VERSION}_aarch64.dmg`;
    writeFileSync(resolve(sealedDirA, dmgName), dmgBytes);
    writeFileSync(resolve(sealedDirA, "release-manifest.json"), JSON.stringify({
      schema: 1, releaseId: releaseIdA, app: APP, version: VERSION, commit: COMMIT_B, platform: "mac",
      files: [{ role: "installer", name: dmgName, sha256: sha256(dmgBytes), sizeBytes: dmgBytes.length }],
      checkpoints: ["preflight_complete", "build_complete", "signed", "hardened", "sealed"],
    }));
    assert.throws(
      () => readSealedPlatformStage({ repoRoot, app: APP, version: VERSION, commit: COMMIT_A }, "mac-arm64"),
      (error) => error instanceof ReleaseDriftError && error.detail.expectedCommit === COMMIT_A && error.detail.sealedCommit === COMMIT_B,
    );
  } finally {
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

// -- multi-platform-release.mjs -------------------------------------------

test("releaseId matches RightKit's own convention: app-version-shortcommit", () => {
  assert.equal(releaseId({ app: APP, version: VERSION, commit: COMMIT_A }), `membrane-hub-0.1.1-${COMMIT_A.slice(0, 8)}`);
  assert.throws(() => releaseId({ app: APP, version: "not-semver", commit: COMMIT_A }), /version must be semver/);
});

test("buildMultiPlatformPlan never fabricates a verified target with nothing sealed", () => {
  const repoRoot = tempRepoRoot();
  try {
    const plan = buildMultiPlatformPlan({ repoRoot, app: APP, version: VERSION, tag: TAG, commit: COMMIT_A, tree: TREE_A });
    assert.equal(plan.publish, false);
    assert.equal(plan.vector_dispatch, VECTOR_DISPATCH);
    const byTarget = Object.fromEntries(plan.targets.map((t) => [t.target, t.status]));
    assert.deepEqual(byTarget, { "mac-arm64": "pending", "mac-x64": "not-buildable", "windows-x64": "pending", "windows-arm64": "not-buildable" });
    // The plan's release_generation must equal independently recomputing the
    // same digest from identity.mjs directly -- proves the orchestrator reuses
    // computeReleaseGeneration rather than deriving its own separate hash.
    const expected = computeReleaseGeneration({ product: "Membrane", vectorDispatch: VECTOR_DISPATCH, commit: COMMIT_A, tree: TREE_A, version: VERSION, targets: targetsInOrder() });
    assert.equal(plan.release.release_generation, expected);
  } finally {
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

test("buildMultiPlatformPlan is reproducible: identical inputs on two separate calls produce byte-identical plans", () => {
  const repoRoot = tempRepoRoot();
  try {
    writeMacSealedFixture({ repoRoot, app: APP, version: VERSION, commit: COMMIT_A });
    const first = buildMultiPlatformPlan({ repoRoot, app: APP, version: VERSION, tag: TAG, commit: COMMIT_A, tree: TREE_A });
    const second = buildMultiPlatformPlan({ repoRoot, app: APP, version: VERSION, tag: TAG, commit: COMMIT_A, tree: TREE_A });
    assert.deepEqual(first, second);
  } finally {
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

test("buildMultiPlatformPlan surfaces cross-platform drift instead of silently combining mismatched commits", () => {
  const repoRoot = tempRepoRoot();
  try {
    writeMacSealedFixture({ repoRoot, app: APP, version: VERSION, commit: COMMIT_A });
    // Windows sealed manifest lives at COMMIT_A's releaseId path (i.e. it looks,
    // from the directory name alone, like it belongs to this release) but its
    // own recorded `commit` field is COMMIT_B -- exactly what a Windows machine
    // building from a stale pull under an otherwise-matching path would produce.
    const releaseIdA = `${APP}-${VERSION}-${COMMIT_A.slice(0, 8)}`;
    const sealedDirA = resolve(repoRoot, ".right-release", "sealed", releaseIdA, "windows");
    mkdirSync(sealedDirA, { recursive: true });
    const exeBytes = Buffer.from("stale pull installer");
    const exeName = `Membrane_${VERSION}_x64-setup.exe`;
    writeFileSync(resolve(sealedDirA, "release-manifest.json"), JSON.stringify({
      schema: 1, releaseId: releaseIdA, app: APP, version: VERSION, commit: COMMIT_B, platform: "win",
      files: [{ role: "installer", name: exeName, sha256: sha256(exeBytes), sizeBytes: exeBytes.length }],
      checkpoints: ["preflight_complete", "build_complete", "signed", "hardened", "sealed"],
    }));
    // Ask for the release at COMMIT_A: the plan must fail closed, not silently
    // merge the mac (COMMIT_A) and windows (COMMIT_B) stages into one release.
    assert.throws(
      () => buildMultiPlatformPlan({ repoRoot, app: APP, version: VERSION, tag: TAG, commit: COMMIT_A, tree: TREE_A }),
      (error) => error instanceof ReleaseDriftError && error.detail.expectedCommit === COMMIT_A && error.detail.sealedCommit === COMMIT_B,
    );
  } finally {
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

test("buildMultiPlatformPlan fails closed on malformed identity", () => {
  const repoRoot = tempRepoRoot();
  try {
    const valid = { repoRoot, app: APP, version: VERSION, tag: TAG, commit: COMMIT_A, tree: TREE_A };
    assert.throws(() => buildMultiPlatformPlan({ ...valid, tag: "0.1.1" }), /tag must be immutable semver/);
    assert.throws(() => buildMultiPlatformPlan({ ...valid, commit: "short" }), /commit must be/);
    assert.throws(() => buildMultiPlatformPlan({ ...valid, targets: [] }), /targets must be a non-empty array/);
  } finally {
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

test("writeImmutableReleaseGeneration is idempotent for an unchanged plan and rejects drift under the same releaseId", () => {
  const repoRoot = tempRepoRoot();
  try {
    const evidenceRoot = resolve(repoRoot, "evidence-fixture");
    const planA = buildMultiPlatformPlan({ repoRoot, app: APP, version: VERSION, tag: TAG, commit: COMMIT_A, tree: TREE_A });

    const firstWrite = writeImmutableReleaseGeneration({ repoRoot, plan: planA, evidenceRoot });
    assert.equal(firstWrite.written, true);

    // Re-running the exact same release (this is the "run once per machine,
    // Mac then Windows" workflow) must succeed without rewriting or erroring.
    const secondWrite = writeImmutableReleaseGeneration({ repoRoot, plan: planA, evidenceRoot });
    assert.equal(secondWrite.written, false);
    assert.equal(secondWrite.path, firstWrite.path);

    // A DIFFERENT plan (different tree => different release_generation) for the
    // SAME releaseId (same app/version/commit) must never silently overwrite --
    // this is the immutability half of "reproducible": once recorded, a release
    // generation cannot quietly become a different release generation.
    const planDrifted = buildMultiPlatformPlan({ repoRoot, app: APP, version: VERSION, tag: TAG, commit: COMMIT_A, tree: TREE_B });
    assert.equal(planDrifted.release.releaseId, planA.release.releaseId);
    assert.notEqual(planDrifted.release.release_generation, planA.release.release_generation);
    assert.throws(
      () => writeImmutableReleaseGeneration({ repoRoot, plan: planDrifted, evidenceRoot }),
      (error) => error instanceof ImmutableReleaseConflictError,
    );
  } finally {
    rmSync(repoRoot, { recursive: true, force: true });
  }
});
