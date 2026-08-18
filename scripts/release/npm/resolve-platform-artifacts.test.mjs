// MBR-906: deterministic tests for the npm-platform artifact resolver.
//
// These tests never build, sign, publish, or run a mutating git command.
// They exercise pure functions against a throwaway temp directory fixture
// shaped exactly like dist/release/contracts/release-generation.v1.schema.json,
// plus one direct assertion against the real repository root proving this
// resolver invents nothing when no release has actually been recorded.
import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  NPM_PLATFORM_TO_PIPELINE_TARGET,
  releaseGenerationPath,
  readReleaseGeneration,
  resolveNpmPlatformArtifact,
  resolveAllNpmPlatformArtifacts,
  NoPipelineTargetError,
  ArtifactNotVerifiedError,
} from "./resolve-platform-artifacts.mjs";
import { releaseId as computeReleaseId } from "../identity.mjs";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const REAL_REPO_ROOT = resolve(HERE, "../../..");

const APP = "membrane-hub";
const VERSION = "0.1.1";
const COMMIT = "a".repeat(40);
const RELEASE_ID = computeReleaseId({ app: APP, version: VERSION, commit: COMMIT });

function tempRepoRoot() {
  return mkdtempSync(join(tmpdir(), "mbr-906-npm-resolver-"));
}

function writeFixtureGeneration(repoRoot, targets) {
  const file = releaseGenerationPath({ repoRoot, releaseId: RELEASE_ID });
  mkdirSync(resolve(file, ".."), { recursive: true });
  const plan = {
    schema: "orthic.membrane.release-generation.v1",
    product: "Membrane",
    vector_dispatch: "CRYPT_VECTOR_DISPATCH_V2",
    publish: false,
    release: {
      app: APP, version: VERSION, tag: `v${VERSION}`, commit: COMMIT, tree: "b".repeat(40),
      release_generation: "c".repeat(64), releaseId: RELEASE_ID,
    },
    targets,
  };
  writeFileSync(file, JSON.stringify(plan, null, 2));
  return file;
}

test("npm platform keys with no Membrane release-pipeline target are declared, permanent gaps", () => {
  assert.equal(NPM_PLATFORM_TO_PIPELINE_TARGET["linux-x64"], null);
  assert.equal(NPM_PLATFORM_TO_PIPELINE_TARGET["linux-arm64"], null);
  assert.throws(
    () => resolveNpmPlatformArtifact({ repoRoot: tempRepoRoot(), releaseId: RELEASE_ID, npmPlatformKey: "linux-x64" }),
    NoPipelineTargetError,
  );
});

test("resolveNpmPlatformArtifact rejects an unknown npm platform key", () => {
  assert.throws(
    () => resolveNpmPlatformArtifact({ repoRoot: tempRepoRoot(), releaseId: RELEASE_ID, npmPlatformKey: "freebsd-x64" }),
    /unknown npm platform key/,
  );
});

test("no release-generation recorded yet is a declared gap, never a fabricated artifact", () => {
  const repoRoot = tempRepoRoot();
  assert.equal(readReleaseGeneration({ repoRoot, releaseId: RELEASE_ID }), null);
  assert.throws(
    () => resolveNpmPlatformArtifact({ repoRoot, releaseId: RELEASE_ID, npmPlatformKey: "darwin-arm64" }),
    ArtifactNotVerifiedError,
  );
});

test("a verified pipeline target resolves to its real, recorded artifact", () => {
  const repoRoot = tempRepoRoot();
  writeFixtureGeneration(repoRoot, [
    { target: "mac-arm64", status: "verified", artifact: { name: "Membrane Hub_0.1.1_aarch64.dmg", sha256: "d".repeat(64) } },
    { target: "mac-x64", status: "not-buildable", reason: "no distinct mac-x64 RightKit build exists" },
    { target: "windows-x64", status: "pending" },
    { target: "windows-arm64", status: "not-buildable", reason: "conformance-only" },
  ]);

  const resolved = resolveNpmPlatformArtifact({ repoRoot, releaseId: RELEASE_ID, npmPlatformKey: "darwin-arm64" });
  assert.deepEqual(resolved, {
    npmPlatformKey: "darwin-arm64",
    pipelineTarget: "mac-arm64",
    version: VERSION,
    commit: COMMIT,
    releaseGeneration: "c".repeat(64),
    artifactName: "Membrane Hub_0.1.1_aarch64.dmg",
    artifactSha256: "d".repeat(64),
  });
});

