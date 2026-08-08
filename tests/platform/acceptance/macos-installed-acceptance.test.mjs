import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { verifyMacosInstalledAcceptance } from "../../../scripts/release/acceptance/macos-installed-acceptance.mjs";

// SHA-256 of the empty byte string. release-macos.mjs's internal directory
// hash (`sha256()` in that script) walks a directory's sorted entries; for
// an empty directory that walk hashes nothing but the empty relative-path
// string, which is exactly SHA256(""). This is standard SHA-256 output, not
// membrane-specific logic — used here only to build a genuine, independently
// verifiable fixture `.app` directory without reimplementing that walk.
const EMPTY_DIR_SHA256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

function fixture() {
  const dir = mkdtempSync(join(tmpdir(), "macos-installed-acceptance-"));
  const releaseId = "membrane-hub-1.2.3-aaaaaaaa";
  const sealedDir = join(dir, ".right-release", "sealed", releaseId, "mac");
  mkdirSync(sealedDir, { recursive: true });

  const dmgBytes = "fixture-dmg-bytes";
  const dmgName = "Membrane Hub_1.2.3_aarch64.dmg";
  const dmgSha256 = sha256(dmgBytes);
  writeFileSync(join(sealedDir, dmgName), dmgBytes);

  const commit = "a".repeat(40);
  const manifest = {
    schema: 1, releaseId, app: "membrane-hub", version: "1.2.3", commit, platform: "mac",
    files: [{ role: "installer", name: dmgName, sha256: dmgSha256, sizeBytes: dmgBytes.length }],
    checkpoints: ["preflight_complete", "build_complete", "signed", "hardened", "sealed"],
  };
  writeFileSync(join(sealedDir, "release-manifest.json"), JSON.stringify(manifest));

  const appPath = join(dir, "Membrane Hub.app");
  mkdirSync(appPath, { recursive: true }); // empty dir fixture -> deterministic EMPTY_DIR_SHA256
  const dmgPath = join(sealedDir, dmgName);

  const macosReceiptPath = join(dir, "macos-release-receipt.json");
  writeFileSync(macosReceiptPath, JSON.stringify({
    schema: "orthic.membrane.macos-release-receipt.v1", product: "Membrane", commit, version: "1.2.3",
    app: "Membrane Hub.app", dmg: dmgName, appSha256: EMPTY_DIR_SHA256, dmgSha256,
    checks: { codesign: "validated", notary: "validated", staple: "validated", gatekeeper: "validated" }, publish: false,
  }));

  const platformReceipt = {
    schema: "orthic.membrane.platform-acceptance.v1", receiptId: "mbr-806-mac-1", mode: "clean-vm",
    commit, releaseGeneration: "e".repeat(64), version: "1.2.3", platform: "macos",
    artifact: { name: dmgName, sha256: dmgSha256 },
    trust: { codesign: "pass", notarization: "pass", staple: "pass", gatekeeper: "pass" },
    lifecycle: { install: "pass", startup: "pass", update: "pass", uninstall: "pass" },
    environment: { clean: true, machineDigest: "f".repeat(64), bypassWarnings: false },
  };

  return { dir, sealedDir, appPath, dmgPath, macosReceiptPath, platformReceipt, manifest };
}

test("accepts a fully bound, fully signed, clean-VM macOS receipt", (t) => {
  const { dir, sealedDir, appPath, dmgPath, macosReceiptPath, platformReceipt } = fixture();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const result = verifyMacosInstalledAcceptance({ platformReceipt, sealedDir, appPath, dmgPath, macosReceiptPath });
  assert.equal(result.status, "accepted");
  assert.equal(result.platform, "macos");
});

test("FAILS CLOSED when the sealed DMG on disk was tampered with after sealing", (t) => {
  const { dir, sealedDir, appPath, dmgPath, macosReceiptPath, platformReceipt } = fixture();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(dmgPath, "tampered-bytes");
  assert.throws(
    () => verifyMacosInstalledAcceptance({ platformReceipt, sealedDir, appPath, dmgPath, macosReceiptPath }),
    /hash mismatch/,
  );
});

test("FAILS CLOSED when the platform receipt's artifact sha256 does not match RightKit's sealed installer", (t) => {
  const { dir, sealedDir, appPath, dmgPath, macosReceiptPath, platformReceipt } = fixture();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const tampered = { ...platformReceipt, artifact: { ...platformReceipt.artifact, sha256: "d".repeat(64) } };
  assert.throws(
    () => verifyMacosInstalledAcceptance({ platformReceipt: tampered, sealedDir, appPath, dmgPath, macosReceiptPath }),
    /artifact sha256 does not match/,
  );
});

