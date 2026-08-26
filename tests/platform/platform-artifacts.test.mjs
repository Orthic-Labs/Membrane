import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { validateReceipt, verifyPair } from "../../scripts/release/verify-platform-artifacts.mjs";

const base = { schema: "membrane.platform-acceptance.v1", receiptId: "platform-windows-1", mode: "installed-local", commit: "a".repeat(40), releaseGeneration: "e".repeat(64), version: "v1.2.3", platform: "windows", artifact: { name: "Membrane Hub_1.2.3_x64-setup.exe", sha256: "b".repeat(64) }, trust: { authenticode: "pass", timestamp: "pass", publisher: "Adrian_Dsouza" }, lifecycle: { install: "pass", startup: "pass", update: "pass", uninstall: "pass" }, environment: { host: "windows-laptop", bypassWarnings: false } };
test("generated schema matches Windows-only verifier authority", () => {
  const schema = JSON.parse(readFileSync(new URL("../../dist/packaging/contracts/platform-acceptance.v1.json", import.meta.url), "utf8"));
  assert.equal(schema.properties.platform.const, "windows");
  assert.deepEqual(Object.keys(schema.$defs).sort(), ["digest", "windowsTrust"]);
  assert.deepEqual(schema.$defs.windowsTrust.required, ["authenticode", "timestamp", "publisher"]);
  assert.equal(schema.properties.mode.const, "installed-local");
  assert.equal(schema.properties.environment.properties.host.const, "windows-laptop");
  assert.equal(schema.properties.environment.properties.bypassWarnings.const, false);
});
test("accepts identity-bound installed Windows receipt", () => assert.equal(verifyPair(base, base).status, "accepted"));
test("rejects non-installed receipt modes", () => assert.throws(() => verifyPair(base, { ...base, mode: "source-ready" }), /mode invalid/));
test("rejects incomplete Windows trust", () => assert.throws(() => validateReceipt({ ...base, trust: { ...base.trust, timestamp: "skip" } }), /Windows trust/));
test("rejects out-of-scope non-Windows receipts", () => assert.throws(() => validateReceipt({ ...base, platform: "freebsd" }), /platform invalid/));
test("rejects contract artifact substitution", () => assert.throws(() => verifyPair(base, { ...base, artifact: { ...base.artifact, sha256: "d".repeat(64) } }), /artifact/));
test("rejects bypasses & unknown evidence", () => {
  assert.throws(() => validateReceipt({ ...base, environment: { ...base.environment, bypassWarnings: true } }), /no-bypass/);
  assert.throws(() => validateReceipt({ ...base, rawLog: "untrusted" }), /receipt invalid/);
});
