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
  ScopeGrantV1: "sha256:81e111edd4aac62e02bb8651d3c867cfb465d4bf58f78b69e27b6228f6f4a9c1",
  ContextCandidateSetV1: "sha256:efd6e981bbb8d19e480dc14eb39c66e533004f9b057939b0be484d3065e829eb",
  ContextPacketV1: "sha256:e56b76f42c821e8c039b2a7d21825eb86f347b4b1ee81dbf126fa7b2ea005d0c",
  ContextReceiptV1: "sha256:d4295ac3369d1a01b88f95d4ea017a106e23b84a416b5f6fecc8e7821a8507a8",
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
