import assert from "node:assert/strict";
import { mkdtempSync, rmSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildOrientationReceipt } from "../src/lib/receipt-store.mjs";
import {
  ORIENTATION_EVIDENCE_KIND,
  buildOrientationEvidence,
  candidateSetDigest,
  writeOrientationEvidenceFile,
  defaultEvidencePath,
} from "../src/lib/orientation-evidence.mjs";

test("candidateSetDigest is stable for equivalent candidate sets", () => {
  const a = {
    schemaVersion: 1,
    provider: "blueprint-static",
    freshness: { revision: "xxh128:1" },
    candidates: [
      { id: "symbol:a", sourceRef: "a.ts:1-2", sourceHash: "xxh128:aa", protected: true, exact: false },
      { id: "symbol:b", sourceRef: "b.ts:1-2", sourceHash: "xxh128:bb", protected: false, exact: true },
    ],
    omissions: [{ id: "symbol:c", reason: "over_candidate_ceiling" }],
  };
  const b = structuredClone(a);
  b.candidates = [b.candidates[1], b.candidates[0]]; // order in digest is array order — document honesty
  assert.equal(candidateSetDigest(a), candidateSetDigest(structuredClone(a)));
  assert.notEqual(candidateSetDigest(a), candidateSetDigest(b));
});

test("buildOrientationEvidence matches Forge-consumable shape", () => {
  const receipt = buildOrientationReceipt({
    receiptId: "rec-1",
    sessionId: "s",
    taskId: "t",
    repoIdentity: "demo@/tmp/demo",
    generationId: "xxh128:gen",
    manifestDigest: "sha256:manifest",
    candidateSetDigest: "sha256:candidates",
    overlayRevision: 2,
  });
  const evidence = buildOrientationEvidence(receipt);
  assert.equal(evidence.kind, ORIENTATION_EVIDENCE_KIND);
  assert.equal(evidence.locator, "blueprint://receipt/rec-1");
  assert.equal(evidence.content_hash, "sha256:candidates");
  assert.equal(evidence.invalidation_key, "sha256:manifest:2");
  assert.equal(evidence.trust_class, "host_attested");
  assert.equal(evidence.supports_or_refutes, "supports");
});

test("writeOrientationEvidenceFile emits an attest-able JSON file", () => {
  const dir = mkdtempSync(join(tmpdir(), "bp-evidence-"));
  try {
    const receipt = buildOrientationReceipt({
      receiptId: "rec-file",
      generationId: "xxh128:g",
      manifestDigest: "sha256:m",
      candidateSetDigest: "sha256:c",
    });
    const path = defaultEvidencePath(dir, receipt.receiptId);
    const { evidence } = writeOrientationEvidenceFile(receipt, path);
    assert.ok(existsSync(path));
    const onDisk = JSON.parse(readFileSync(path, "utf8"));
    assert.deepEqual(onDisk, evidence);
    assert.equal(onDisk.kind, "blueprint_orientation");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
