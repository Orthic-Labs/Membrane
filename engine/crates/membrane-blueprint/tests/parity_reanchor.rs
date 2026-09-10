//! Parity tests ported from `blueprint/tests/portable-identity-reanchor.test.mjs`
//! for `reanchorEvidence` only (the `service.mjs:resolve` fallback this lane
//! wires up). `portableIdentity`/`reconcileRenameAliases` are out of this
//! lane's scope and not ported here.

use membrane_blueprint::reanchor::{anchor_fingerprint, reanchor_evidence};
use serde_json::json;

// "reanchoring follows exact entity then exact fingerprint then unique normalized text"
#[test]
fn reanchoring_follows_exact_entity_then_fingerprint_then_unique_text() {
    let portable_id = format!("bp:domain:sha256:{}", "a".repeat(64));
    let exact = reanchor_evidence(
        &json!({"portableId": portable_id, "text": "old"}),
        &[json!({"id": "x", "portableId": portable_id, "text": "new"})],
    );
    assert_eq!(exact["state"], "reanchored");
    assert_eq!(exact["tier"], "exact_entity");

    let fingerprint = anchor_fingerprint("keep this exact text");
    let by_fingerprint = reanchor_evidence(&json!({"fingerprint": fingerprint}), &[json!({"id": "y", "text": "keep this exact text"})]);
    assert_eq!(by_fingerprint["tier"], "exact_fingerprint");

    let by_text = reanchor_evidence(&json!({"text": "hello    world"}), &[json!({"id": "z", "text": "hello world"})]);
    assert_eq!(by_text["tier"], "unique_normalized_text");
    assert_eq!(by_text["state"], "reanchored");
}

// "reanchoring refuses ambiguity and staleness instead of choosing a fuzzy winner"
#[test]
fn reanchoring_refuses_ambiguity_and_staleness() {
    let ambiguous = reanchor_evidence(
        &json!({"text": "same  text"}),
        &[json!({"id": "a", "text": "same text"}), json!({"id": "b", "text": "same   text"})],
    );
    assert_eq!(ambiguous["state"], "ambiguous");
    assert_eq!(ambiguous["candidates"], json!(["a", "b"]));

    let stale = reanchor_evidence(&json!({"text": "gone"}), &[json!({"id": "x", "text": "different"})]);
    assert_eq!(stale["state"], "stale");
    assert_eq!(stale["reason"], "no_exact_reanchor");
}

#[test]
fn fingerprint_is_derived_from_text_when_not_supplied_directly() {
    let previous = json!({"text": "same content"});
    let candidate_matching = json!({"id": "m", "text": "same content"});
    let result = reanchor_evidence(&previous, &[candidate_matching]);
    // Exact-fingerprint tier fires before the normalized-text tier when the
    // derived fingerprints already match byte-for-byte.
    assert_eq!(result["tier"], "exact_fingerprint");
    assert_eq!(result["state"], "reanchored");
}
