import test from "node:test";
import assert from "node:assert/strict";
import { validateReceipt, verifyPair } from "../../scripts/release/verify-platform-artifacts.mjs";

const base = { schema: "membrane.platform-acceptance.v1", receiptId: "platform-mac-1", mode: "clean-vm", commit: "a".repeat(40), releaseGeneration: "e".repeat(64), version: "v1.2.3", platform: "macos", artifact: { name: "Membrane.dmg", sha256: "b".repeat(64) }, trust: { codesign: "pass", notarization: "pass", staple: "pass", gatekeeper: "pass" }, lifecycle: { install: "pass", startup: "pass", update: "pass", uninstall: "pass" }, environment: { clean: true, machineDigest: "f".repeat(64), bypassWarnings: false } };
test("accepts identity-bound clean macOS receipt", () => assert.equal(verifyPair(base, base).status, "accepted"));
test("rejects source-ready as installed acceptance", () => assert.throws(() => verifyPair(base, { ...base, mode: "source-ready" }), /not clean-VM/));
test("rejects incomplete macOS trust", () => assert.throws(() => validateReceipt({ ...base, trust: { ...base.trust, gatekeeper: "skip" } }), /macOS trust/));
test("rejects out-of-scope non-Mac receipts", () => assert.throws(() => validateReceipt({ ...base, platform: "freebsd" }), /platform invalid/));
test("rejects contract artifact substitution", () => assert.throws(() => verifyPair(base, { ...base, artifact: { ...base.artifact, sha256: "d".repeat(64) } }), /artifact/));
test("rejects bypasses & unknown evidence", () => {
  assert.throws(() => validateReceipt({ ...base, environment: { ...base.environment, bypassWarnings: true } }), /no-bypass/);
  assert.throws(() => validateReceipt({ ...base, rawLog: "untrusted" }), /receipt invalid/);
});
