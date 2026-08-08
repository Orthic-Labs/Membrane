// MBR-903: reads what RightKit has already sealed for one logical release
// target, if anything, and binds it to the release's expected source commit.
//
// This module never builds, signs, notarizes, or invokes right-release. It
// only re-reads RightKit's own sealed release-manifest.json (schema 1) --
// via the same functions scripts/release/macos/contract.mjs (MBR-901) and
// scripts/release/windows/contract.mjs (MBR-902) already expose for that --
// and fails closed if a sealed manifest's commit does not match the commit
// the multi-platform orchestrator resolved. That mismatch is exactly the
// failure mode Adrian's real workflow can hit: finish on the Mac, then pull
// to Windows and build there -- if the Windows machine builds before
// pulling, or from a stale branch, its sealed manifest names a different
// commit, and this must be caught, not silently accepted as "the release."
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { sealedReleaseDir, readSealedReleaseManifest, verifySealedInstallerHash } from "../macos/contract.mjs";
import { validateSealedManifest } from "../windows/contract.mjs";
import { describeTarget } from "./platform-targets.mjs";

export class ReleaseDriftError extends Error {
  constructor(message, detail) {
    super(message);
    this.name = "ReleaseDriftError";
    this.detail = detail;
  }
}

function windowsSealedManifestPath({ repoRoot, app, version, commit }) {
  if (!/^[0-9a-f]{40}$/.test(commit ?? "")) throw new Error("FAIL CLOSED: commit must be a 40-character git SHA");
  const releaseId = `${app}-${version}-${commit.slice(0, 8)}`;
  return resolve(repoRoot, ".right-release", "sealed", releaseId, "windows", "release-manifest.json");
}

/**
 * Reports the sealed-artifact stage for one logical target, for one
 * expected commit. Status is one of:
 *   - "not-buildable": no RightKit platform build exists for this target
 *     today (see platform-targets.mjs); never fabricated as verified.
 *   - "pending": buildable, but nothing sealed on this machine yet.
 *   - "verified": a sealed manifest exists, its commit matches, and its
 *     installer hash was independently recomputed and matched (mac) or its
 *     shape validated (windows).
 * A commit mismatch throws ReleaseDriftError -- it is never downgraded to
 * "pending", because the artifact DOES exist, it is just for the wrong
 * commit, which is a stronger, more actionable failure than "not built yet."
 */
export function readSealedPlatformStage({ repoRoot, app, version, commit }, target) {
  const info = describeTarget(target);
  if (!info.buildable) return { target, status: "not-buildable", reason: info.reason };

  if (info.rightKitPlatform === "mac") {
    let sealedDir;
    try {
      sealedDir = sealedReleaseDir({ repoRoot, app, version, commit });
    } catch (error) {
      throw new Error(`FAIL CLOSED: ${target}: ${error.message}`);
    }
    if (!existsSync(resolve(sealedDir, "release-manifest.json"))) return { target, status: "pending", sealedDir };
    const { manifest, installer } = readSealedReleaseManifest(sealedDir);
    if (manifest.commit !== commit) {
      throw new ReleaseDriftError(
        `FAIL CLOSED: ${target} sealed release was built from a different commit than this release (expected ${commit}, sealed manifest has ${manifest.commit})`,
        { target, expectedCommit: commit, sealedCommit: manifest.commit, sealedDir },
      );
    }
    const dmgPath = verifySealedInstallerHash({ sealedDir, installer });
    return { target, status: "verified", sealedDir, artifact: { name: installer.name, sha256: installer.sha256, path: dmgPath } };
  }

  if (info.rightKitPlatform === "win") {
    const manifestPath = windowsSealedManifestPath({ repoRoot, app, version, commit });
    if (!existsSync(manifestPath)) return { target, status: "pending", manifestPath };
    let manifest;
    try {
      manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    } catch (error) {
      throw new Error(`FAIL CLOSED: ${target}: sealed release manifest invalid: ${manifestPath}: ${error.message}`);
    }
    const { installer } = validateSealedManifest(manifest);
    if (manifest.commit !== commit) {
      throw new ReleaseDriftError(
        `FAIL CLOSED: ${target} sealed release was built from a different commit than this release (expected ${commit}, sealed manifest has ${manifest.commit})`,
        { target, expectedCommit: commit, sealedCommit: manifest.commit, manifestPath },
      );
    }
    return { target, status: "verified", manifestPath, artifact: { name: installer.name, sha256: installer.sha256 } };
  }

  throw new Error(`FAIL CLOSED: ${target} has no known RightKit platform mapping`);
}

export function readAllSealedStages({ repoRoot, app, version, commit }, targets) {
  return targets.map((target) => readSealedPlatformStage({ repoRoot, app, version, commit }, target));
}
