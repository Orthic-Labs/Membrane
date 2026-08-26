import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { assembleWindowsReleaseEvidence, writeWindowsReleaseEvidence } from "../scripts/write-windows-release-evidence.mjs";
import { verifyReleaseEvidence } from "../../../scripts/release/verify-release-evidence.mjs";

const sha = (value) => createHash("sha256").update(value).digest("hex");
const fixture = () => {
  const root = mkdtempSync(join(tmpdir(), "membrane-windows-evidence-"));
  const file = (name, content = name) => { writeFileSync(join(root, name), content); return name; };
  const installer = file("Membrane Hub_1.2.3_x64-setup.exe", "signed installer bytes");
  const artifactSha256 = sha(readFileSync(join(root, installer)));
  const identity = { tag: "v1.2.3", commit: "a".repeat(40), tree: "b".repeat(64), generation: "c".repeat(64), target: "windows-x86_64", vector_dispatch: "CORTEX_VECTOR_DISPATCH_V2" };
  const platform = { schema: "membrane.platform-acceptance.v1", receiptId: "windows-platform-1", mode: "installed-local", commit: identity.commit, releaseGeneration: identity.generation, version: identity.tag, platform: "windows", artifact: { name: installer, sha256: artifactSha256 }, trust: { authenticode: "pass", timestamp: "pass", publisher: "Damned_Ventures_LLC" }, lifecycle: { install: "pass", startup: "pass", update: "pass", uninstall: "pass" }, environment: { host: "windows-laptop", bypassWarnings: false } };
  const contract = { schema: platform.schema, commit: identity.commit, releaseGeneration: identity.generation, version: identity.tag, platform: "windows", artifact: platform.artifact };
  const paths = {
    installer,
    sbom: file("sbom.json", "sbom"),
    provenance: file("provenance.json", "provenance"),
    toolchain: file("toolchain.json", "toolchain"),
    compatibility: file("compatibility.json", "compatibility"),
    test: file("windows.test.json", "test"),
    edReceipt: file("ed25519.receipt", "ed25519 receipt"),
    authReceipt: file("authenticode.receipt", "auth receipt"),
    event: file("event.receipt", "event"),
    contract: file("platform.contract.json", JSON.stringify(contract)),
    platform: file("platform.receipt.json", JSON.stringify(platform)),
  };
  const input = {
    root,
    release: identity,
    ...paths,
    platformContract: paths.contract,
    platformReceipt: paths.platform,
    tests: [paths.test],
    signatures: [{ kind: "ed25519", identity: "release-ed25519", subject_sha256: artifactSha256, receipt: paths.edReceipt }],
    platformTrust: { kind: "authenticode", identity: "CN=Damned Ventures LLC", subject_sha256: artifactSha256, receipt: paths.authReceipt },
    eventHistory: { status: "sealed", receipt: paths.event },
  };
  return { root, input, artifactSha256 };
};

test("writes exact hash-bound Windows evidence & verifier accepts it", () => {
  const { root, input, artifactSha256 } = fixture();
  const result = writeWindowsReleaseEvidence({ ...input, output: "RELEASE.json" });
  assert.equal(result.path, "RELEASE.json");
  assert.equal(result.manifest.release.artifact_sha256, artifactSha256);
  assert.equal(result.manifest.release.target, "windows-x86_64");
  assert.equal(verifyReleaseEvidence(JSON.parse(readFileSync(join(root, "RELEASE.json"), "utf8")), root).verified, true);
});

test("rejects non-Windows identity, unbound signature, & empty test receipts", () => {
  const { input, artifactSha256 } = fixture();
  assert.throws(() => assembleWindowsReleaseEvidence({ ...input, release: { ...input.release, target: "macos-arm64" } }), /release.target/);
  assert.throws(() => assembleWindowsReleaseEvidence({ ...input, signatures: [{ kind: "ed25519", identity: "release-ed25519", subject_sha256: "0".repeat(64), receipt: input.signatures[0].receipt }] }), /subject_sha256/);
  assert.throws(() => assembleWindowsReleaseEvidence({ ...input, tests: [] }), /test receipts/);
  assert.ok(artifactSha256);
});
