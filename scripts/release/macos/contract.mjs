import { readFile } from "node:fs/promises";
import { existsSync, readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import os from "node:os";
import path from "node:path";
import { validateReceipt as validateSharedPlatformAcceptanceReceipt } from "../verify-platform-artifacts.mjs";

// The canonical macOS build/sign/notarize/harden/seal path is RightKit:
// `pnpm release:doctor` then `pnpm release:build:mac` (right-release build
// --platform mac), configured by apps/membrane-hub/right-release.config.mjs.
// That command already:
//   - builds via `pnpm run rightkit:package:mac`
//     (apps/membrane-hub/scripts/build-mac-release.mjs), which runs
//     `tauri build --bundles app,dmg` (codesigns crypt-service, the membrane
//     sidecar, and the outer Membrane Hub.app inner-to-outer, with hardened
//     runtime, using the identity in
//     apps/membrane-hub/src-tauri/tauri.conf.json#bundle.macOS.signingIdentity),
//     then `xcrun notarytool submit --wait`, `xcrun stapler staple`/`validate`
//     on the DMG, and a `spctl -a -vv` Gatekeeper assessment of the DMG;
//   - runs RightKit's own hardeningscan.mjs over the sealed artifacts
//     (leaked local paths, secrets, debug symbols);
//   - seals the result under `.right-release/sealed/<app>-<version>-<commit>/mac/`
//     with a `release-manifest.json` binding every artifact's sha256.
//
// This module does not re-sign, re-notarize, or re-staple anything RightKit
// already did. It only (a) builds/validates the membrane-specific
// `orthic.membrane.platform-acceptance.v1` acceptance receipt the book gate
// consumes (RightKit has no such receipt type — it seals artifacts, it does
// not attest install/startup/update/uninstall lifecycle outcomes), and
// (b) reads back what RightKit actually sealed, so the acceptance layer in
// release-macos.mjs verifies RightKit's real output instead of a bespoke
// substitute for it.

export async function readJson(filePath) {
  try { return JSON.parse(await readFile(path.resolve(filePath), "utf8")); }
  catch (error) { throw new Error(`invalid JSON input: ${filePath}: ${error.message}`); }
}

// Reuses the shared cross-platform validator (scripts/release/verify-platform-artifacts.mjs)
// so a macOS receipt built here is guaranteed to satisfy the same schema the
// book gate checks. This wrapper only additionally pins platform: "macos".
export function validatePlatformAcceptanceReceipt(receipt) {
  const validated = validateSharedPlatformAcceptanceReceipt(receipt);
  if (validated.platform !== "macos") throw new Error("receipt platform must be macos");
  return validated;
}

export function buildPlatformAcceptanceReceipt(input) {
  const receipt = {
    schema: "orthic.membrane.platform-acceptance.v1",
    receiptId: input.receiptId,
    mode: input.mode,
    commit: input.commit,
    releaseGeneration: input.releaseGeneration,
    version: input.version,
    platform: "macos",
    artifact: { name: input.artifactName, sha256: input.artifactSha256 },
    trust: { codesign: input.codesign, notarization: input.notarization, staple: input.staple, gatekeeper: input.gatekeeper },
    lifecycle: { install: input.install, startup: input.startup, update: input.update, uninstall: input.uninstall },
    environment: { clean: input.clean, machineDigest: input.machineDigest, bypassWarnings: input.bypassWarnings },
  };
  return validatePlatformAcceptanceReceipt(receipt);
}

// --- Read back what RightKit actually sealed --------------------------------

const SEALED_SCHEMA = 1;

export function sealedReleaseDir({ repoRoot, app, version, commit }) {
  if (!/^[0-9a-f]{40}$/.test(commit ?? "")) throw new Error("commit must be a 40-character git SHA");
  if (!app) throw new Error("app is required");
  if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) throw new Error("version must be semver");
  const releaseId = `${app}-${version}-${commit.slice(0, 8)}`;
  return path.resolve(repoRoot, ".right-release", "sealed", releaseId, "mac");
}

