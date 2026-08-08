import test from "node:test";
import assert from "node:assert/strict";
import { validateSealedManifest, validateExpectedPayloads, validateLifecycleReceipt, minimumExpectedPayloadCount } from "./contract.mjs";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const sealed = {
  schema: 1,
  releaseId: "membrane-hub-0.1.2-01234567",
  app: "membrane-hub",
  version: "0.1.2",
  commit: "0123456789abcdef0123456789abcdef01234567",
  platform: "win",
  files: [
    { role: "installer", name: "Membrane_0.1.2_x64-setup.exe", sha256: "a".repeat(64), sizeBytes: 100 },
  ],
  checkpoints: ["preflight_complete", "build_complete", "signed", "hardened", "sealed"],
};

const expectedPayloads = ["membrane-hub.exe", "crypt-service.exe", "membrane.exe"];

const passingReceipt = {
  schema: "windows-installer-receipt.v1",
  status: "pass",
  source_commit: sealed.commit,
  installer_sha256: "a".repeat(64),
  gates: { signature: "pass", install: "pass", update: "pass", uninstall: "pass" },
  payloads: expectedPayloads.map((name, i) => ({ name, sha256: String.fromCharCode(98 + i).repeat(64), verify: "pass" })),
};

test("validateSealedManifest requires RightKit's real sealed shape and checkpoints", () => {
  assert.deepEqual(validateSealedManifest(sealed).manifest, sealed);
  assert.throws(() => validateSealedManifest({ ...sealed, schema: "windows-release-manifest.v1" }), /schema 1/);
  assert.throws(() => validateSealedManifest({ ...sealed, checkpoints: ["sealed"] }), /missing checkpoint: signed/);
  assert.throws(() => validateSealedManifest({ ...sealed, platform: "mac" }), /platform must be win/);
  const noInstaller = { ...sealed, files: [{ role: "updater", name: "x", sha256: "a".repeat(64) }] };
  assert.throws(() => validateSealedManifest(noInstaller), /no file with role installer/);
});

test("validateExpectedPayloads rejects too-short or malformed payload lists", () => {
  assert.throws(() => validateExpectedPayloads([], 1), /at least 1 PE/);
  assert.throws(() => validateExpectedPayloads(["a.txt"], 1), /must be a \.exe or \.dll/);
  assert.throws(() => validateExpectedPayloads(["a.exe"], 3), /at least 3 PE/);
  assert.deepEqual(validateExpectedPayloads(expectedPayloads, 3), expectedPayloads);
});

test("minimumExpectedPayloadCount reads externalBin + main binary from tauri.conf.json, read-only", async () => {
  const dir = mkdtempSync(path.join(tmpdir(), "windows-contract-"));
  const configPath = path.join(dir, "tauri.conf.json");
  try {
    writeFileSync(configPath, JSON.stringify({ bundle: { externalBin: ["binaries/crypt-service", "binaries/membrane"] } }));
    assert.equal(await minimumExpectedPayloadCount(configPath), 3);
    writeFileSync(configPath, JSON.stringify({ bundle: {} }));
    assert.equal(await minimumExpectedPayloadCount(configPath), 1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("validateLifecycleReceipt passes only when the outer installer and every inner PE are proven", () => {
  assert.equal(validateLifecycleReceipt(passingReceipt, sealed, expectedPayloads), passingReceipt);

  const wrongCommit = { ...passingReceipt, source_commit: "f".repeat(40) };
  assert.throws(() => validateLifecycleReceipt(wrongCommit, sealed, expectedPayloads), /commit does not match/);

  const wrongInstallerHash = { ...passingReceipt, installer_sha256: "f".repeat(64) };
  assert.throws(() => validateLifecycleReceipt(wrongInstallerHash, sealed, expectedPayloads), /installer_sha256 does not match/);

  const missingGate = { ...passingReceipt, gates: { ...passingReceipt.gates, uninstall: "fail" } };
  assert.throws(() => validateLifecycleReceipt(missingGate, sealed, expectedPayloads), /gate not passed: uninstall/);

  const missingPayload = { ...passingReceipt, payloads: passingReceipt.payloads.slice(0, 1) };
  assert.throws(() => validateLifecycleReceipt(missingPayload, sealed, expectedPayloads), /verify every expected inner-PE payload/);

  const unverifiedPayload = { ...passingReceipt, payloads: [{ ...passingReceipt.payloads[0], verify: "fail" }, ...passingReceipt.payloads.slice(1)] };
  assert.throws(() => validateLifecycleReceipt(unverifiedPayload, sealed, expectedPayloads), /signtool verify \/pa \/tw not pass/);

  // Never touches AzureSignTool/signtool/credentials -- purely JSON validation.
  assert.equal(JSON.stringify(passingReceipt).includes("SECRET"), false);
});
