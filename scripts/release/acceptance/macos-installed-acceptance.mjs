#!/usr/bin/env node
// MBR-806 — macOS platform signing + installed-path acceptance gate.
//
// This module signs, notarizes, and installs nothing. It binds three
// documents already produced elsewhere -- none re-implemented here -- into
// one fail-closed accept/reject decision:
//
//   1. The cross-platform `orthic.membrane.platform-acceptance.v1` receipt
//      (scripts/release/verify-platform-artifacts.mjs, via
//      scripts/release/macos/contract.mjs's `validatePlatformAcceptanceReceipt`,
//      imported and never forked) -- codesign, notarization, staple,
//      Gatekeeper trust, plus install/startup/update/uninstall lifecycle
//      outcomes.
//   2. RightKit's real sealed `release-manifest.json`, read back (never
//      re-signed) via `readSealedReleaseManifest` / `verifySealedInstallerHash`
//      (scripts/release/macos/contract.mjs) -- cross-checks the receipt's
//      artifact is byte-for-byte what RightKit actually sealed for this
//      exact commit/version, not an arbitrary same-named file.
//   3. The macOS release receipt (`orthic.membrane.macos-release-receipt.v1`)
//      that `scripts/release/macos/release-macos.mjs`'s `receipt` command
//      writes, re-verified read-only against the actual `.app`/`.dmg`
//      bytes by that same script's `verify-receipt` command (spawned as a
//      subprocess and reused rather than reimplemented — the app-directory
//      sha256 walk it performs is internal to that script; `verify-receipt`
//      never shells out to codesign/spctl/hdiutil/xcrun, it only compares
//      recorded hashes against the files on disk).
//
// Unlike the Windows NSIS installer, RightKit/Tauri codesign every
// `bundle.externalBin` sidecar and the outer `.app` inner-to-outer, with
// hardened runtime, before notarization (docs/release/macos.md) — so there
// is no equivalent "outer signed, inner unsigned" gap here. This gate still
// fails closed on every other way a "signed" claim could be hollow: a DMG
// that does not match RightKit's sealed hash, a receipt whose recorded
// hashes do not match the actual `.app`/`.dmg` bytes, incomplete trust or
// lifecycle fields, or a `source-ready` receipt masquerading as installed
// acceptance.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  readSealedReleaseManifest,
  verifySealedInstallerHash,
  validatePlatformAcceptanceReceipt,
} from "../macos/contract.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const releaseMacosScript = resolve(here, "../macos/release-macos.mjs");

const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };
const stripV = (version) => String(version ?? "").replace(/^v/, "");

export function verifyMacosInstalledAcceptance({ platformReceipt, sealedDir, appPath, dmgPath, macosReceiptPath }) {
  if (!platformReceipt) fail("platform-acceptance receipt is required");
  if (!sealedDir) fail("RightKit sealed release directory is required");
  if (!appPath || !dmgPath) fail("--app and --dmg are required");
  if (!macosReceiptPath) fail("macOS release receipt (orthic.membrane.macos-release-receipt.v1) is required");

  // #1 — the shared cross-platform receipt: trust + full lifecycle.
  const receipt = validatePlatformAcceptanceReceipt(platformReceipt);
  if (receipt.mode !== "clean-vm") fail("installed-path acceptance requires mode: clean-vm, not source-ready");

  // #2 — RightKit's real sealed manifest, read back, never re-signed.
  const { manifest, installer } = readSealedReleaseManifest(sealedDir);
  const sealedDmgPath = verifySealedInstallerHash({ sealedDir, installer });

  if (receipt.commit !== manifest.commit) fail("platform-acceptance receipt commit does not match RightKit sealed manifest commit");
  if (receipt.artifact.name !== installer.name) fail("platform-acceptance receipt artifact name does not match RightKit sealed installer");
  if (receipt.artifact.sha256 !== installer.sha256) fail("platform-acceptance receipt artifact sha256 does not match RightKit sealed installer");
  if (stripV(receipt.version) !== manifest.version) fail("platform-acceptance receipt version does not match RightKit sealed manifest version");
  if (resolve(dmgPath) !== resolve(sealedDmgPath)) fail("--dmg does not resolve to RightKit's sealed installer path");

  // #3 — the macOS release receipt, re-verified read-only against the actual
  // .app/.dmg bytes by release-macos.mjs's own verify-receipt command
  // (reused, not reimplemented; never touches codesign/spctl/hdiutil/xcrun).
  if (!existsSync(macosReceiptPath)) fail(`macOS release receipt not found: ${macosReceiptPath}`);
  const result = spawnSync(process.execPath, [
    releaseMacosScript, "verify-receipt", "--app", appPath, "--dmg", dmgPath, "--receipt", macosReceiptPath,
  ], { encoding: "utf8" });
  if (result.status !== 0) fail(`macOS release receipt did not verify against --app/--dmg: ${(result.stderr || result.stdout || "unknown error").trim()}`);

  return {
    status: "accepted",
    platform: "macos",
    commit: receipt.commit,
    installer: installer.name,
    installerSha256: installer.sha256,
  };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [platformReceiptPath, sealedDir, appPath, dmgPath, macosReceiptPath] = process.argv.slice(2);
  if (!platformReceiptPath || !sealedDir || !appPath || !dmgPath || !macosReceiptPath) {
    console.error("usage: node macos-installed-acceptance.mjs <platform-receipt.json> <sealed-dir> <app-path> <dmg-path> <macos-receipt.json>");
    console.error("");
    console.error("Fails closed unless the platform-acceptance receipt (trust + lifecycle) AND");
    console.error("RightKit's sealed release-manifest.json AND a macOS release receipt whose");
    console.error("recorded hashes match the actual .app/.dmg bytes all agree on the same");
    console.error("commit/installer identity. Never invokes codesign, notarytool, spctl,");
    console.error("hdiutil, xcrun, or stapler — those already ran inside `pnpm release:build:mac`.");
    process.exit(2);
  }
  try {
    const platformReceipt = JSON.parse(await readFile(resolve(platformReceiptPath), "utf8"));
    const result = verifyMacosInstalledAcceptance({ platformReceipt, sealedDir: resolve(sealedDir), appPath: resolve(appPath), dmgPath: resolve(dmgPath), macosReceiptPath: resolve(macosReceiptPath) });
    console.log(`PASS macOS installed-path acceptance: ${JSON.stringify(result)}`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
