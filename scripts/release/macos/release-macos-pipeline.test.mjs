import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import { join, resolve } from "node:path";
import {
  buildPlatformAcceptanceReceipt,
  sealedReleaseDir, readSealedReleaseManifest, verifySealedInstallerHash,
  notarytoolAuthArgs, NOTARY_KEYCHAIN_PROFILE_DEFAULT,
} from "./contract.mjs";

const root = resolve(import.meta.dirname, "../../..");
const script = resolve(root, "scripts/release/macos/release-macos.mjs");

// -- platform-acceptance receipt bridge (scripts/release/verify-platform-artifacts.mjs) --

const passingReceiptInput = {
  receiptId: "mbr-901-fixture", mode: "clean-vm", commit: "a".repeat(40),
  releaseGeneration: "b".repeat(64), version: "1.0.0",
  artifactName: "Membrane.dmg", artifactSha256: "c".repeat(64),
  codesign: "pass", notarization: "pass", staple: "pass", gatekeeper: "pass",
  install: "pass", startup: "pass", update: "pass", uninstall: "pass",
  clean: true, machineDigest: "d".repeat(64), bypassWarnings: false,
};

test("buildPlatformAcceptanceReceipt produces a receipt the shared platform-acceptance validator accepts", () => {
  const receipt = buildPlatformAcceptanceReceipt(passingReceiptInput);
  assert.equal(receipt.schema, "orthic.membrane.platform-acceptance.v1");
  assert.equal(receipt.platform, "macos");
  assert.equal(receipt.trust.gatekeeper, "pass");
  assert.equal(receipt.lifecycle.update, "pass");
});

test("a receipt is rejected closed when gatekeeper, update, or clean-vm isolation is not proven", () => {
  assert.throws(() => buildPlatformAcceptanceReceipt({ ...passingReceiptInput, gatekeeper: undefined }), /macOS trust receipt incomplete/);
  assert.throws(() => buildPlatformAcceptanceReceipt({ ...passingReceiptInput, update: "fail" }), /lifecycle receipt incomplete: update/);
  assert.throws(() => buildPlatformAcceptanceReceipt({ ...passingReceiptInput, clean: false }), /clean-vm receipt needs isolated no-bypass evidence/);
});

// -- notarytool credential-argument shape (mirrors @rightkit/release/notary-auth.mjs, see contract.mjs) --

test("notarytoolAuthArgs defaults to RightKit's own keychain profile name", () => {
  const previous = { path: process.env.APPLE_API_KEY_PATH, id: process.env.APPLE_API_KEY, issuer: process.env.APPLE_API_ISSUER };
  delete process.env.APPLE_API_KEY_PATH; delete process.env.APPLE_API_KEY; delete process.env.APPLE_API_ISSUER;
  try {
    assert.deepEqual(notarytoolAuthArgs(), ["--keychain-profile", NOTARY_KEYCHAIN_PROFILE_DEFAULT]);
    assert.equal(NOTARY_KEYCHAIN_PROFILE_DEFAULT, "apple-dev-notary");
  } finally {
    if (previous.path !== undefined) process.env.APPLE_API_KEY_PATH = previous.path;
    if (previous.id !== undefined) process.env.APPLE_API_KEY = previous.id;
    if (previous.issuer !== undefined) process.env.APPLE_API_ISSUER = previous.issuer;
  }
});

test("notarytoolAuthArgs fails closed on a partial API-key triplet", () => {
  const previous = { path: process.env.APPLE_API_KEY_PATH, id: process.env.APPLE_API_KEY, issuer: process.env.APPLE_API_ISSUER };
  process.env.APPLE_API_KEY_PATH = "/tmp/key.p8"; delete process.env.APPLE_API_KEY; delete process.env.APPLE_API_ISSUER;
  try {
    assert.throws(() => notarytoolAuthArgs(), /APPLE_API_KEY_PATH, APPLE_API_KEY, and APPLE_API_ISSUER together/);
  } finally {
    if (previous.path === undefined) delete process.env.APPLE_API_KEY_PATH; else process.env.APPLE_API_KEY_PATH = previous.path;
    if (previous.id !== undefined) process.env.APPLE_API_KEY = previous.id;
    if (previous.issuer !== undefined) process.env.APPLE_API_ISSUER = previous.issuer;
  }
});

// -- reading back RightKit's own sealed release output ------------------------

function fixtureSealedRelease() {
  const dir = mkdtempSync(join(tmpdir(), "membrane-macos-sealed-"));
  const releaseId = "membrane-hub-0.1.5-aaaaaaaa";
  const sealedDir = join(dir, ".right-release", "sealed", releaseId, "mac");
  mkdirSync(sealedDir, { recursive: true });
  const dmgBytes = "fixture-dmg-bytes";
  writeFileSync(join(sealedDir, "Membrane Hub_0.1.5_aarch64.dmg"), dmgBytes);
  const sha256 = (value) => createHash("sha256").update(value).digest("hex");
  const manifest = {
    schema: 1, releaseId, app: "membrane-hub", version: "0.1.5", commit: "a".repeat(40), platform: "mac",
    files: [{ role: "installer", name: "Membrane Hub_0.1.5_aarch64.dmg", sha256: sha256(dmgBytes), sizeBytes: dmgBytes.length }],
    checkpoints: ["preflight_complete", "build_complete", "signed", "hardened", "sealed"],
  };
  writeFileSync(join(sealedDir, "release-manifest.json"), JSON.stringify(manifest));
  return { dir, sealedDir, manifest };
}