test("a pending or not-buildable pipeline target never fabricates a verified artifact", () => {
  const repoRoot = tempRepoRoot();
  writeFixtureGeneration(repoRoot, [
    { target: "mac-arm64", status: "verified", artifact: { name: "x.dmg", sha256: "e".repeat(64) } },
    { target: "mac-x64", status: "not-buildable", reason: "no distinct mac-x64 RightKit build exists" },
    { target: "windows-x64", status: "pending" },
    { target: "windows-arm64", status: "not-buildable", reason: "conformance-only" },
  ]);

  assert.throws(
    () => resolveNpmPlatformArtifact({ repoRoot, releaseId: RELEASE_ID, npmPlatformKey: "win32-x64" }),
    (error) => error instanceof ArtifactNotVerifiedError && /"pending"/.test(error.message),
  );
  assert.throws(
    () => resolveNpmPlatformArtifact({ repoRoot, releaseId: RELEASE_ID, npmPlatformKey: "win32-arm64" }),
    (error) => error instanceof ArtifactNotVerifiedError && /"not-buildable": conformance-only/.test(error.message),
  );
  assert.throws(
    () => resolveNpmPlatformArtifact({ repoRoot, releaseId: RELEASE_ID, npmPlatformKey: "darwin-x64" }),
    (error) => error instanceof ArtifactNotVerifiedError && /"not-buildable"/.test(error.message),
  );
});

test("resolveAllNpmPlatformArtifacts aggregates every platform's real state without throwing", () => {
  const repoRoot = tempRepoRoot();
  writeFixtureGeneration(repoRoot, [
    { target: "mac-arm64", status: "verified", artifact: { name: "x.dmg", sha256: "f".repeat(64) } },
    { target: "mac-x64", status: "not-buildable", reason: "no distinct mac-x64 RightKit build exists" },
    { target: "windows-x64", status: "verified", artifact: { name: "Membrane_0.1.1_x64-setup.exe", sha256: "0".repeat(64) } },
    { target: "windows-arm64", status: "not-buildable", reason: "conformance-only" },
  ]);

  const { releaseId, results } = resolveAllNpmPlatformArtifacts({ repoRoot, app: APP, version: VERSION, commit: COMMIT });
  assert.equal(releaseId, RELEASE_ID);
  assert.equal(results["darwin-arm64"].ok, true);
  assert.equal(results["darwin-arm64"].artifactSha256, "f".repeat(64));
  assert.equal(results["win32-x64"].ok, true);
  assert.equal(results["win32-x64"].artifactName, "Membrane_0.1.1_x64-setup.exe");
  assert.equal(results["darwin-x64"].ok, false);
  assert.equal(results["darwin-x64"].errorType, "ArtifactNotVerifiedError");
  assert.equal(results["win32-arm64"].ok, false);
  assert.equal(results["linux-x64"].ok, false);
  assert.equal(results["linux-x64"].errorType, "NoPipelineTargetError");
  assert.equal(results["linux-arm64"].errorType, "NoPipelineTargetError");
});

test("today's real Membrane checkout has no release-generation recorded for a plausible release: nothing is invented", () => {
  // Proves this resolver does not fabricate an artifact against the actual
  // repository: no docs/evidence/releases/<releaseId>/release-generation.json
  // exists for this releaseId in the real checkout (only mac releases are
  // sealed under .right-release/sealed/ today; MBR-903's write-once
  // evidence layer has not been run with --write on this checkout).
  assert.equal(readReleaseGeneration({ repoRoot: REAL_REPO_ROOT, releaseId: RELEASE_ID }), null);
  assert.throws(
    () => resolveNpmPlatformArtifact({ repoRoot: REAL_REPO_ROOT, releaseId: RELEASE_ID, npmPlatformKey: "darwin-arm64" }),
    ArtifactNotVerifiedError,
  );
});
