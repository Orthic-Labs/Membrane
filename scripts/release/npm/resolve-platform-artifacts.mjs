// MBR-906: resolves, per npm platform key, the real artifact MBR-903's
// multi-platform release pipeline has already sealed and recorded, by
// reading its immutable docs/evidence/releases/<releaseId>/release-generation.json
// (dist/release/contracts/release-generation.v1.schema.json), read-only.
//
// This module never builds, signs, notarizes, uploads, or publishes
// anything, and it never invents a version, commit, digest, or URL: every
// value it returns is copied verbatim from a release-generation.json MBR-903
// already wrote for a target it independently verified. A platform whose
// pipeline target is not "verified" -- or that has no Membrane
// release-pipeline target at all (linux-*: no RightKit Linux build exists,
// dist/release/contracts/platforms.v1.json has no linux entry) -- is a declared,
// named gap, never a fabricated artifact. It is the resolution layer the
// npm bootstrapper (dist/npm/bin/membrane.mjs, dist/npm/index.mjs) and the per-platform
// package scaffolds under dist/npm/platforms/** use it without re-deriving a
// release identity or mutating release state.
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { releaseId as computeReleaseId } from "../identity.mjs";

// npm platform key (dist/npm/index.mjs's PLATFORM_PACKAGES) -> the pipeline
// target enum dist/release/contracts/platforms.v1.json and
// docs/evidence/releases/<releaseId>/release-generation.json actually use. Only
// entries with a real pipeline target are mapped; the rest are `null`,
// documented, permanent gaps -- never guessed at.
export const NPM_PLATFORM_TO_PIPELINE_TARGET = Object.freeze({
  "darwin-arm64": "mac-arm64",
  "darwin-x64": "mac-x64",
  "linux-arm64": null,
  "linux-x64": null,
  "win32-arm64": "windows-arm64",
  "win32-x64": "windows-x64",
});

export class NoPipelineTargetError extends Error {}
export class ArtifactNotVerifiedError extends Error {}

export function releaseGenerationPath({ repoRoot, releaseId }) {
  return resolve(repoRoot, "docs", "evidence", "releases", releaseId, "release-generation.json");
}

/** Reads MBR-903's immutable release-generation record, or null if none has been written yet (the true state of an unreleased checkout, not an error). */
export function readReleaseGeneration({ repoRoot, releaseId }) {
  const file = releaseGenerationPath({ repoRoot, releaseId });
  if (!existsSync(file)) return null;
  return JSON.parse(readFileSync(file, "utf8"));
}

/**
 * Resolves the real, verified artifact (name + sha256) MBR-903 recorded for
 * one npm platform key, from an already-written release-generation.json.
 * Throws, naming the exact reason, instead of ever returning a fabricated
 * value:
 *  - NoPipelineTargetError: this npm platform key has no Membrane
 *    release-pipeline target today (linux-*).
 *  - ArtifactNotVerifiedError: no release-generation has been recorded yet
 *    for this releaseId, or the target's own pipeline status is not
 *    "verified" (e.g. "pending" or "not-buildable") -- the real status and
 *    reason (if any) are included verbatim.
 */
export function resolveNpmPlatformArtifact({ repoRoot, releaseId, npmPlatformKey }) {
  if (!(npmPlatformKey in NPM_PLATFORM_TO_PIPELINE_TARGET)) {
    throw new Error(`FAIL CLOSED: unknown npm platform key: ${npmPlatformKey}`);
  }
  const pipelineTarget = NPM_PLATFORM_TO_PIPELINE_TARGET[npmPlatformKey];
  if (pipelineTarget === null) {
    throw new NoPipelineTargetError(
      `declared gap: ${npmPlatformKey} has no Membrane release-pipeline target; no RightKit build produces this platform today (dist/release/contracts/platforms.v1.json)`,
    );
  }

  const generation = readReleaseGeneration({ repoRoot, releaseId });
  if (!generation) {
    throw new ArtifactNotVerifiedError(
      `declared gap: no sealed release-generation record for ${releaseId}; nothing to resolve`,
    );
  }

  const entry = generation.targets.find((candidate) => candidate.target === pipelineTarget);
  if (!entry) {
    throw new Error(`FAIL CLOSED: release-generation ${releaseId} has no entry for pipeline target ${pipelineTarget}`);
  }
  if (entry.status !== "verified" || !entry.artifact) {
    const reason = entry.reason ? `: ${entry.reason}` : "";
    throw new ArtifactNotVerifiedError(`declared gap: ${npmPlatformKey} (${pipelineTarget}) is "${entry.status}"${reason}; no verified artifact to publish yet`);
  }

  return {
    npmPlatformKey,
    pipelineTarget,
    version: generation.release.version,
    commit: generation.release.commit,
    releaseGeneration: generation.release.release_generation,
    artifactName: entry.artifact.name,
    artifactSha256: entry.artifact.sha256,
  };
}

/**
 * Resolves every known npm platform key at once, for one app/version/commit.
 * Never throws: each key's result is either `{ ok: true, ...artifact }` or
 * `{ ok: false, errorType, reason }`, so a caller (e.g. a future publish-time
 * script) can see the true state of every platform in one pass without a
 * missing target aborting the others.
 */
export function resolveAllNpmPlatformArtifacts({ repoRoot, app, version, commit }) {
  const releaseId = computeReleaseId({ app, version, commit });
  const results = {};
  for (const npmPlatformKey of Object.keys(NPM_PLATFORM_TO_PIPELINE_TARGET)) {
    try {
      results[npmPlatformKey] = { ok: true, ...resolveNpmPlatformArtifact({ repoRoot, releaseId, npmPlatformKey }) };
    } catch (error) {
      results[npmPlatformKey] = { ok: false, errorType: error.constructor.name, reason: error.message };
    }
  }
  return { releaseId, results };
}
