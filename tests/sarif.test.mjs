// D42: SARIF conversion, GitHub Action, and signed run ledger.

import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { toSarif } from "../lib/sarif.mjs";
import { ledgerLink, verifyLedgerLink, verifyLedgerChain } from "../lib/run-ledger.mjs";
import { runAction } from "../action/index.mjs";

test("SARIF maps findings with stable rules and severities", () => {
  const sarif = toSarif([
    { ruleId: "r1", ruleName: "R1", severity: "error", message: "violation", fingerprint: "fp1", path: "src/a.ts", startLine: 3, endLine: 5, generationId: "g1", confidenceTier: "CROSS_FILE_HEURISTIC", evidencePath: ["src/a.ts", "src/b.ts"] },
    { ruleId: "r2", severity: "warning", message: "warn", fingerprint: "fp2", path: "src/b.ts" },
  ], "0.2.0");
  assert.equal(sarif.version, "2.1.0");
  assert.equal(sarif.runs[0].tool.driver.name, "Cortex");
  assert.equal(sarif.runs[0].results.length, 2);
  assert.equal(sarif.runs[0].results[0].level, "error");
  assert.equal(sarif.runs[0].results[0].partialFingerprints.cortexFinding, "fp1");
  assert.equal(sarif.runs[0].results[0].locations[0].physicalLocation.region.startLine, 3);
  assert.equal(sarif.runs[0].results[0].properties.generationId, "g1");
  assert.equal(sarif.runs[0].tool.driver.rules[0].defaultConfiguration.level, "error");
});

test("run ledger links verify and tamper detection fails", () => {
  const key = "test-key";
  const link1 = ledgerLink({ key, version: "0.1.0", commit: "c1", generationId: "g1", providerDigests: {}, ruleDigests: {}, inputs: {}, outputs: {}, artifactHashes: {} });
  const link2 = ledgerLink({ key, previous: link1.digest, version: "0.2.0", commit: "c2", generationId: "g2", providerDigests: {}, ruleDigests: {}, inputs: {}, outputs: {}, artifactHashes: {} });
  assert.equal(verifyLedgerLink(link1, { key }).ok, true);
  assert.equal(verifyLedgerChain([link1, link2], key).ok, true);
  const tampered = { ...link2, digest: "0".repeat(64) };
  assert.equal(verifyLedgerChain([link1, tampered], key).ok, false);
  const wrongKey = ledgerLink({ key: "other", version: "0.3.0", commit: "c3", generationId: "g3", providerDigests: {}, ruleDigests: {}, inputs: {}, outputs: {}, artifactHashes: {} });
  assert.equal(verifyLedgerChain([link1, link2, wrongKey], key).ok, false);
});

test("run ledger rejects broken chains", () => {
  const key = "k";
  const link1 = ledgerLink({ key, version: "0.1.0", commit: "c1", generationId: "g1", providerDigests: {}, ruleDigests: {}, inputs: {}, outputs: {}, artifactHashes: {} });
  const link2 = ledgerLink({ key, version: "0.2.0", commit: "c2", generationId: "g2", providerDigests: {}, ruleDigests: {}, inputs: {}, outputs: {}, artifactHashes: {} });
  const result = verifyLedgerChain([link1, link2], key);
  assert.equal(result.ok, false);
  assert.equal(result.reason, "broken_chain");
});

test("Action runs read-only on forks and writes SARIF + ledger", async () => {
  const workspace = mkdtempSync(join(tmpdir(), "cortex-action-"));
  try {
    writeFileSync(join(workspace, "cortex.rules.yml"), [
      "version: 1",
      "rules:",
      "  - id: ui-must-not-import-db",
      "    from:",
      '      path: "src/ui/**"',
      "    disallow:",
      "      edgeKinds: [IMPORTS]",
      "      to:",
      '        path: "src/db/**"',
      "    severity: error",
    ].join("\n"));
    const result = await runAction({ workspace, isFork: true });
    assert.equal(result.status, "completed");
    assert.equal(result.ruleCount, 1);
    assert.ok(existsSync(join(workspace, "cortex.sarif")));
    const sarif = JSON.parse(readFileSync(join(workspace, "cortex.sarif"), "utf8"));
    assert.equal(sarif.version, "2.1.0");
    assert.ok(result.ledgerDigest);
  } finally {
    rmSync(workspace, { recursive: true, force: true });
  }
});
