import assert from "node:assert/strict";
import test from "node:test";
import { buildMatrix } from "./generate-support-matrix.mjs";
import { mkdtempSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const commit = "a".repeat(40), generation = "b".repeat(64), artifact = "c".repeat(64);
const platformReceipt = platform => ({ schema: "orthic.membrane.platform-acceptance.v1", receiptId: `${platform}-platform-1`, mode: "clean-vm", commit, releaseGeneration: generation, version: "v1.2.3", platform, artifact: { name: `${platform}.artifact`, sha256: artifact }, lifecycle: { install: "pass", startup: "pass", update: "pass", uninstall: "pass" }, trust: platform === "macos" ? { codesign: "pass", notarization: "pass", staple: "pass", gatekeeper: "pass" } : { authenticode: "pass", publicTrust: "pass", rfc3161: "pass" }, environment: { clean: true, machineDigest: "d".repeat(64), bypassWarnings: false } });
const receipt = (platform, client = "claude") => ({ schema: "orthic.membrane.client-qualification.v1", receiptId: `${platform}-${client}-1`, status: "pass", client, feature: "installed-path", evidenceDigest: "e".repeat(64), clientEvidence: { client: "pass", model: "pass", host: "pass", provider: "pass", delivery: "pass", outcome: "pass" }, platformReceipt: platformReceipt(platform) });

test("missing or stale receipts stay unavailable", () => {
  const dir = mkdtempSync(join(tmpdir(), "matrix-"));
  const stale = receipt("macos"); stale.platformReceipt.commit = "f".repeat(40); writeFileSync(join(dir, "stale.json"), JSON.stringify(stale));
  const rows = buildMatrix({ commit, clients: ["claude"], receiptDirectory: dir }).rows;
  assert.deepEqual(rows.map(row => row.tier), ["unavailable", "unavailable"]);
});

test("current receipt qualifies only its platform", () => {
  const dir = mkdtempSync(join(tmpdir(), "matrix-"));
  writeFileSync(join(dir, "macos.json"), JSON.stringify(receipt("macos")));
  const rows = buildMatrix({ commit, clients: ["claude"], receiptDirectory: dir }).rows;
  assert.equal(rows[0].tier, "qualified"); assert.equal(rows[1].tier, "unavailable");
});
test("one client receipt never qualifies a sibling client", () => {
  const dir = mkdtempSync(join(tmpdir(), "matrix-"));
  writeFileSync(join(dir, "claude.json"), JSON.stringify(receipt("macos", "claude")));
  const rows = buildMatrix({ commit, clients: ["claude", "codex"], receiptDirectory: dir }).rows;
  assert.deepEqual(rows.filter(row => row.platform === "macos").map(row => row.tier), ["qualified", "unavailable"]);
});
