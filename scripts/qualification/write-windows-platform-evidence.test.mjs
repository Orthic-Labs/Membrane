import assert from "node:assert/strict";
import test from "node:test";
import { verifyPair } from "../release/verify-platform-artifacts.mjs";
import { buildEvidence } from "./write-windows-platform-evidence.mjs";

const hash = "a".repeat(64);
const qualification = {
  schema: "membrane.windows-installed-qualification.v1",
  platform: "windows-x86_64",
  profile: "installed-local",
  artifact: {
    path: "C:\\release\\Membrane Hub_0.1.12_x64-setup.exe",
    sha256: hash,
    authenticode: "Valid",
    signerThumbprint: "ABCDEF012345",
    timestampSubject: "CN=Trusted Timestamp",
  },
  lifecycle: { install: "pass", startup: "pass", upgrade: "pass", uninstall: "pass" },
};
const release = { version: "v0.1.12", commit: "b".repeat(40), generation: "c".repeat(64) };

test("emits verifier-exact Windows contract & installed receipt", () => {
  const { contract, receipt } = buildEvidence(qualification, release);
  assert.deepEqual(verifyPair(contract, receipt), { status: "accepted", commit: release.commit, platform: "windows", artifactSha256: hash });
  assert.equal(receipt.lifecycle.update, "pass");
  assert.equal(receipt.trust.publisher, "authenticode:ABCDEF012345");
});

test("rejects invalid trust, artifact, identity, & lifecycle", () => {
  assert.throws(() => buildEvidence({ ...qualification, artifact: { ...qualification.artifact, authenticode: "NotSigned" } }, release), /Authenticode/);
  assert.throws(() => buildEvidence({ ...qualification, artifact: { ...qualification.artifact, sha256: "x" } }, release), /SHA-256/);
  assert.throws(() => buildEvidence(qualification, { ...release, commit: "x" }), /commit/);
  assert.throws(() => buildEvidence({ ...qualification, lifecycle: { ...qualification.lifecycle, upgrade: "fail" } }, release), /upgrade/);
});