// RightKit's own sealRelease() (tools/rightkit/packages/release/build-release.mjs)
// writes this file; this reader only validates and locates the installer
// entry within it, it never writes or mutates a sealed release.
export function readSealedReleaseManifest(sealedDir) {
  const manifestPath = path.resolve(sealedDir, "release-manifest.json");
  if (!existsSync(manifestPath)) {
    throw new Error(`sealed release manifest not found (run: pnpm release:build:mac): ${manifestPath}`);
  }
  let manifest;
  try { manifest = JSON.parse(readFileSync(manifestPath, "utf8")); }
  catch (error) { throw new Error(`sealed release manifest invalid: ${manifestPath}: ${error.message}`); }
  if (manifest.schema !== SEALED_SCHEMA) throw new Error("sealed release manifest schema invalid");
  if (manifest.platform !== "mac") throw new Error("sealed release manifest platform must be mac");
  if (!Array.isArray(manifest.checkpoints) || !manifest.checkpoints.includes("sealed")) {
    throw new Error("sealed release manifest is missing the sealed checkpoint");
  }
  const installer = (manifest.files ?? []).find(
    (file) => String(file.role).split("+").includes("installer") && /\.dmg$/i.test(file.name),
  );
  if (!installer) throw new Error("sealed release manifest has no installer .dmg entry");
  return { manifest, manifestPath, installer };
}

export function verifySealedInstallerHash({ sealedDir, installer }) {
  const dmgPath = path.resolve(sealedDir, installer.name);
  if (!existsSync(dmgPath)) throw new Error(`sealed installer missing on disk: ${dmgPath}`);
  const actual = createHash("sha256").update(readFileSync(dmgPath)).digest("hex");
  if (actual !== installer.sha256) throw new Error(`sealed installer hash mismatch: ${dmgPath}`);
  return dmgPath;
}

// --- notarytool credential-argument shape ------------------------------------
//
// apps/membrane-hub/scripts/build-mac-release.mjs (the `pnpm
// rightkit:package:mac` step RightKit's build invokes) already authenticates
// with notarytool via `notarytoolAuthArgs()` from `@rightkit/release/notary-auth.mjs`
// (default keychain profile "apple-dev-notary", or the APPLE_API_KEY_PATH /
// APPLE_API_KEY / APPLE_API_ISSUER triplet). This membrane repo is its own
// standalone Git checkout — it has no workspace or file link to
// tools/rightkit, and RightKit's own rules forbid apps depending on it by
// anything but a published npm version — so this ~15-line helper cannot be
// imported across that boundary. It intentionally mirrors that function's
// argument shape and default profile name so this evidence-capture step
// authenticates with the exact same credentials the real build already used,
// instead of the prior pipeline's disconnected `MEMBRANE_MACOS_NOTARY_PROFILE`
// env var (which named a profile nothing in the real build ever read).
export const NOTARY_KEYCHAIN_PROFILE_DEFAULT = "apple-dev-notary";

export function notarytoolAuthArgs({ profile = NOTARY_KEYCHAIN_PROFILE_DEFAULT } = {}) {
  const keyPath = process.env.APPLE_API_KEY_PATH?.trim();
  const keyId = process.env.APPLE_API_KEY?.trim();
  const issuer = process.env.APPLE_API_ISSUER?.trim();
  const apiValues = [keyPath, keyId, issuer];

  if (apiValues.every(Boolean)) {
    return ["--key", expandHome(keyPath), "--key-id", keyId, "--issuer", issuer];
  }
  if (apiValues.some(Boolean)) {
    throw new Error("Notarization API auth requires APPLE_API_KEY_PATH, APPLE_API_KEY, and APPLE_API_ISSUER together");
  }
  return ["--keychain-profile", profile];
}

function expandHome(value) {
  return value === "~" ? os.homedir() : value.startsWith("~/") ? path.join(os.homedir(), value.slice(2)) : value;
}
