// MBR-911: fail-closed dual-signature update-admission contract at
// release-orchestration time. This module signs nothing and verifies no
// signature bytes itself -- it only validates the *shape* of two artifacts
// RightKit's own release pipeline already produces, and binds them to one
// consistent artifact identity before a release may be treated as
// admissible for update publication:
//
//  1. The Tauri updater manifest entry
//     (`tools/rightkit/packages/release/publish-update.mjs`
//     `body.manifest.platforms[artifact.platform] = { signature, url }`,
//     signed by `sign-updater.mjs` / `create-mac-updater.mjs` against the
//     shared `rightsuite-updater-key`). Tauri's own updater plugin verifies
//     that signature at update-check time; this contract only asserts the
//     field is present and non-empty, the same "is there a signature at
//     all" check `apps/membrane-hub/src-tauri/src/update_admission.rs`
//     performs on the Rust side.
//  2. The `orthic.membrane.platform-acceptance.v1` receipt already defined
//     by `../verify-platform-artifacts.mjs` and built by
//     `../macos/contract.mjs` / `../windows/contract.mjs`. Reused
//     (imported), never redefined.
//
// Mirrors `engine/crates/membrane-updater::verify`'s fail-closed shape and
// failure codes exactly, so a release-time rejection here and a runtime
// rejection there always mean the same thing.
import { validateReceipt as validatePlatformAcceptanceReceipt } from "../verify-platform-artifacts.mjs";

export const FAILURE = Object.freeze({
  INVALID_IDENTITY: "invalid_update_evidence_identity",
  DOWNGRADE: "update_downgrade_rejected",
  TAURI_SIGNATURE_INVALID: "tauri_updater_signature_invalid",
  PLATFORM_TRUST_INVALID: "platform_trust_invalid",
});

// The only updater-platform-key -> receipt-platform pairing RightKit's own
// config currently declares (apps/membrane-hub/right-release.config.mjs
// targets.win.updater.artifacts[0].platform). A new platform key must be
// added here explicitly once RightKit's config declares it -- never
// inferred or guessed ahead of that config.
export const UPDATER_PLATFORM_TO_RECEIPT_PLATFORM = Object.freeze({
  "windows-x86_64": "windows",
});

const SHA256 = /^[0-9a-f]{64}$/;

export function versionTriple(version) {
  if (typeof version !== "string") return null;
  const parts = version.split(".");
  if (parts.length < 3) return null;
  const triple = parts.slice(0, 3).map((part) => {
    const digits = part.match(/^\d+/);
    return digits ? Number(digits[0]) : null;
  });
  return triple.every((value) => value !== null) ? triple : null;
}

export function isUpgrade(fromVersion, toVersion) {
  const from = versionTriple(fromVersion);
  const to = versionTriple(toVersion);
  if (!from || !to) return false;
  for (let i = 0; i < 3; i++) {
    if (to[i] > from[i]) return true;
    if (to[i] < from[i]) return false;
  }
  return false; // equal versions are not an upgrade
}

// Validates only the fields RightKit's publish-update.mjs actually emits
// per platform entry. Does not invent a signature format: `signature` is
// treated as an opaque non-empty string, exactly as Tauri's own updater
// manifest carries it.
export function validateUpdaterManifestEntry(entry) {
  if (!entry || typeof entry !== "object") throw new Error("updater manifest entry must be an object");
  if (typeof entry.signature !== "string" || entry.signature.trim() === "") {
    throw new Error("updater manifest entry missing a non-empty Tauri updater signature");
  }
  if (typeof entry.url !== "string" || !/^https:\/\//.test(entry.url)) {
    throw new Error("updater manifest entry url must be https");
  }
  return entry;
}

// Pure fail-closed admission, mirroring engine/crates/membrane-updater::verify.
// Every failure is additive to `failures`; nothing here downloads, installs,
// or mutates a release -- it only decides whether one is admissible.
export function assessDualSignatureUpdate({
  fromVersion,
  toVersion,
  artifactSha256,
  updaterPlatform,
  updaterManifestEntry,
  platformTrustReceipt,
}) {
  const failures = [];

  const identityValid =
    typeof artifactSha256 === "string" &&
    SHA256.test(artifactSha256) &&
    typeof fromVersion === "string" &&
    typeof toVersion === "string" &&
    fromVersion !== toVersion &&
    fromVersion.length > 0 &&
    toVersion.length > 0;
  if (!identityValid) failures.push(FAILURE.INVALID_IDENTITY);

  if (identityValid && !isUpgrade(fromVersion, toVersion)) {
    failures.push(FAILURE.DOWNGRADE);
  }

  try {
    validateUpdaterManifestEntry(updaterManifestEntry);
  } catch {
    failures.push(FAILURE.TAURI_SIGNATURE_INVALID);
  }

  const receiptPlatform = UPDATER_PLATFORM_TO_RECEIPT_PLATFORM[updaterPlatform];
  if (!receiptPlatform) failures.push(FAILURE.TAURI_SIGNATURE_INVALID);

  let receipt = null;
  try {
    receipt = validatePlatformAcceptanceReceipt(platformTrustReceipt);
  } catch {
    failures.push(FAILURE.PLATFORM_TRUST_INVALID);
  }
  if (receipt && receiptPlatform && receipt.platform !== receiptPlatform) {
    failures.push(FAILURE.PLATFORM_TRUST_INVALID);
  }
  if (receipt && identityValid && receipt.artifact.sha256 !== artifactSha256) {
    failures.push(FAILURE.INVALID_IDENTITY);
  }

  const outcome = failures.length ? "blocked" : "verified";
  return {
    outcome,
    fromVersion,
    toVersion,
    artifactSha256,
    failures,
    repairPath: "repair/update-signatures",
  };
}
