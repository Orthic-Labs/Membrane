import { readFile } from "node:fs/promises";
import { basename, resolve } from "node:path";

export const INSTALLER_NAME = /^Membrane_[0-9]+\.[0-9]+\.[0-9]+_x64-setup\.exe$/;
export const PAYLOAD_NAME = /^[A-Za-z0-9_.-]+\.(exe|dll)$/;

export async function readJson(path) {
  try { return JSON.parse(await readFile(resolve(path), "utf8")); }
  catch (error) { throw new Error(`invalid JSON input: ${path}: ${error.message}`); }
}

// --- Legacy compatibility shim --------------------------------------------
//
// validateReleaseManifest / signingPlan / validateReceipt below are the
// original MBR-902 exports (commit 2ff0a4be, "MBR-902: prepare Windows
// release contract"), kept byte-for-byte. They exist only because
// tests/release/windows.test.mjs -- at the repo root, outside this repair's
// allowed file set (packaging/windows/**, scripts/release/windows/**,
// docs/release/windows.md, evidence/productization/MBR-902/**) -- imports
// them directly, and this repair must not touch that file.
//
// They are NOT the canonical signing path. signingPlan's AzureSignTool/
// SignTool argv construction (the /dlib, /dmdf, timestamp-authority URL,
// etc.) duplicates what RightKit's tools/rightkit/packages/release/
// sign-windows.mjs already does end to end, invoked automatically by
// `pnpm --dir apps/membrane-hub release:build:win` (right-release build
// --platform win). No script in this repository calls signingPlan to
// actually sign anything. New code uses validateSealedManifest /
// validateLifecycleReceipt below instead, which verify what RightKit
// itself produced rather than re-deriving a signing plan.
export function validateReleaseManifest(manifest) {
  const required = ["product_version", "source_commit", "installer_path", "installer_sha256"];
  for (const key of required) if (typeof manifest?.[key] !== "string" || !manifest[key]) throw new Error(`missing manifest field: ${key}`);
  if (!INSTALLER_NAME.test(basename(manifest.installer_path))) throw new Error("installer must be Membrane_<version>_x64-setup.exe");
  if (basename(manifest.installer_path) !== `Membrane_${manifest.product_version}_x64-setup.exe`) throw new Error("installer version must match product_version");
  if (!/^[0-9a-f]{40}$/.test(manifest.source_commit)) throw new Error("source_commit must be a 40-character git SHA");
  if (!/^[0-9a-f]{64}$/.test(manifest.installer_sha256)) throw new Error("installer_sha256 must be SHA-256");
  if (manifest.signing?.provider !== "azure-artifact-signing") throw new Error("signing.provider must be azure-artifact-signing");
  if (manifest.signing?.trust !== "public") throw new Error("signing.trust must be public");
  if (manifest.signing?.timestamp !== "rfc3161") throw new Error("signing.timestamp must be rfc3161");
  return manifest;
}

export function signingPlan(manifest, inputPath) {
  validateReleaseManifest(manifest);
  if (basename(inputPath) !== basename(manifest.installer_path)) throw new Error("input installer does not match manifest identity");
  return { tool: "signtool", args: ["sign", "/fd", "SHA256", "/tr", "http://timestamp.acs.microsoft.com", "/td", "SHA256", "/dlib", "<Azure.CodeSigning.Dlib.dll>", "/dmdf", "<artifact-signing-metadata.json>", inputPath], requires: ["DefaultAzureCredential", "Azure.CodeSigning.Dlib.dll", "artifact-signing-metadata.json"], verify: ["signtool", "verify", "/pa", "/tw", inputPath] };
}

export function validateReceipt(receipt, manifest) {
  validateReleaseManifest(manifest);
  if (receipt?.schema !== "windows-installer-receipt.v1" || receipt.status !== "pass") throw new Error("receipt must be windows-installer-receipt.v1 with status pass");
  if (receipt.source_commit !== manifest.source_commit || receipt.installer_sha256 !== manifest.installer_sha256) throw new Error("receipt identity does not match manifest");
  for (const gate of ["signature", "install", "update", "uninstall"]) if (receipt.gates?.[gate] !== "pass") throw new Error(`receipt gate not passed: ${gate}`);
  return receipt;
}

// --- RightKit-delegated verification layer --------------------------------
//
// Build and signing are RightKit's job, not this repository's:
//   pnpm --dir apps/membrane-hub release:doctor -- --platform win   (preflight)
//   pnpm --dir apps/membrane-hub release:build:win                  (build+sign+harden+seal)
// right-release build runs the app's rightkit:package:win script (tauri
// build --bundles nsis), then RightKit's own sign-windows.mjs against
// apps/membrane-hub/right-release.config.mjs's targets.win.sign.files
// (Azure Artifact Signing, SHA-256 digest, RFC3161 timestamp, signtool
// /dlib), then RightKit's hardeningscan.mjs, then seals a schema:1
// release-manifest.json. Nothing below re-implements any of that; it only
// verifies the manifest RightKit itself sealed.
//
// What RightKit does not provide, and what this module still owns:
//  1. Proof that a specific sealed release actually reached the signed/
//     hardened/sealed checkpoints (validateSealedManifest) -- reads
//     RightKit's own release-manifest.json shape, never a bespoke schema.
//  2. Proof that every inner PE the NSIS installer embeds -- not only the
//     outer installer RightKit's sign.files/installer.artifacts currently
//     cover -- was itself signtool-verified (/pa /tw), plus proof of a
//     clean-machine install/update/uninstall (validateLifecycleReceipt).
//     See docs/release/windows.md for why RightKit does not close this gap
//     today: sign.files runs after NSIS has already embedded the main
//     binary and the externalBin sidecars, so signing the files on disk
//     afterward never reaches the copies already inside the installer.

