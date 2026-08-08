// Reads the *real* Windows release artifact this app already produces:
// RightKit's sealed release manifest (`.right-release/sealed/<releaseId>/windows/release-manifest.json`),
// written by tools/rightkit/packages/release/build-release.mjs from
// apps/membrane-hub/right-release.config.mjs's `targets.win` block
// (`src-tauri/target/release/bundle/nsis/Membrane_<version>_x64-setup.exe`,
// installer key `membrane/installers/windows/current/Membrane_x64-setup.exe`).
//
// This module never invents a version, commit, URL, or hash: every value it
// returns is copied from that sealed JSON file (or the environment-overridable
// public download base RightKit's own upload tool uses), after independently
// re-verifying the manifest's own integrity the same way RightKit's
// `verifySealedRelease` does. It is a same-repo *reader* of RightKit's output,
// not an import of RightKit's package internals -- apps consume RightKit's
// published output, never its parent paths (tools/rightkit/docs/agent-rules.md).

import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

// Identical default to tools/rightkit/packages/release/pub-upload.mjs's
// PUBLIC_BASE -- the real public R2 download base RightKit uploads to and
// verifies against. Overridable by the same environment variable for parity
// with that script, never a second, different constant.
export const PUBLIC_DOWNLOAD_BASE = (process.env.RIGHTAPPS_PUBLIC_DOWNLOAD_BASE || "https://pub-6c73208d46c245a9b4881d5e02f6b618.r2.dev").replace(/\/$/, "");

const COMMIT_RE = /^[0-9a-f]{40}$/;
const SHA256_RE = /^[0-9a-f]{64}$/;
const REQUIRED_CHECKPOINTS = ["preflight_complete", "build_complete", "signed", "hardened", "sealed"];

export function sealedWindowsDir(root, app, version, commit) {
  if (!COMMIT_RE.test(commit)) throw new Error("commit must be a 40-character git SHA");
  const releaseId = `${app}-${version}-${commit.slice(0, 8)}`;
  return join(root, ".right-release", "sealed", releaseId, "windows");
}

/**
 * Loads `<sealedDir>/release-manifest.json` without trusting it yet -- callers
 * must pass the result through verifySealedWindowsManifest before deriving
 * any channel artifact from it. Returns null (never throws) when no sealed
 * release exists at that path, because "no Windows release has been sealed
 * yet" is the true, expected state of this checkout today, not an error.
 */
export function readSealedWindowsManifest(sealedDir) {
  const manifestPath = join(sealedDir, "release-manifest.json");
  if (!existsSync(manifestPath)) return null;
  return JSON.parse(readFileSync(manifestPath, "utf8"));
}

/**
 * Re-derives the same integrity proof RightKit's own
 * tools/rightkit/packages/release/release-state.mjs#verifySealedRelease
 * requires before any upload/publish step may run: schema 1, every required
 * build checkpoint present (through "sealed"), and every listed file's
 * bytes on disk still hashing/sizing to what the manifest recorded. This is
 * a same-shape reimplementation reading only the committed JSON contract
 * RightKit writes, not an import of RightKit's package -- membrane consumes
 * RightKit's release *output*, never its source.
 */
export function verifySealedWindowsManifest(manifest, sealedDir) {
  if (manifest?.schema !== 1) throw new Error("sealed release manifest: unsupported schema");
  if (manifest.app !== "membrane-hub") throw new Error("sealed release manifest: unexpected app");
  if (manifest.platform !== "win") throw new Error("sealed release manifest: platform must be win");
  if (!COMMIT_RE.test(manifest.commit)) throw new Error("sealed release manifest: commit must be a 40-character git SHA");
  if (typeof manifest.version !== "string" || !/^\d+\.\d+\.\d+$/.test(manifest.version)) throw new Error("sealed release manifest: version must be strict SemVer");
  for (const checkpoint of REQUIRED_CHECKPOINTS) {
    if (!manifest.checkpoints?.includes(checkpoint)) throw new Error(`sealed release manifest: missing checkpoint ${checkpoint}`);
  }
  if (!Array.isArray(manifest.files) || manifest.files.length === 0) throw new Error("sealed release manifest: no files recorded");
  for (const file of manifest.files) {
    const filePath = join(sealedDir, file.name);
    if (!existsSync(filePath)) throw new Error(`sealed release manifest: file missing on disk: ${file.name}`);
    const bytes = readFileSync(filePath);
    if (createHash("sha256").update(bytes).digest("hex") !== file.sha256) throw new Error(`sealed release manifest: hash mismatch: ${file.name}`);
    if (statSync(filePath).size !== file.sizeBytes) throw new Error(`sealed release manifest: size mismatch: ${file.name}`);
  }
  return manifest;
}

/**
 * Extracts the one Windows channel artifact this app ships today from an
 * already-verified sealed manifest: the single NSIS x64 installer role.
 * apps/membrane-hub/right-release.config.mjs defines exactly one Windows
 * target (`windowsInstaller`, x64 only) -- no arm64 build exists. If the
 * sealed manifest ever carries more than one "installer"-role file, or one
 * whose name does not match the x64 NSIS pattern, this fails closed with a
 * named "unsupported architecture" gap instead of guessing which one to
 * publish or silently ignoring the extra artifact.
 */
export function deriveWindowsChannelArtifact(manifest) {
  const installerFiles = manifest.files.filter((file) => file.role === "installer");
  if (installerFiles.length === 0) throw new Error("sealed release manifest: no installer-role file present");
  if (installerFiles.length > 1) {
    throw new Error(`declared gap: sealed release manifest carries ${installerFiles.length} installer artifacts; this generator only supports the single x64 NSIS installer RightKit's windows target produces today`);
  }
  const [installer] = installerFiles;
  const expectedName = `Membrane_${manifest.version}_x64-setup.exe`;
  if (installer.name !== expectedName) {
    throw new Error(`declared gap: installer artifact "${installer.name}" is not the x64 NSIS installer this generator supports (expected ${expectedName}); a non-x64 or non-NSIS Windows artifact is not a supported channel target yet`);
  }
  if (!SHA256_RE.test(installer.sha256)) throw new Error("sealed release manifest: installer sha256 malformed");

  const route = (manifest.routes?.patch ?? []).find((entry) => entry.role === "installer" && entry.name === installer.name);
  if (!route?.key) throw new Error("sealed release manifest: no public installer route for the sealed artifact");
  if (route.bucket !== "public") throw new Error("sealed release manifest: installer route must be the public bucket for a channel download URL");

  return {
    version: manifest.version,
    commit: manifest.commit,
    releaseGeneration: `${manifest.app}-${manifest.version}-${manifest.commit.slice(0, 8)}`,
    installerName: installer.name,
    installerSha256: installer.sha256,
    installerUrl: `${PUBLIC_DOWNLOAD_BASE}/${route.key}`,
  };
}

/**
 * Convenience wrapper: locate, load, verify, and derive in one call for a
 * given app/version/commit. Returns null when nothing is sealed yet (the
 * true state of this checkout), so callers can distinguish "no release to
 * publish" from a verification failure on a release that does exist.
 */
export function loadWindowsChannelArtifact({ root, app = "membrane-hub", version, commit }) {
  const sealedDir = sealedWindowsDir(root, app, version, commit);
  const manifest = readSealedWindowsManifest(sealedDir);
  if (!manifest) return null;
  verifySealedWindowsManifest(manifest, sealedDir);
  return deriveWindowsChannelArtifact(manifest);
}
