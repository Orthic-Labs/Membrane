// Golden-fixture round-trip test — the TypeScript half of the MBR-101 acceptance.
//
// For each of the five typed shapes, over the SAME fixture and schema files the
// Rust crate reads, this proves:
//   (a) the fixture validates against its canonical JSON Schema;
//   (b) the fixture parses + re-emits to the IDENTICAL canonical bytes
//       (a parse -> canonicalize -> parse -> canonicalize round-trip is stable);
//   (c) the `sha256:` digest of that canonical form equals the value the Rust
//       test (`tests/roundtrip.rs::canonical_digests_are_pinned`) pins.
//
// Run with:  node --test engine/crates/membrane-protocol/bindings/roundtrip.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { canonicalize, canonicalDigest, loadJson, validate, SHAPES } from "./protocol.mjs";

// The exact digests the Rust test pins. Both sides compute these from the same
// fixture bytes; a drift in the Rust types, the fixtures, or the canonical rules
// fails BOTH suites.
const PINNED_DIGESTS = {
  ScopeGrantV1: "sha256:21a54f593f194de52b30ebbde911aea78025c403becfbd62ae34ed1969bf43dd",
  ContextCandidateSetV1: "sha256:12d6903c2883200da371fdaf8f2f6b6ccefbf6542ca3da480fb70124a3512a8f",
  ContextPacketV1: "sha256:fb56e8e59a99fa364c6acbfaf327140415fd19e78520a655c9b0aeacd478350a",
  ContextReceiptV1: "sha256:ed0d9ac5a641bd90e87aab6e949f0a207214cd43c4e75d6d31b5d2758c853ca2",
  KnowledgeEmissionV1: "sha256:e3bdc7824165c35b8d71635a1db4f1eb6850919979fa113e722e65bc5f66dec9",
};

for (const shape of SHAPES) {
  test(`${shape.name}: validates, round-trips, and matches the Rust-pinned digest`, () => {
    const schema = loadJson(shape.schema);
    const fixture = loadJson(shape.fixture);

    // (a) Schema validation against the same schema file the Rust test embeds.
    const violations = validate(schema, fixture);
    assert.deepEqual(violations, [], `${shape.name} fails schema validation`);

    // (b) Canonical serialization is stable across a parse -> emit -> parse -> emit
    //     round-trip (the JS re-emission is byte-identical to itself).
    const canonical1 = canonicalize(fixture);
    const canonical2 = canonicalize(JSON.parse(canonical1));
    assert.equal(canonical2, canonical1, `${shape.name} canonical form is not stable`);

    // (c) The canonical digest equals the Rust-pinned value.
    const digest = canonicalDigest(fixture);
    assert.equal(
      digest,
      PINNED_DIGESTS[shape.name],
      `${shape.name} digest drifted from the Rust pin — update both sides only on an INTENTIONAL contract change`,
    );
  });
}

// MBR-608 audit finding: `ContextPacketV1.reconciliation` is optional and
// omitted when absent, but a packet that DOES carry one must still validate
// against the canonical schema. This sibling fixture (the base
// ContextPacketV1 golden fixture omits `reconciliation` entirely) exercises
// the present-field path so the schema's `#/$defs/reconciliation` shape can
// never silently drift from `BudgetReconciliationV1` again. The digest is
// pinned the same way the Rust test
// (`tests/roundtrip.rs::context_packet_with_reconciliation_round_trips`)
// pins it; update both sides only on an INTENTIONAL contract change.
test("ContextPacketV1 (with reconciliation): validates, round-trips, and matches the Rust-pinned digest", () => {
  const schema = loadJson("schemas/context-packet.v1.schema.json");
  const fixture = loadJson("schemas/registry/context-packet.v1.with-reconciliation.golden.json");

  const violations = validate(schema, fixture);
  assert.deepEqual(violations, [], "ContextPacketV1 (with reconciliation) fails schema validation");

  const canonical1 = canonicalize(fixture);
  const canonical2 = canonicalize(JSON.parse(canonical1));
  assert.equal(canonical2, canonical1, "ContextPacketV1 (with reconciliation) canonical form is not stable");

  const digest = canonicalDigest(fixture);
  assert.equal(
    digest,
    "sha256:9a71fcef72dd0c4436739bd0ca5d048bdb4aeffa369f4ac5b79ca72966a864d0",
    "ContextPacketV1 (with reconciliation) digest drifted from the Rust pin — update both sides only on an INTENTIONAL contract change",
  );
});

// MBR-608 audit finding: `BlockV1.lane` (`Option<BudgetLaneKind>`) and
// `BlockV1.deliveryMode` (`Option<DeliveryMode>`) are optional and omitted
// when absent, but a block that DOES carry them must still validate against
// the canonical schema. This sibling fixture carries both fields on every
// block, exercising the present-field path so the schema's `#/$defs/block`
// `lane`/`deliveryMode` properties can never silently drift from `BlockV1`
// again. Kept separate from the `with-reconciliation` fixture: that one
// exercises the receipt-level `reconciliation` shape, this one exercises
// block-level fields — each fixture stays a single concern. The digest is
// pinned the same way the Rust test
// (`tests/roundtrip.rs::context_packet_with_block_lane_round_trips`) pins it;
// update both sides only on an INTENTIONAL contract change.
test("ContextPacketV1 (with block lane/deliveryMode): validates, round-trips, and matches the Rust-pinned digest", () => {
  const schema = loadJson("schemas/context-packet.v1.schema.json");
  const fixture = loadJson("schemas/registry/context-packet.v1.with-block-lane.golden.json");

  const violations = validate(schema, fixture);
  assert.deepEqual(violations, [], "ContextPacketV1 (with block lane/deliveryMode) fails schema validation");

  const canonical1 = canonicalize(fixture);
  const canonical2 = canonicalize(JSON.parse(canonical1));
  assert.equal(canonical2, canonical1, "ContextPacketV1 (with block lane/deliveryMode) canonical form is not stable");

  const digest = canonicalDigest(fixture);
  assert.equal(
    digest,
    "sha256:733abc9554b9f64e6d7427479c458beb266420e4a604ff53279844ad6b0a8bb9",
    "ContextPacketV1 (with block lane/deliveryMode) digest drifted from the Rust pin — update both sides only on an INTENTIONAL contract change",
  );
});

test("canonical form is independent of input key order", () => {
  const a = { b: 1, a: { y: [true, null], x: "s" } };
  const b = { a: { x: "s", y: [true, null] }, b: 1 };
  assert.equal(canonicalize(a), canonicalize(b));
});

test("canonical serialization matches the Rust rules byte-for-byte", () => {
  // Sorted keys, no whitespace, null preserved, nested arrays/objects.
  assert.equal(
    canonicalize({ z: [3, 1, 2], a: null, m: { b: true, a: "x" } }),
    '{"a":null,"m":{"a":"x","b":true},"z":[3,1,2]}',
  );
});