export function validateSealedManifest(manifest) {
  if (manifest?.schema !== 1) throw new Error("expected a RightKit sealed release-manifest.json (schema 1)");
  for (const key of ["releaseId", "app", "version", "commit", "platform", "files", "checkpoints"]) {
    if (manifest[key] === undefined) throw new Error(`RightKit release-manifest missing field: ${key}`);
  }
  if (manifest.platform !== "win") throw new Error("release-manifest platform must be win");
  if (!/^[0-9a-f]{40}$/.test(manifest.commit ?? "")) throw new Error("release-manifest commit must be a 40-character git SHA");
  for (const checkpoint of ["signed", "hardened", "sealed"]) {
    if (!Array.isArray(manifest.checkpoints) || !manifest.checkpoints.includes(checkpoint)) {
      throw new Error(`RightKit release-manifest missing checkpoint: ${checkpoint}`);
    }
  }
  if (!Array.isArray(manifest.files)) throw new Error("release-manifest.files must be an array");
  const installer = manifest.files.find((file) => String(file?.role ?? "").split("+").includes("installer"));
  if (!installer) throw new Error("release-manifest has no file with role installer");
  if (!INSTALLER_NAME.test(installer.name ?? "")) throw new Error("installer must be Membrane_<version>_x64-setup.exe");
  if (installer.name !== `Membrane_${manifest.version}_x64-setup.exe`) throw new Error("installer name must match release-manifest version");
  if (!/^[0-9a-f]{64}$/.test(installer.sha256 ?? "")) throw new Error("installer sha256 must be SHA-256");
  return { manifest, installer };
}

// Tauri's `bundle.externalBin` sidecars plus the one main compiled binary
// are the PEs NSIS embeds inside the installer (see
// apps/membrane-hub/src-tauri/windows/installer.nsi's "Copy main
// executable" and "Copy external binaries" sections). RightKit has no
// concept of these; this is the one place that still has to look at
// Tauri's own config to know the minimum inner-PE count. Read-only --
// this repair never edits apps/membrane-hub/**.
export async function minimumExpectedPayloadCount(tauriConfigPath) {
  const config = await readJson(tauriConfigPath);
  const externalBin = config?.bundle?.externalBin ?? [];
  return externalBin.length + 1;
}

export function validateExpectedPayloads(payloads, minimumCount = 1) {
  if (!Array.isArray(payloads) || payloads.length < minimumCount) {
    throw new Error(`expected inner-PE payload list must name at least ${minimumCount} PE(s) (RightKit's sealed manifest does not enumerate NSIS-embedded binaries); got ${Array.isArray(payloads) ? payloads.length : 0}`);
  }
  for (const name of payloads) if (!PAYLOAD_NAME.test(name)) throw new Error(`expected payload name must be a .exe or .dll: ${name}`);
  return payloads;
}

// The full acceptance proof: RightKit's sealed manifest (outer installer,
// signed+hardened+sealed checkpoints) plus a receipt proving every expected
// inner PE was signtool-verified and the clean-machine install/update/
// uninstall lifecycle passed. Fails closed on any missing or mismatched
// proof, exactly like validateReceipt above, but against RightKit's real
// manifest shape and extended to the payload level.
export function validateLifecycleReceipt(receipt, sealed, expectedPayloads) {
  const { manifest, installer } = validateSealedManifest(sealed);
  validateExpectedPayloads(expectedPayloads);
  if (receipt?.schema !== "windows-installer-receipt.v1" || receipt.status !== "pass") throw new Error("receipt must be windows-installer-receipt.v1 with status pass");
  if (receipt.source_commit !== manifest.commit) throw new Error("receipt source_commit does not match RightKit sealed release-manifest commit");
  if (receipt.installer_sha256 !== installer.sha256) throw new Error("receipt installer_sha256 does not match RightKit sealed release-manifest installer entry");
  for (const gate of ["signature", "install", "update", "uninstall"]) {
    if (receipt.gates?.[gate] !== "pass") throw new Error(`receipt gate not passed: ${gate}`);
  }
  if (!Array.isArray(receipt.payloads) || receipt.payloads.length !== expectedPayloads.length) {
    throw new Error("receipt must verify every expected inner-PE payload");
  }
  for (const name of expectedPayloads) {
    const proof = receipt.payloads.find((entry) => entry.name === name);
    if (!proof) throw new Error(`receipt missing payload proof: ${name}`);
    if (!/^[0-9a-f]{64}$/.test(proof.sha256 ?? "")) throw new Error(`receipt payload sha256 must be SHA-256: ${name}`);
    if (proof.verify !== "pass") throw new Error(`receipt payload signtool verify /pa /tw not pass: ${name}`);
  }
  return receipt;
}
