//! Golden-fixture round-trip tests — the Rust half of the MBR-101 acceptance.
//!
//! For each of the five typed shapes this proves, over the SAME fixture files
//! the TypeScript binding reads:
//!
//!   1. The golden fixture validates against its canonical JSON Schema.
//!   2. The fixture deserializes into the Rust type and re-serializes to the
//!      IDENTICAL canonical bytes (sorted keys, no whitespace) as the fixture's
//!      own canonicalization.
//!   3. The `sha256:` digest of that canonical form matches a pinned value that
//!      the TypeScript test independently reproduces from the same files.

mod common;

use membrane_protocol::{
    canonicalize, ContextCandidateSetV1, ContextPacketV1, ContextReceiptV1, KnowledgeEmissionV1,
    ScopeGrantV1, SHAPES,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

/// Canonical digest of a fixture, computed by canonicalizing the raw JSON.
fn fixture_digest(fixture_text: &str) -> String {
    let fixture: Value = serde_json::from_str(fixture_text).expect("fixture parses");
    format!("sha256:{}", sha256_hex(&canonicalize(&fixture)))
}

/// Core round-trip for one shape: schema-validate the fixture, deserialize into
/// the Rust type, re-serialize to canonical JSON, and require byte-identical
/// canonical form. Returns the canonical JSON and its digest.
fn check_shape<T>(shape_name: &str, schema_text: &str, fixture_text: &str) -> (String, String)
where
    T: DeserializeOwned + Serialize,
{
    let schema: Value = serde_json::from_str(schema_text).expect("schema parses");
    let fixture: Value = serde_json::from_str(fixture_text).expect("fixture parses");

    // (1) Schema validation.
    let violations = common::validate(&schema, &fixture);
    assert!(
        violations.is_empty(),
        "{shape_name}: fixture fails schema validation: {violations:#?}"
    );

    // (2) Deserialize into the Rust type, then re-serialize to canonical JSON.
    let typed: T = serde_json::from_str(fixture_text)
        .unwrap_or_else(|e| panic!("{shape_name}: fixture fails to deserialize: {e}"));
    let typed_value = serde_json::to_value(&typed).expect("typed value re-serializes");
    let canonical_from_typed = canonicalize(&typed_value);
    let canonical_from_fixture = canonicalize(&fixture);

    assert_eq!(
        canonical_from_typed, canonical_from_fixture,
        "{shape_name}: Rust round-trip is not byte-identical to the fixture canonical form"
    );

    // (3) Stable digest over the canonical bytes.
    let digest = format!("sha256:{}", sha256_hex(&canonical_from_typed));
    (canonical_from_typed, digest)
}

#[test]
fn scope_grant_round_trips() {
    let shape = &SHAPES[0];
    let (canonical, digest) = check_shape::<ScopeGrantV1>(shape.name, shape.schema, shape.fixture);
    assert!(!canonical.is_empty());
    assert!(digest.starts_with("sha256:"));
}

#[test]
fn context_candidate_set_round_trips() {
    let shape = &SHAPES[1];
    check_shape::<ContextCandidateSetV1>(shape.name, shape.schema, shape.fixture);
}

#[test]
fn context_packet_round_trips() {
    let shape = &SHAPES[2];
    check_shape::<ContextPacketV1>(shape.name, shape.schema, shape.fixture);
}

#[test]
fn context_receipt_round_trips() {
    let shape = &SHAPES[3];
    check_shape::<ContextReceiptV1>(shape.name, shape.schema, shape.fixture);
}

#[test]
fn knowledge_emission_round_trips() {
    let shape = &SHAPES[4];
    check_shape::<KnowledgeEmissionV1>(shape.name, shape.schema, shape.fixture);
}

/// Every shape's canonical digest, pinned in a stable order. The TypeScript
/// binding recomputes these exact digests from the same fixture files and
/// asserts equality; a drift in the Rust types, the fixtures, or the canonical
/// rules is a hard failure on BOTH sides. Update a pin only on an INTENTIONAL
/// contract change.
#[test]
fn canonical_digests_are_pinned() {
    let expected: &[(&str, &str)] = &[
        (
            "ScopeGrantV1",
            "sha256:21a54f593f194de52b30ebbde911aea78025c403becfbd62ae34ed1969bf43dd",
        ),
        (
            "ContextCandidateSetV1",
            "sha256:12d6903c2883200da371fdaf8f2f6b6ccefbf6542ca3da480fb70124a3512a8f",
        ),
        (
            "ContextPacketV1",
            "sha256:fb56e8e59a99fa364c6acbfaf327140415fd19e78520a655c9b0aeacd478350a",
        ),
        (
            "ContextReceiptV1",
            "sha256:ed0d9ac5a641bd90e87aab6e949f0a207214cd43c4e75d6d31b5d2758c853ca2",
        ),
        (
            "KnowledgeEmissionV1",
            "sha256:e3bdc7824165c35b8d71635a1db4f1eb6850919979fa113e722e65bc5f66dec9",
        ),
    ];

    for (shape, (want_name, want_digest)) in SHAPES.iter().zip(expected.iter()) {
        assert_eq!(shape.name, *want_name, "shape order drifted");
        let got = fixture_digest(shape.fixture);
        assert_eq!(
            *want_digest, got,
            "{}: canonical digest drifted — update the pin only on an INTENTIONAL contract change",
            shape.name
        );
    }
}

/// The canonical serialization is independent of input key order: a fixture
/// with scrambled keys must canonicalize to the same bytes (so Rust and JS,
/// which may iterate maps differently, still agree).
#[test]
fn canonical_form_is_key_order_independent() {
    let a: Value = serde_json::json!({ "b": 1, "a": { "y": [true, null], "x": "s" } });
    let b: Value = serde_json::json!({ "a": { "x": "s", "y": [true, null] }, "b": 1 });
    assert_eq!(canonicalize(&a), canonicalize(&b));
}
