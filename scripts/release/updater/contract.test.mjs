import assert from "node:assert/strict";
import { test } from "node:test";
import {
  FAILURE,
  UPDATER_PLATFORM_TO_RECEIPT_PLATFORM,
  assessDualSignatureUpdate,
  isUpgrade,
  validateUpdaterManifestEntry,
  versionTriple,
} from "./contract.mjs";

const ARTIFACT_SHA256 = "0".repeat(63) + "a";

function windowsManifestEntry(overrides = {}) {
  // Real shape from tools/rightkit/packages/release/publish-update.mjs:
  // body.manifest.platforms[artifact.platform] = { signature, url }.
  return {
    signature: "dW50cnVzdGVkLWxvb2tpbmctYnV0LXZhbGlkLWJhc2U2NC1zaWc=",
    url: "https://rightapps-license-gate.adrdsouza.workers.dev/membrane/updates/windows/current/Membrane_x64-setup.exe",
    ...overrides,
  };
}

function windowsPlatformReceipt(overrides = {}, trustOverrides = {}) {
  // Real shape from scripts/release/verify-platform-artifacts.mjs's
  // orthic.membrane.platform-acceptance.v1 schema.
  return {
    schema: "orthic.membrane.platform-acceptance.v1",
    receiptId: "r-1",
    mode: "clean-vm",
    commit: "0".repeat(40),
    releaseGeneration: "0".repeat(64),
    version: "0.1.6",
    platform: "windows",
    artifact: { name: "Membrane_0.1.6_x64-setup.exe", sha256: ARTIFACT_SHA256 },
    trust: { authenticode: "pass", publicTrust: "pass", rfc3161: "pass", ...trustOverrides },
    lifecycle: { install: "pass", startup: "pass", update: "pass", uninstall: "pass" },
    environment: { clean: true, machineDigest: "a".repeat(64), bypassWarnings: false },
    ...overrides,
  };
}

function baseInput(overrides = {}) {
  return {
    fromVersion: "0.1.5",
    toVersion: "0.1.6",
    artifactSha256: ARTIFACT_SHA256,
    updaterPlatform: "windows-x86_64",
    updaterManifestEntry: windowsManifestEntry(),
    platformTrustReceipt: windowsPlatformReceipt(),
    ...overrides,
  };
}

test("versionTriple parses semver and rejects short/unparseable versions", () => {
  assert.deepEqual(versionTriple("0.1.5"), [0, 1, 5]);
  assert.deepEqual(versionTriple("2.1.0-rc.1"), [2, 1, 0]);
  assert.equal(versionTriple("1.2"), null);
  assert.equal(versionTriple("latest"), null);
});

test("isUpgrade requires a strictly greater triple", () => {
  assert.equal(isUpgrade("0.1.5", "0.1.6"), true);
  assert.equal(isUpgrade("0.1.5", "0.1.5"), false);
  assert.equal(isUpgrade("0.2.0", "0.1.9"), false);
  assert.equal(isUpgrade("0.1.5", "not-a-version"), false);
});

test("validateUpdaterManifestEntry accepts RightKit's real shape and rejects a missing signature", () => {
  assert.deepEqual(validateUpdaterManifestEntry(windowsManifestEntry()), windowsManifestEntry());
  assert.throws(() => validateUpdaterManifestEntry(windowsManifestEntry({ signature: "" })));
  assert.throws(() => validateUpdaterManifestEntry(windowsManifestEntry({ url: "http://insecure" })));
});

test("both signatures valid and version advancing: verifies with no failures", () => {
  const result = assessDualSignatureUpdate(baseInput());
  assert.equal(result.outcome, "verified");
  assert.deepEqual(result.failures, []);
});

test("missing Tauri updater signature blocks", () => {
  const result = assessDualSignatureUpdate(
    baseInput({ updaterManifestEntry: windowsManifestEntry({ signature: "" }) }),
  );
  assert.equal(result.outcome, "blocked");
  assert.ok(result.failures.includes(FAILURE.TAURI_SIGNATURE_INVALID));
});

test("incomplete platform trust (one leg failed out of several) blocks", () => {
  const result = assessDualSignatureUpdate(
    baseInput({ platformTrustReceipt: windowsPlatformReceipt({}, { rfc3161: "fail" }) }),
  );
  assert.equal(result.outcome, "blocked");
  assert.ok(result.failures.includes(FAILURE.PLATFORM_TRUST_INVALID));
});

test("Tauri signature present but platform receipt is for the wrong platform blocks", () => {
  const result = assessDualSignatureUpdate(
    baseInput({ platformTrustReceipt: windowsPlatformReceipt({ platform: "macos" }) }),
  );
  assert.equal(result.outcome, "blocked");
  assert.ok(result.failures.includes(FAILURE.PLATFORM_TRUST_INVALID));
});

test("a downgrade blocks even with two fully valid signatures", () => {
  const result = assessDualSignatureUpdate(baseInput({ fromVersion: "0.2.0", toVersion: "0.1.9" }));
  assert.equal(result.outcome, "blocked");
  assert.ok(result.failures.includes(FAILURE.DOWNGRADE));
});

test("an unparseable target version is treated as a downgrade and blocks", () => {
  const result = assessDualSignatureUpdate(baseInput({ toVersion: "latest" }));
  assert.equal(result.outcome, "blocked");
  assert.ok(result.failures.includes(FAILURE.DOWNGRADE));
});

test("a mismatched artifact hash between the manifest identity and the platform receipt blocks", () => {
  const result = assessDualSignatureUpdate(
    baseInput({ platformTrustReceipt: windowsPlatformReceipt({ artifact: { name: "x", sha256: "1".repeat(64) } }) }),
  );
  assert.equal(result.outcome, "blocked");
  assert.ok(result.failures.includes(FAILURE.INVALID_IDENTITY));
});

test("an unregistered updater platform key blocks rather than being silently accepted", () => {
  const result = assessDualSignatureUpdate(baseInput({ updaterPlatform: "darwin-aarch64" }));
  assert.equal(result.outcome, "blocked");
  assert.ok(result.failures.includes(FAILURE.TAURI_SIGNATURE_INVALID));
});

test("only RightKit's currently-declared updater platform key is registered", () => {
  // apps/membrane-hub/right-release.config.mjs only declares an `updater`
  // block for targets.win today; a mac entry must be added here explicitly
  // once that config exists, never assumed ahead of it.
  assert.deepEqual(Object.keys(UPDATER_PLATFORM_TO_RECEIPT_PLATFORM), ["windows-x86_64"]);
});

console.log("MBR-911 release-time dual-signature admission contract: PASS");
