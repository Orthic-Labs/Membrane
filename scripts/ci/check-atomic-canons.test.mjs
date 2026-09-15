import assert from "node:assert/strict";
import test from "node:test";

import { atomicCanonTestHooks, validateAtomicCanons } from "./check-atomic-canons.mjs";

test("normalized canon inventory is complete & generated indexes are current", () => {
  assert.deepEqual(validateAtomicCanons(), {
    canons: 6,
    capabilityRows: 352,
    atoms: 339,
    exploratory: 13,
    competitiveClosed: 55,
    competitiveOpen: 284,
    lifecycleClosed: 0,
    lifecycleOpen: 339,
    groups: 6,
    implementations: 353,
    qualifications: 352,
    decisions: 93,
    preservationRows: 728,
    legacyAtoms: 249,
    introducedSplits: 30,
    introducedCapabilities: 106,
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

test("retired Push IDs resolve to live Pull/Membrane owners with inherited acceptance", () => {
  const pull = atomicCanonTestHooks.parseCanon({ owner: "Pull", file: "pull.md", prefix: "PUL", boundary: "RELEASED" });
  const membrane = atomicCanonTestHooks.parseCanon({ owner: "Membrane", file: "membrane.md", prefix: "MEM", boundary: "RELEASED" });
  const capabilityIds = new Set([...pull.capabilities, ...membrane.capabilities].map((row) => row.ID));
  const aliases = atomicCanonTestHooks.validateHistoricalAliases(capabilityIds, [...pull.qualifications, ...membrane.qualifications]);
  assert.equal(aliases.size, 29);
  assert.equal([...aliases.values()].filter((id) => id.startsWith("PUL-")).length, 24);
  assert.equal([...aliases.values()].filter((id) => id.startsWith("MEM-")).length, 5);
  assert.equal(aliases.get("PSH-010"), "PUL-027");
  assert.equal(aliases.get("PSH-001"), "MEM-070");
});

test("accepted decision ownership guards reject plausible regressions", () => {
  const configs = [
    ["Membrane", "membrane.md", "MEM"], ["Pull", "pull.md", "PUL"], ["Cortex", "cortex.md", "CTX"],
    ["Blueprint", "blueprint.md", "BPT"], ["Ledger", "ledger.md", "LDG"], ["Adapt", "adapt.md", "ADP"],
  ];
  const parsed = configs.map(([owner, file, prefix]) => atomicCanonTestHooks.parseCanon({ owner, file, prefix, boundary: "RELEASED" }));
  const mutate = (id, behavior) => parsed.map((canon) => ({ ...canon, capabilities: canon.capabilities.map((row) => row.ID === id ? { ...row, "Observable behavior": behavior } : row) }));
  const cases = [
    ["MEM-055", "Restart through tray only", /MEM-055/],
    ["MEM-068", "Push stores memory directly", /MEM-068/],
    ["CTX-043", "Derived embedding replaces original body", /CTX-043/],
    ["BPT-021", "Always rebuild graph on every change", /BPT-021/],
    ["BPT-056", "Treat incompatible graph as corruption", /BPT-056/],
    ["BPT-072", "Read refreshes and mutates graph", /BPT-072/],
    ["ADP-029", "Write direct provider result to canonical store", /ADP-029/],
    ["ADP-035", "Write direct provider result to canonical store", /ADP-035/],
  ];
  assert.doesNotThrow(() => atomicCanonTestHooks.validateSemanticOwnership(parsed));
  for (const [id, behavior, message] of cases) assert.throws(() => atomicCanonTestHooks.validateSemanticOwnership(mutate(id, behavior)), message);
  const current = (id) => parsed.flatMap((canon) => canon.capabilities).find((row) => row.ID === id)["Observable behavior"];
  for (const [id, before, after] of [
    ["MEM-055", "only while a valid owner remains", "even after every owner is lost"],
    ["MEM-068", "reject non-memory destinations", "accept Blueprint write destinations"],
    ["CTX-043", "never replace original body", "replace original body after embedding"],
    ["MEM-026", "five active subsystems", "six active subsystems"],
  ]) {
    assert.ok(current(id).includes(before), id);
    assert.throws(() => atomicCanonTestHooks.validateSemanticOwnership(mutate(id, current(id).replace(before, after))), new RegExp(id));
  }
});

test("focused proof requires a live assertion instead of placeholder prose", () => {
  assert.equal(atomicCanonTestHooks.focusedProofLooksExact("rightkit cargo test --manifest-path engine/Cargo.toml -p membrane-runtime --lib", "`serve::tests::expand_anchor_recovers_exact_content_and_rejects_missing`"), true);
  assert.equal(atomicCanonTestHooks.focusedProofLooksExact("cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast", "`serve::tests::expand_anchor_recovers_exact_content_and_rejects_missing`", "GitHub Actions managed CI run 123; 0 fail"), true);
  assert.equal(atomicCanonTestHooks.focusedProofLooksExact("cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast", "`serve::tests::expand_anchor_recovers_exact_content_and_rejects_missing`"), false);
  assert.equal(atomicCanonTestHooks.focusedProofLooksExact("rightkit cargo test --manifest-path engine/Cargo.toml -p membrane-runtime --lib", "TBD"), false);
  assert.equal(atomicCanonTestHooks.focusedProofLooksExact("node --test tests/example.test.mjs", "`TBD`"), false);
  assert.equal(atomicCanonTestHooks.focusedProofLooksExact("node --test tests/example.test.mjs", "focused suites passed"), false);
});


test("Cortex governed-lifecycle additions preserve status boundaries", () => {
  const canon = atomicCanonTestHooks.parseCanon({ owner: "Cortex", file: "cortex.md", prefix: "CTX", boundary: "RELEASED" });
  const byId = new Map(canon.capabilities.map((row) => [row.ID, row]));
  assert.equal(canon.capabilities.length, 43);
  assert.equal(canon.capabilities.filter((row) => row.Scope === "COMMITTED").length, 40);
  assert.deepEqual(canon.capabilities.filter((row) => row.Scope === "EXPLORATORY").map((row) => row.ID), ["CTX-033", "CTX-039", "CTX-042"]);
  for (const id of ["CTX-035", "CTX-040", "CTX-041"]) {
    assert.equal(byId.get(id).Implementation, "DELIVERED");
    assert.equal(byId.get(id).Verification, "FOCUSED_PASS");
    assert.equal(byId.get(id).Qualification, "PENDING");
    assert.equal(byId.get(id).Delivery, "PUSHED");
  }
  assert.equal(byId.get("CTX-042").Implementation, "MISSING");
  assert.equal(byId.get("CTX-042").Scope, "EXPLORATORY");
  assert.equal(byId.get("CTX-039").Scope, "EXPLORATORY");
  assert.equal(byId.get("CTX-021").Implementation, "DELIVERED");
  assert.equal(byId.get("CTX-021").Verification, "FOCUSED_PASS");
  assert.equal(byId.get("CTX-019").Implementation, "DELIVERED");
  assert.equal(byId.get("CTX-019").Verification, "FOCUSED_PASS");
});

// Donor intake refines acceptance without inventing delivery or capability rows.
test("Ripwire intake preserves one qualification per capability and no donor promotion", () => {
  const cases = [["Blueprint", "blueprint.md", "BPT", 70, 4], ["Ledger", "ledger.md", "LDG", 31, 2], ["Pull", "pull.md", "PUL", 59, 2], ["Adapt", "adapt.md", "ADP", 75, 1], ["Cortex", "cortex.md", "CTX", 43, 1], ["Membrane", "membrane.md", "MEM", 74, 2]];
  for (const [owner, file, prefix, count, decisionCount] of cases) {
    const canon = atomicCanonTestHooks.parseCanon({ owner, file, prefix, boundary: "RELEASED" });
    assert.equal(canon.capabilities.length, count, file);
    assert.equal(canon.qualifications.length, count, file);
    const refined = canon.qualifications.filter((row) => row["Acceptance boundary"].includes("2026-09-07 Ripwire intake"));
    assert.equal(refined.length > 0, owner !== "Cortex", file);
    for (const row of refined) {
      assert.ok(["PENDING", "STALE"].includes(row.State));
      assert.equal(row.Evidence, "PENDING");
    }
    for (const row of canon.decisions.slice(-decisionCount)) {
      assert.match(row["Authority/evidence"], /2026-09-07-ripwire-intake\/README\.md@[a-f0-9]{40}$/);
      if (row.Kind === "BACKLOG") assert.equal(row.State, "HOLD");
    }
  }
});
