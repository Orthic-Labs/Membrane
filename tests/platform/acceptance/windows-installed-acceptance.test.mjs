import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
  verifyWindowsInstalledAcceptance,
  verifyWindowsInstalledAcceptanceFromFiles,
} from "../../../scripts/release/acceptance/windows-installed-acceptance.mjs";

const root = resolve(import.meta.dirname, "../../..");
const script = resolve(root, "scripts/release/acceptance/windows-installed-acceptance.mjs");

const sealedManifest = {
  schema: 1,
  releaseId: "membrane-hub-1.2.3-aaaaaaaa",
  app: "membrane-hub",
  version: "1.2.3",
  commit: "a".repeat(40),
  platform: "win",
  files: [{ role: "installer", name: "Membrane_1.2.3_x64-setup.exe", sha256: "b".repeat(64), sizeBytes: 100 }],
  checkpoints: ["preflight_complete", "build_complete", "signed", "hardened", "sealed"],
};

const expectedPayloads = ["membrane-hub.exe", "crypt-service.exe", "membrane.exe"];

const passingLifecycleReceipt = {
  schema: "windows-installer-receipt.v1",
  status: "pass",
  source_commit: sealedManifest.commit,
  installer_sha256: sealedManifest.files[0].sha256,
  gates: { signature: "pass", install: "pass", update: "pass", uninstall: "pass" },
  payloads: expectedPayloads.map((name, i) => ({ name, sha256: String.fromCharCode(99 + i).repeat(64), verify: "pass" })),
};

const passingPlatformReceipt = {
  schema: "orthic.membrane.platform-acceptance.v1",
  receiptId: "mbr-806-win-1",
  mode: "clean-vm",
  commit: sealedManifest.commit,
  releaseGeneration: "e".repeat(64),
  version: "1.2.3",
  platform: "windows",
  artifact: { name: sealedManifest.files[0].name, sha256: sealedManifest.files[0].sha256 },
  trust: { authenticode: "pass", publicTrust: "pass", rfc3161: "pass" },
  lifecycle: { install: "pass", startup: "pass", update: "pass", uninstall: "pass" },
  environment: { clean: true, machineDigest: "f".repeat(64), bypassWarnings: false },
};

const base = () => ({
  platformReceipt: passingPlatformReceipt,
  sealedManifest,
  expectedPayloads: [...expectedPayloads],
  lifecycleReceipt: passingLifecycleReceipt,
  minimumPayloadCount: 3,
});

test("accepts a fully bound, fully signed, clean-VM Windows receipt", () => {
  const result = verifyWindowsInstalledAcceptance(base());
  assert.equal(result.status, "accepted");
  assert.equal(result.platform, "windows");
  assert.equal(result.payloadsVerified, 3);
});

// --- the confirmed defect: outer installer signed, inner payload not -------

test("FAILS CLOSED when the outer installer is signed but one inner-PE payload was never verified (the confirmed RightKit NSIS gap)", () => {
  const input = base();
  input.lifecycleReceipt = {
    ...passingLifecycleReceipt,
    payloads: [
      passingLifecycleReceipt.payloads[0],
      { ...passingLifecycleReceipt.payloads[1], verify: "fail" }, // crypt-service.exe sidecar unverified
      passingLifecycleReceipt.payloads[2],
    ],
  };
  assert.throws(() => verifyWindowsInstalledAcceptance(input), /signtool verify \/pa \/tw not pass: crypt-service\.exe/);
});

test("FAILS CLOSED when an inner-PE payload is entirely absent from the lifecycle receipt", () => {
  const input = base();
  input.lifecycleReceipt = { ...passingLifecycleReceipt, payloads: passingLifecycleReceipt.payloads.slice(0, 2) };
  assert.throws(() => verifyWindowsInstalledAcceptance(input), /verify every expected inner-PE payload/);
});

test("FAILS CLOSED when fewer inner PEs are proven than the Tauri sidecar count requires", () => {
  const input = base();
  input.expectedPayloads = ["membrane-hub.exe"];
  input.lifecycleReceipt = { ...passingLifecycleReceipt, payloads: [passingLifecycleReceipt.payloads[0]] };
  assert.throws(() => verifyWindowsInstalledAcceptance(input), /at least 3 PE/);
});

test("FAILS CLOSED, never assume-pass, when the lifecycle receipt is entirely missing", () => {
  const input = base();
  input.lifecycleReceipt = undefined;
  assert.throws(() => verifyWindowsInstalledAcceptance(input), /lifecycle receipt \(inner-PE \+ install\/update\/uninstall proof\) is required/);
});