test("sealedReleaseDir names the exact .right-release/sealed path RightKit's build-release.mjs seals to", () => {
  const dir = sealedReleaseDir({ repoRoot: "/repo", app: "membrane-hub", version: "0.1.5", commit: "a".repeat(40) });
  assert.equal(dir, "/repo/.right-release/sealed/membrane-hub-0.1.5-aaaaaaaa/mac");
  assert.throws(() => sealedReleaseDir({ repoRoot: "/repo", app: "membrane-hub", version: "0.1.5", commit: "not-a-sha" }), /commit must be/);
});

test("readSealedReleaseManifest reads RightKit's real sealed manifest shape and finds the installer entry", (t) => {
  const { dir, sealedDir } = fixtureSealedRelease();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const { manifest, installer } = readSealedReleaseManifest(sealedDir);
  assert.equal(manifest.platform, "mac");
  assert.equal(installer.name, "Membrane Hub_0.1.5_aarch64.dmg");
});

test("readSealedReleaseManifest fails closed on a missing, unsealed, or wrong-platform manifest", (t) => {
  const { dir, sealedDir, manifest } = fixtureSealedRelease();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  assert.throws(() => readSealedReleaseManifest(join(dir, "missing")), /sealed release manifest not found/);
  const unsealedDir = join(dir, "unsealed");
  mkdirSync(unsealedDir, { recursive: true });
  writeFileSync(join(unsealedDir, "release-manifest.json"), JSON.stringify({ ...manifest, checkpoints: ["build_complete"] }));
  assert.throws(() => readSealedReleaseManifest(unsealedDir), /missing the sealed checkpoint/);
});

test("verifySealedInstallerHash confirms the DMG on disk matches what RightKit sealed, and fails closed on tampering", (t) => {
  const { dir, sealedDir } = fixtureSealedRelease();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const { installer } = readSealedReleaseManifest(sealedDir);
  const dmgPath = verifySealedInstallerHash({ sealedDir, installer });
  assert.ok(dmgPath.endsWith(".dmg"));
  writeFileSync(join(sealedDir, installer.name), "tampered");
  assert.throws(() => verifySealedInstallerHash({ sealedDir, installer }), /hash mismatch/);
});

// -- CLI: fail-closed argument validation only. Never spawns codesign,      --
// -- spctl, hdiutil, xcrun, notarytool, or stapler — those only run after   --
// -- every check below has already failed and exited the process.          --

test("CLI verify rejects missing --app/--dmg before touching any signing tool", () => {
  const result = spawnSync(process.execPath, [script, "verify"], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /--app and --dmg are required/);
});

test("CLI receipt rejects a nonexistent app before touching any signing tool", () => {
  const result = spawnSync(process.execPath, [script, "receipt", "--app", "x.app", "--dmg", "x.dmg"], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /missing app/);
});

test("CLI verify rejects a --sealed-dir mismatch before touching any signing tool", (t) => {
  const { dir, sealedDir } = fixtureSealedRelease();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const appPath = join(dir, "Membrane Hub.app");
  mkdirSync(appPath, { recursive: true });
  const wrongDmg = join(dir, "wrong.dmg");
  writeFileSync(wrongDmg, "not-the-sealed-bytes");
  const result = spawnSync(process.execPath, [script, "verify", "--app", appPath, "--dmg", wrongDmg, "--sealed-dir", sealedDir], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /does not match RightKit's sealed installer/);
});

test("CLI notarization-log rejects a missing/malformed --submission-id before invoking xcrun", () => {
  const result = spawnSync(process.execPath, [script, "notarization-log", "--out", "log.json"], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /--submission-id must be a notarytool UUID/);
});

test("CLI notarization-log rejects a partial API-key triplet before invoking xcrun", () => {
  const result = spawnSync(process.execPath, [script, "notarization-log", "--submission-id", "11111111-1111-1111-1111-111111111111", "--out", "log.json"], { encoding: "utf8", env: { ...process.env, APPLE_API_KEY_PATH: "/tmp/key.p8", APPLE_API_KEY: "", APPLE_API_ISSUER: "" } });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /APPLE_API_KEY_PATH, APPLE_API_KEY, and APPLE_API_ISSUER together/);
});

test("CLI platform-receipt rejects an incomplete receipt before writing any file", () => {
  const result = spawnSync(process.execPath, [script, "platform-receipt", "--out", "receipt.json", "--commit", "not-a-sha"], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /platform receipt invalid/);
});

test("CLI verify-receipt rejects a missing receipt file before touching any signing tool", () => {
  const result = spawnSync(process.execPath, [script, "verify-receipt", "--app", "x.app", "--dmg", "x.dmg", "--receipt", "does-not-exist.json"], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /--receipt must name an existing file/);
});

test("the removed bespoke sign/notarize/staple/plan commands are gone; the CLI rejects them", () => {
  for (const removed of ["sign", "notarize", "staple", "plan"]) {
    const result = spawnSync(process.execPath, [script, removed], { encoding: "utf8" });
    assert.equal(result.status, 2, `${removed} should be rejected`);
    assert.match(result.stderr, /release-macos\.mjs <verify\|receipt\|verify-receipt\|notarization-log\|platform-receipt>/);
  }
});
