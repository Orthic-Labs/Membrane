#!/usr/bin/env node
// MBR-806 — Windows platform signing + installed-path acceptance gate.
//
// This module signs nothing, builds nothing, and installs nothing. It is
// the single fail-closed gate that binds two documents already produced
// elsewhere -- neither re-implemented here -- into one accept/reject
// decision:
//
//   1. The cross-platform `orthic.membrane.platform-acceptance.v1` receipt
//      (scripts/release/verify-platform-artifacts.mjs, imported and never
//      forked) -- proves the OUTER installer's Authenticode / Public-Trust
//      / RFC3161 signature and the install/startup/update/uninstall
//      lifecycle outcomes the book gate already consumes.
//   2. RightKit's real sealed `release-manifest.json` (schema 1) plus the
//      Windows-specific lifecycle receipt that proves every INNER PE (the
//      main compiled binary and every Tauri `bundle.externalBin` sidecar)
//      was individually `signtool verify /pa /tw`-checked
//      (scripts/release/windows/contract.mjs, imported and never forked).
//
// Why both are required, not #1 alone: docs/release/windows.md records a
// confirmed defect -- RightKit signs `targets.win.sign.files` strictly
// AFTER `tauri build --bundles nsis` has already embedded the main binary
// and the externalBin sidecars into the installer, so signing files on
// disk afterward never reaches the copies already inside the installer. A
// receipt satisfying #1 alone (outer installer signed) proves nothing
// about the payloads NSIS embedded -- it would pass even if every inner PE
// were unsigned. This gate fails closed unless #2 additionally proves
// every one of those payloads individually, and unless commit/installer
// identity is bound across both documents so a valid #1 cannot be paired
// with an unrelated or incomplete #2.
import {
  validateSealedManifest,
  validateExpectedPayloads,
  validateLifecycleReceipt,
  minimumExpectedPayloadCount,
  readJson,
} from "../windows/contract.mjs";
import { validateReceipt as validatePlatformAcceptanceReceipt } from "../verify-platform-artifacts.mjs";

const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };
const stripV = (version) => String(version ?? "").replace(/^v/, "");

// Pure-data gate: every argument is already-parsed JSON. Throws (never
// resolves to a partial/unknown result) on any missing, mismatched, or
// incomplete proof.
export function verifyWindowsInstalledAcceptance({
  platformReceipt,
  sealedManifest,
  expectedPayloads,
  lifecycleReceipt,
  minimumPayloadCount = 1,
}) {
  if (!platformReceipt) fail("platform-acceptance receipt is required");
  if (!sealedManifest) fail("RightKit sealed release-manifest.json is required");
  if (!lifecycleReceipt) fail("Windows lifecycle receipt (inner-PE + install/update/uninstall proof) is required");

  // #1 — the shared cross-platform receipt: outer installer trust + full lifecycle.
  const receipt = validatePlatformAcceptanceReceipt(platformReceipt);
  if (receipt.platform !== "windows") fail("platform-acceptance receipt platform must be windows");
  if (receipt.mode !== "clean-vm") fail("installed-path acceptance requires mode: clean-vm, not source-ready");

  // #2 — RightKit's real sealed manifest, plus proof every inner PE was verified.
  const { manifest, installer } = validateSealedManifest(sealedManifest);
  const payloads = Array.isArray(expectedPayloads) ? expectedPayloads : [];
  validateExpectedPayloads(payloads, minimumPayloadCount);
  validateLifecycleReceipt(lifecycleReceipt, sealedManifest, payloads);

  // Identity binding: a passing #1 must describe the exact artifact #2 proves —
  // otherwise a valid outer-installer receipt could be paired with an
  // unrelated (or incomplete) inner-PE proof.
  if (receipt.commit !== manifest.commit) fail("platform-acceptance receipt commit does not match RightKit sealed manifest commit");
  if (receipt.artifact.name !== installer.name) fail("platform-acceptance receipt artifact name does not match RightKit sealed installer");
  if (receipt.artifact.sha256 !== installer.sha256) fail("platform-acceptance receipt artifact sha256 does not match RightKit sealed installer");
  if (stripV(receipt.version) !== manifest.version) fail("platform-acceptance receipt version does not match RightKit sealed manifest version");

  return {
    status: "accepted",
    platform: "windows",
    commit: receipt.commit,
    installer: installer.name,
    installerSha256: installer.sha256,
    payloadsVerified: payloads.length,
  };
}

export async function verifyWindowsInstalledAcceptanceFromFiles({
  platformReceiptPath, sealedManifestPath, expectedPayloadsPath, lifecycleReceiptPath, tauriConfigPath,
}) {
  const [platformReceipt, sealedManifest, expectedPayloads, lifecycleReceipt] = await Promise.all([
    readJson(platformReceiptPath), readJson(sealedManifestPath), readJson(expectedPayloadsPath), readJson(lifecycleReceiptPath),
  ]);
  const minimumPayloadCount = tauriConfigPath ? await minimumExpectedPayloadCount(tauriConfigPath) : 1;
  return verifyWindowsInstalledAcceptance({ platformReceipt, sealedManifest, expectedPayloads, lifecycleReceipt, minimumPayloadCount });
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [platformReceiptPath, sealedManifestPath, expectedPayloadsPath, lifecycleReceiptPath, tauriConfigPath] = process.argv.slice(2);
  if (!platformReceiptPath || !sealedManifestPath || !expectedPayloadsPath || !lifecycleReceiptPath) {
    console.error("usage: node windows-installed-acceptance.mjs <platform-receipt.json> <release-manifest.json> <expected-payloads.json> <lifecycle-receipt.json> [tauri.conf.json]");
    console.error("");
    console.error("Fails closed unless the platform-acceptance receipt (outer installer trust +");
    console.error("lifecycle) AND the RightKit sealed manifest AND a signtool /pa /tw proof for");
    console.error("every expected inner PE all agree on the same commit/installer identity. Never");
    console.error("invokes signtool, AzureSignTool, or right-release — it only validates JSON.");
    process.exit(2);
  }
  try {
    const result = await verifyWindowsInstalledAcceptanceFromFiles({
      platformReceiptPath, sealedManifestPath, expectedPayloadsPath, lifecycleReceiptPath, tauriConfigPath,
    });
    console.log(`PASS windows installed-path acceptance: ${JSON.stringify(result)}`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