// --- outer-receipt-level gaps ------------------------------------------------

test("FAILS CLOSED when the platform-acceptance receipt is source-ready, not installed", () => {
  const input = base();
  input.platformReceipt = { ...passingPlatformReceipt, mode: "source-ready" };
  assert.throws(() => verifyWindowsInstalledAcceptance(input), /requires mode: clean-vm/);
});

test("FAILS CLOSED when bypass warnings were shown on the clean VM", () => {
  const input = base();
  input.platformReceipt = { ...passingPlatformReceipt, environment: { ...passingPlatformReceipt.environment, bypassWarnings: true } };
  assert.throws(() => verifyWindowsInstalledAcceptance(input), /no-bypass/);
});

test("FAILS CLOSED when Windows trust (Authenticode/Public Trust/RFC3161) is incomplete", () => {
  const input = base();
  input.platformReceipt = { ...passingPlatformReceipt, trust: { ...passingPlatformReceipt.trust, rfc3161: "skip" } };
  assert.throws(() => verifyWindowsInstalledAcceptance(input), /Windows trust receipt incomplete/);
});

// --- identity binding: a valid outer receipt cannot ride on an unrelated inner proof ---

test("FAILS CLOSED when the platform receipt's artifact sha256 does not match RightKit's sealed installer", () => {
  const input = base();
  input.platformReceipt = { ...passingPlatformReceipt, artifact: { ...passingPlatformReceipt.artifact, sha256: "d".repeat(64) } };
  assert.throws(() => verifyWindowsInstalledAcceptance(input), /artifact sha256 does not match/);
});

test("FAILS CLOSED when the platform receipt's commit does not match RightKit's sealed manifest commit", () => {
  const input = base();
  input.platformReceipt = { ...passingPlatformReceipt, commit: "f".repeat(40) };
  assert.throws(() => verifyWindowsInstalledAcceptance(input), /commit does not match/);
});

// --- fixture/file-based CLI path --------------------------------------------

function writeFixtures(dir) {
  const paths = {
    platformReceipt: join(dir, "platform-receipt.json"),
    sealedManifest: join(dir, "release-manifest.json"),
    expectedPayloads: join(dir, "expected-payloads.json"),
    lifecycleReceipt: join(dir, "lifecycle-receipt.json"),
  };
  writeFileSync(paths.platformReceipt, JSON.stringify(passingPlatformReceipt));
  writeFileSync(paths.sealedManifest, JSON.stringify(sealedManifest));
  writeFileSync(paths.expectedPayloads, JSON.stringify(expectedPayloads));
  writeFileSync(paths.lifecycleReceipt, JSON.stringify(passingLifecycleReceipt));
  return paths;
}

test("verifyWindowsInstalledAcceptanceFromFiles reads all four documents and accepts a consistent set", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "windows-installed-acceptance-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const paths = writeFixtures(dir);
  const result = await verifyWindowsInstalledAcceptanceFromFiles({
    platformReceiptPath: paths.platformReceipt,
    sealedManifestPath: paths.sealedManifest,
    expectedPayloadsPath: paths.expectedPayloads,
    lifecycleReceiptPath: paths.lifecycleReceipt,
  });
  assert.equal(result.status, "accepted");
});

test("CLI rejects missing arguments before validating anything", () => {
  const result = spawnSync(process.execPath, [script], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /usage: node windows-installed-acceptance\.mjs/);
});

test("CLI exits non-zero and FAILS CLOSED on the outer-signed/inner-unsigned defect via real files", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "windows-installed-acceptance-cli-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const paths = writeFixtures(dir);
  writeFileSync(paths.lifecycleReceipt, JSON.stringify({
    ...passingLifecycleReceipt,
    payloads: passingLifecycleReceipt.payloads.map((p) => (p.name === "membrane.exe" ? { ...p, verify: "fail" } : p)),
  }));
  const result = spawnSync(process.execPath, [
    script, paths.platformReceipt, paths.sealedManifest, paths.expectedPayloads, paths.lifecycleReceipt,
  ], { encoding: "utf8" });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /signtool verify \/pa \/tw not pass: membrane\.exe/);
});

test("CLI exits 0 and prints PASS for a fully consistent, fully signed set", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "windows-installed-acceptance-cli-pass-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const paths = writeFixtures(dir);
  const result = spawnSync(process.execPath, [
    script, paths.platformReceipt, paths.sealedManifest, paths.expectedPayloads, paths.lifecycleReceipt,
  ], { encoding: "utf8" });
  assert.equal(result.status, 0);
  assert.match(result.stdout, /PASS windows installed-path acceptance/);
});