test("FAILS CLOSED when the macOS release receipt's recorded hash does not match the actual .app/.dmg bytes", (t) => {
  const { dir, sealedDir, appPath, dmgPath, macosReceiptPath, platformReceipt, manifest } = fixture();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(macosReceiptPath, JSON.stringify({
    schema: "orthic.membrane.macos-release-receipt.v1", product: "Membrane", commit: manifest.commit, version: "1.2.3",
    app: "Membrane Hub.app", dmg: manifest.files[0].name, appSha256: EMPTY_DIR_SHA256, dmgSha256: "f".repeat(64),
    checks: { codesign: "validated", notary: "validated", staple: "validated", gatekeeper: "validated" }, publish: false,
  }));
  assert.throws(
    () => verifyMacosInstalledAcceptance({ platformReceipt, sealedDir, appPath, dmgPath, macosReceiptPath }),
    /macOS release receipt did not verify.*receipt hash mismatch/s,
  );
});

test("FAILS CLOSED when the macOS release receipt's checks are not all validated (e.g. Gatekeeper skipped)", (t) => {
  const { dir, sealedDir, appPath, dmgPath, macosReceiptPath, platformReceipt, manifest } = fixture();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(macosReceiptPath, JSON.stringify({
    schema: "orthic.membrane.macos-release-receipt.v1", product: "Membrane", commit: manifest.commit, version: "1.2.3",
    app: "Membrane Hub.app", dmg: manifest.files[0].name, appSha256: EMPTY_DIR_SHA256, dmgSha256: manifest.files[0].sha256,
    checks: { codesign: "validated", notary: "validated", staple: "validated", gatekeeper: "skipped" }, publish: false,
  }));
  assert.throws(
    () => verifyMacosInstalledAcceptance({ platformReceipt, sealedDir, appPath, dmgPath, macosReceiptPath }),
    /macOS release receipt did not verify/,
  );
});

test("FAILS CLOSED, never assume-pass, when the macOS release receipt file is missing", (t) => {
  const { dir, sealedDir, appPath, dmgPath, platformReceipt } = fixture();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  assert.throws(
    () => verifyMacosInstalledAcceptance({ platformReceipt, sealedDir, appPath, dmgPath, macosReceiptPath: join(dir, "does-not-exist.json") }),
    /macOS release receipt not found/,
  );
});

test("FAILS CLOSED when macOS trust (codesign/notarization/staple/gatekeeper) is incomplete", (t) => {
  const { dir, sealedDir, appPath, dmgPath, macosReceiptPath, platformReceipt } = fixture();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const tampered = { ...platformReceipt, trust: { ...platformReceipt.trust, notarization: "skip" } };
  assert.throws(
    () => verifyMacosInstalledAcceptance({ platformReceipt: tampered, sealedDir, appPath, dmgPath, macosReceiptPath }),
    /macOS trust receipt incomplete/,
  );
});

test("FAILS CLOSED when the platform receipt is source-ready, not installed", (t) => {
  const { dir, sealedDir, appPath, dmgPath, macosReceiptPath, platformReceipt } = fixture();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const tampered = { ...platformReceipt, mode: "source-ready" };
  assert.throws(
    () => verifyMacosInstalledAcceptance({ platformReceipt: tampered, sealedDir, appPath, dmgPath, macosReceiptPath }),
    /requires mode: clean-vm/,
  );
});

test("resolves relative --dmg to the same file the sealed manifest names, and rejects a different-but-existing dmg path", (t) => {
  const { dir, sealedDir, appPath, dmgPath, macosReceiptPath, platformReceipt } = fixture();
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const decoyDmg = join(dir, "decoy.dmg");
  writeFileSync(decoyDmg, "fixture-dmg-bytes"); // identical bytes/hash, different path/name
  assert.throws(
    () => verifyMacosInstalledAcceptance({ platformReceipt, sealedDir, appPath, dmgPath: decoyDmg, macosReceiptPath }),
    /does not resolve to RightKit's sealed installer path/,
  );
  // The real sealed path still verifies.
  const result = verifyMacosInstalledAcceptance({ platformReceipt, sealedDir, appPath, dmgPath: resolve(dmgPath), macosReceiptPath });
  assert.equal(result.status, "accepted");
});
