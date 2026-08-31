import assert from "node:assert/strict";
import test from "node:test";

import { atomicCanonTestHooks, validateAtomicCanons } from "./check-atomic-canons.mjs";

test("normalized canon inventory is complete & generated indexes are current", () => {
  assert.deepEqual(validateAtomicCanons(), {
    canons: 7,
    capabilityRows: 335,
    atoms: 324,
    exploratory: 11,
    competitiveClosed: 66,
    competitiveOpen: 258,
    lifecycleClosed: 0,
    lifecycleOpen: 324,
    groups: 7,
    implementations: 336,
    qualifications: 335,
    decisions: 59,
    preservationRows: 728,
    legacyAtoms: 249,
    introducedSplits: 30,
    introducedCapabilities: 60,
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

test("competitive closure stays separate from lifecycle qualification", () => {
  const comparison = "Receipt: docs/provenance/foundation/2026-08-31-competitive-comparison/membrane.md@0123456789abcdef0123456789abcdef01234567; Atom: MEM-001; Compared: 0123456789abcdef0123456789abcdef01234567";
  assert.deepEqual(atomicCanonTestHooks.comparisonEvidence(comparison), {
    relative: "docs/provenance/foundation/2026-08-31-competitive-comparison/membrane.md",
    hash: "0123456789abcdef0123456789abcdef01234567",
    atom: "MEM-001",
    compared: "0123456789abcdef0123456789abcdef01234567",
  });
  const row = { Scope: "COMMITTED", Competitive: "CURRENT_BEST", Implementation: "DELIVERED", Verification: "FOCUSED_PASS", Qualification: "PENDING", Delivery: "PUSHED", Evidence: "PENDING" };
  assert.equal(atomicCanonTestHooks.competitivelyClosed(row), true);
  assert.equal(atomicCanonTestHooks.closed(row, "RELEASED"), false);
});

test("semantic duplicate detector is conservative but catches aliases", () => {
  assert.equal(atomicCanonTestHooks.similarity("admit enabled provider with authority", "admit enabled provider with authority"), 1);
  assert.ok(atomicCanonTestHooks.similarity("materialize Ledger candidates", "schedule Pull providers") < 0.9);
});

test("focused proof requires a live assertion instead of placeholder prose", () => {
  assert.equal(atomicCanonTestHooks.focusedProofLooksExact("rightkit cargo test --manifest-path engine/Cargo.toml -p membrane-runtime --lib", "`serve::tests::expand_anchor_recovers_exact_content_and_rejects_missing`"), true);
  assert.equal(atomicCanonTestHooks.focusedProofLooksExact("rightkit cargo test --manifest-path engine/Cargo.toml -p membrane-runtime --lib", "TBD"), false);
  assert.equal(atomicCanonTestHooks.focusedProofLooksExact("node --test tests/example.test.mjs", "`TBD`"), false);
  assert.equal(atomicCanonTestHooks.focusedProofLooksExact("node --test tests/example.test.mjs", "focused suites passed"), false);
});
