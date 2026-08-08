// Operations round-trip test — the TypeScript half of the MBR-301 acceptance.
//
// For every entry in OPERATIONS this proves:
//   (a) the success fixture validates against the per-operation JSON Schema;
//   (b) the error fixture validates against the same schema's error branch;
//   (c) the per-operation schema's error-code enum is a SUPERSET of the
//       closed taxonomy the index entry lists, so the contract can never
//       drift to a code the schema does not know;
//   (d) the on-disk `operations-index.v1` fixture matches OPERATIONS on
//       every observable field (name, schemaVersion, errorVersion,
//       schemaPath, fixture paths, error-code set);
//   (e) the canonical digest of the operations-index fixture equals the
//       value the Rust test pins, so any drift fails BOTH suites.
//
// Run with:  node --test engine/crates/membrane-protocol/bindings/operations.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { canonicalize, loadJson } from "./protocol.mjs";
import {
  OPERATIONS,
  digestStr,
  loadRepoJson,
  validateOperationFixtures,
} from "./operations.mjs";

// The Rust test (`tests/operations_roundtrip.rs::operations_index_canonical_digest_is_pinned`)
// pins the SAME digest. A drift in the Rust registry, the on-disk JSON, or
// the canonical rules fails BOTH suites.
const PINNED_INDEX_DIGEST =
  "sha256:4f40ab3b07cf80e6e905de1c454388c1e8074d5f0b3d1001fe82fe3f11e69567";

for (const operation of OPERATIONS) {
  test(`${operation.name}: validates, round-trips, and lists every error code in the schema`, () => {
    validateOperationFixtures(operation);
  });
}

test("OPERATIONS registry covers every required MCP tool", () => {
  const expectedNames = [
    "membrane_context",
    "membrane_source_read",
    "membrane_knowledge_propose",
    "membrane_checkpoint_save",
    "membrane_checkpoint_load",
    "membrane_working_context",
    "membrane_temporal_fact",
    "membrane_scratchpad",
    "membrane_feedback",
    "hub.capabilities",
    "hub.snapshot",
  ];
  assert.deepEqual(
    OPERATIONS.map((op) => op.name),
    expectedNames,
    "OPERATIONS must list every MCP tool in the canonical order",
  );
  // Independence: every operation's schemaVersion + errorVersion is its own.
  for (const op of OPERATIONS) {
    assert.ok(op.schemaVersion >= 1, `${op.name}: schemaVersion must be >= 1`);
    assert.ok(op.errorVersion >= 1, `${op.name}: errorVersion must be >= 1`);
    assert.ok(
      op.errorCodes.length > 0,
      `${op.name}: must declare at least one error code`,
    );
    assert.strictEqual(
      new Set(op.errorCodes).size,
      op.errorCodes.length,
      `${op.name}: error codes must be unique`,
    );
  }
});

test("on-disk operations-index fixture matches the OPERATIONS registry", () => {
  const onDisk = loadRepoJson(
    "operations/operations/operations-index.v1.golden.json",
  );
  assert.strictEqual(onDisk.operations.length, OPERATIONS.length, "operation count drifted");
  for (let i = 0; i < onDisk.operations.length; i += 1) {
    const left = onDisk.operations[i];
    const right = OPERATIONS[i];
    assert.strictEqual(left.name, right.name, "operation name drifted");
    assert.strictEqual(
      left.schemaVersion,
      right.schemaVersion,
      `${left.name}: schemaVersion drifted between index fixture and OPERATIONS`,
    );
    assert.strictEqual(
      left.errorVersion,
      right.errorVersion,
      `${left.name}: errorVersion drifted between index fixture and OPERATIONS`,
    );
    assert.strictEqual(left.schemaPath, right.schemaPath, `${left.name}: schemaPath drift`);
    assert.strictEqual(
      left.successFixture,
      right.successFixture,
      `${left.name}: successFixture drift`,
    );
    assert.strictEqual(
      left.errorFixture,
      right.errorFixture,
      `${left.name}: errorFixture drift`,
    );
    const leftSet = new Set(left.errorCodes);
    const rightSet = new Set(right.errorCodes);
    assert.deepEqual(
      [...leftSet].sort(),
      [...rightSet].sort(),
      `${left.name}: error-code set drifted between index fixture and OPERATIONS`,
    );
  }
});

test("operations-index canonical digest is pinned", () => {
  const raw = JSON.stringify(
    loadJson("operations/operations/operations-index.v1.golden.json"),
  );
  // Parse and re-canonicalize to absorb any whitespace drift in the on-disk
  // file, matching the Rust side exactly.
  const value = JSON.parse(raw);
  const digest = digestStr(canonicalize(value));
  assert.strictEqual(
    digest,
    PINNED_INDEX_DIGEST,
    "operations-index digest drifted from the Rust pin",
  );
});
