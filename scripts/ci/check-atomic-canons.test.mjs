import assert from "node:assert/strict";
import test from "node:test";

import { atomicCanonTestHooks, validateAtomicCanons } from "./check-atomic-canons.mjs";

test("normalized canon inventory is complete & generated indexes are current", () => {
  assert.deepEqual(validateAtomicCanons(), {
    canons: 7,
    capabilityRows: 261,
    atoms: 256,
    exploratory: 5,
    closed: 0,
    open: 256,
    groups: 7,
    implementations: 261,
    qualifications: 261,
    decisions: 30,
    preservationRows: 728,
    legacyAtoms: 249,
    introducedSplits: 12,
    introducedCapabilities: 0,
    specRows: 479,
    unclassified: 0,
  });
});

test("closure requires exact fresh acceptance, revision & receipt evidence", () => {
  const evidence = "Acceptance: MEM-ACC-001; Revision: 0123456789abcdef0123456789abcdef01234567; Receipt: qualification@0123456789abcdef; Freshness: 2026-08-30";
  assert.ok(atomicCanonTestHooks.proofEvidence(evidence));
  assert.equal(atomicCanonTestHooks.proofEvidence(evidence.replace("0123456789abcdef0123456789abcdef01234567", "short")), null);
  assert.equal(atomicCanonTestHooks.proofEvidence(evidence.replace("2026-08-30", "2999-01-01")), null);
  const row = { Scope: "COMMITTED", Implementation: "DELIVERED", Verification: "FOCUSED_PASS", Qualification: "PASS", Delivery: "RELEASED", Evidence: evidence };
  assert.equal(atomicCanonTestHooks.closed(row, "RELEASED"), true);
  assert.equal(atomicCanonTestHooks.closed({ ...row, Verification: "PENDING" }, "RELEASED"), false);
  assert.equal(atomicCanonTestHooks.closed({ ...row, Delivery: "COMMITTED" }, "RELEASED"), false);
});

test("semantic duplicate detector is conservative but catches aliases", () => {
  assert.equal(atomicCanonTestHooks.similarity("admit enabled provider with authority", "admit enabled provider with authority"), 1);
  assert.ok(atomicCanonTestHooks.similarity("materialize Ledger candidates", "schedule Pull providers") < 0.9);
});
