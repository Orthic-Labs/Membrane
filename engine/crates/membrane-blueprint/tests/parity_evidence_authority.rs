//! Parity tests for `src/evidence_authority.rs` against the legacy
//! `blueprint/src/graph/evidence-authority.mjs` plus its `provenance.mjs`
//! helpers, porting every case from
//! `blueprint/tests/evidence-authority.test.mjs` and
//! `blueprint/tests/evidence-authority-boundary.test.mjs`.

use membrane_blueprint::evidence_authority::{
    confidence_for_provenance, evaluate_evidence, semantic_authority_for_fact, semantic_authority_rank_for_fact,
    source_coherence_rank,
};
use serde_json::{json, Value};

fn evaluate(candidates: Vec<Value>, requested_relation: Option<&str>, target_source_state: Option<&Value>) -> Value {
    evaluate_evidence(target_source_state, &candidates, requested_relation)
}

// -----------------------------------------------------------------
// evidence-authority.test.mjs
// -----------------------------------------------------------------

#[test]
fn freshness_precedes_authority_current_structural_beats_stale_compiler() {
    let result = evaluate(
        vec![
            json!({
                "id": "compiler-stale", "kind": "CALLS", "target": "symbol:old",
                "provenance": "AUTHORITATIVE_SEMANTIC", "confidenceTier": "EXACT_RESOLUTION",
                "sourceRelation": "behind", "confidence": null,
            }),
            json!({
                "id": "structural-current", "kind": "CALLS", "target": "symbol:current",
                "provenance": "STRUCTURAL_RESOLVED", "confidenceTier": "SAME_FILE_LEXICAL",
                "sourceRelation": "equal", "confidence": null,
            }),
        ],
        Some("CALLS"),
        None,
    );
    assert_eq!(result["state"], "admitted");
    assert_eq!(result["admitted"]["id"], "structural-current");
    assert_eq!(result["vector"]["coherence"], 0);
}

#[test]
fn authority_precedes_inferential_confidence() {
    let result = evaluate(
        vec![
            json!({
                "id": "heuristic", "target": "symbol:guess", "provenance": "HEURISTIC_BRIDGE",
                "confidenceTier": "CROSS_FILE_HEURISTIC", "sourceRelation": "equal", "confidence": 0.999,
            }),
            json!({
                "id": "compiler", "target": "symbol:exact", "provenance": "AUTHORITATIVE_SEMANTIC",
                "confidenceTier": "EXACT_RESOLUTION", "sourceRelation": "equal", "confidence": null,
            }),
        ],
        None,
        None,
    );
    assert_eq!(result["state"], "admitted");
    assert_eq!(result["admitted"]["id"], "compiler");
}

#[test]
fn equal_categorical_authority_with_conflicting_targets_is_frontier() {
    let result = evaluate(
        vec![
            json!({ "id": "a", "target": "symbol:a", "provenance": "RULE_RESOLVED", "confidenceTier": "EXACT_RESOLUTION", "sourceRelation": "equal" }),
            json!({ "id": "b", "target": "symbol:b", "provenance": "RULE_RESOLVED", "confidenceTier": "EXACT_RESOLUTION", "sourceRelation": "equal" }),
        ],
        None,
        None,
    );
    assert_eq!(result["state"], "unresolved_frontier");
    assert_eq!(result["reason"], "authority_tie_conflict");
    assert_eq!(result["targets"], json!(["symbol:a", "symbol:b"]));
}

#[test]
fn inferential_confidence_is_final_tiebreak_only_within_heuristic_evidence() {
    let result = evaluate(
        vec![
            json!({ "id": "weak", "target": "symbol:weak", "provenance": "HEURISTIC_BRIDGE", "confidenceTier": "CROSS_FILE_HEURISTIC", "sourceRelation": "equal", "confidence": 0.61 }),
            json!({ "id": "strong", "target": "symbol:strong", "provenance": "HEURISTIC_BRIDGE", "confidenceTier": "CROSS_FILE_HEURISTIC", "sourceRelation": "equal", "confidence": 0.84 }),
        ],
        None,
        None,
    );
    assert_eq!(result["state"], "admitted");
    assert_eq!(result["admitted"]["id"], "strong");
}

#[test]
fn unknown_never_collapses_to_current() {
    let result = evaluate(
        vec![json!({ "id": "unknown", "target": "symbol:x", "provenance": "AUTHORITATIVE_SEMANTIC", "confidenceTier": "EXACT_RESOLUTION", "sourceRelation": "unknown" })],
        None,
        None,
    );
    assert_eq!(source_coherence_rank(&json!({ "sourceRelation": "unknown" }), None), 2);
    assert_eq!(result["state"], "unresolved_frontier");
    assert_eq!(result["reason"], "no_source_coherent_evidence");
}

#[test]
fn inadmissible_and_wrong_relation_candidates_cannot_participate() {
    let result = evaluate(
        vec![
            json!({ "id": "wrong", "kind": "IMPORTS", "target": "file:x", "provenance": "AUTHORITATIVE_SEMANTIC", "confidenceTier": "EXACT_RESOLUTION", "sourceRelation": "equal" }),
            json!({ "id": "blocked", "kind": "CALLS", "target": "symbol:x", "provenance": "AUTHORITATIVE_SEMANTIC", "confidenceTier": "EXACT_RESOLUTION", "sourceRelation": "equal", "scopeAllowed": false }),
        ],
        Some("CALLS"),
        None,
    );
    assert_eq!(result["state"], "unresolved_frontier");
    assert_eq!(result["reason"], "no_admissible_evidence");
}

#[test]
fn legacy_facts_are_mapped_categorically_without_consulting_scalar_confidence() {
    assert_eq!(
        semantic_authority_for_fact(&json!({ "provider": "scip-python", "precisionTier": "COMPILER", "confidence": 0.01 })),
        "AUTHORITATIVE_SEMANTIC"
    );
    assert_eq!(
        semantic_authority_for_fact(&json!({ "confidenceTier": "CROSS_FILE_HEURISTIC", "confidence": 1 })),
        "HEURISTIC_BRIDGE"
    );
    assert!(
        semantic_authority_rank_for_fact(&json!({ "provider": "scip-python", "precisionTier": "COMPILER", "confidence": 0.01 }))
            < semantic_authority_rank_for_fact(&json!({ "confidenceTier": "CROSS_FILE_HEURISTIC", "confidence": 1 }))
    );
}

#[test]
fn same_target_from_equivalent_evidence_is_admitted_and_retained() {
    let result = evaluate(
        vec![
            json!({ "id": "a", "target": "symbol:x", "provenance": "RULE_RESOLVED", "confidenceTier": "EXACT_RESOLUTION", "sourceRelation": "equal" }),
            json!({ "id": "b", "target": "symbol:x", "provenance": "RULE_RESOLVED", "confidenceTier": "EXACT_RESOLUTION", "sourceRelation": "equal" }),
        ],
        None,
        None,
    );
    assert_eq!(result["state"], "admitted");
    assert_eq!(result["admitted"]["target"], "symbol:x");
    assert_eq!(result["equivalentEvidence"].as_array().unwrap().len(), 2);
}

// -----------------------------------------------------------------
// evidence-authority-boundary.test.mjs
// -----------------------------------------------------------------

fn fact(extra: Value) -> Value {
    let mut base = json!({
        "id": "candidate", "kind": "REFERENCES", "target": "symbol:current", "resolved": true,
        "provenance": "STRUCTURAL_RESOLVED", "confidenceTier": "EXACT_RESOLUTION",
        "confidence": null, "sourceRelation": "current",
    });
    if let (Value::Object(base_map), Value::Object(extra_map)) = (&mut base, extra) {
        for (k, v) in extra_map {
            base_map.insert(k, v);
        }
    }
    base
}

fn evaluate_ref(candidates: Vec<Value>, target_source_state: Option<&Value>) -> Value {
    evaluate_evidence(target_source_state, &candidates, Some("REFERENCES"))
}

#[test]
fn optimistic_current_flags_cannot_override_mismatched_source_identity() {
    let stale = fact(json!({ "id": "scip", "provenance": "AUTHORITATIVE_SEMANTIC", "sourceStateId": "old", "sourceCoherent": true }));
    let current = fact(json!({ "id": "ast", "sourceStateId": "new" }));
    let target = json!("new");
    assert_eq!(source_coherence_rank(&stale, Some(&target)), 1);
    let result = evaluate_ref(vec![stale.clone(), current], Some(&target));
    assert_eq!(result["admitted"]["id"], "ast");
    let only_stale = evaluate_ref(vec![stale], Some(&target));
    assert_eq!(only_stale["reason"], "no_source_coherent_evidence");
}

#[test]
fn explicit_stale_observations_dominate_current_flags_and_matching_identity() {
    let target = json!("same");
    for relation in ["stale", "behind", "ahead", "diverged"] {
        let candidate = fact(json!({ "sourceCoherent": true, "sourceRelation": relation, "sourceStateId": "same" }));
        assert_eq!(source_coherence_rank(&candidate, Some(&target)), 1);
    }
    let candidate = fact(json!({ "sourceCoherent": false, "sourceStateId": "same" }));
    assert_eq!(source_coherence_rank(&candidate, Some(&target)), 1);
}

#[test]
fn unknown_or_object_identities_never_become_current_through_string_coercion() {
    let new_target = json!("new");
    assert_eq!(source_coherence_rank(&fact(json!({})), Some(&new_target)), 2);
    let empty_obj = json!({});
    assert_eq!(source_coherence_rank(&fact(json!({ "sourceStateId": {} })), Some(&empty_obj)), 2);
    assert_eq!(source_coherence_rank(&fact(json!({ "sourceStateId": "" })), None), 2);
    let str_42 = json!("42");
    assert_eq!(source_coherence_rank(&fact(json!({ "sourceStateId": 42 })), Some(&str_42)), 1);
    let num_42 = json!(42);
    assert_eq!(source_coherence_rank(&fact(json!({ "sourceStateId": 42 })), Some(&num_42)), 0);
}

#[test]
fn unresolved_markers_and_unknown_provenance_cannot_acquire_compiler_authority() {
    assert_eq!(semantic_authority_for_fact(&fact(json!({ "provenance": "AUTHORITATIVE_SEMANTIC", "resolved": false }))), "UNRESOLVED");
    assert_eq!(semantic_authority_for_fact(&fact(json!({ "provenance": "AUTHORITATIVE_SEMANTIC", "confidenceTier": "UNRESOLVED" }))), "UNRESOLVED");
    assert_eq!(
        semantic_authority_for_fact(&fact(json!({ "provenance": "unknown-class", "provider": "scip-python", "precisionTier": "COMPILER" }))),
        "UNRESOLVED"
    );
    assert_eq!(semantic_authority_for_fact(&json!({ "provider": "docs-about-compiler" })), "UNRESOLVED");
    let result = evaluate_ref(vec![fact(json!({ "resolved": false, "provenance": "AUTHORITATIVE_SEMANTIC" }))], None);
    assert_eq!(result["reason"], "resolution_unresolved");
}

#[test]
fn required_relationship_and_real_target_cannot_be_replaced_by_edge_id() {
    let mut missing_kind = fact(json!({}));
    missing_kind.as_object_mut().unwrap().remove("kind");
    assert_eq!(evaluate_ref(vec![missing_kind], None)["reason"], "no_admissible_evidence");

    let mut missing_target = fact(json!({}));
    missing_target.as_object_mut().unwrap().remove("target");
    assert_eq!(evaluate_ref(vec![missing_target], None)["reason"], "resolution_target_missing");

    assert_eq!(
        evaluate_ref(vec![fact(json!({ "target": null, "targetId": "symbol:other" }))], None)["reason"],
        "resolution_target_missing"
    );
}

#[test]
fn inference_confidence_rejects_coercible_nonnumbers() {
    // NaN/Infinity are not representable via serde_json::Value; those two
    // legacy cases have no Rust equivalent here.
    for value in [json!("0.9"), json!(""), json!(true), json!(false), json!([]), json!([0.8]), json!(-0.1), json!(1.1)] {
        assert!(confidence_for_provenance("HEURISTIC_BRIDGE", Some(&value)).is_err());
        let result = evaluate_ref(vec![fact(json!({ "provenance": "HEURISTIC_BRIDGE", "confidence": value }))], None);
        assert_eq!(result["reason"], "invalid_inferential_confidence");
    }
}

#[test]
fn null_inferential_confidence_remains_unknown_not_numeric_zero() {
    let inferred = fact(json!({ "provenance": "HEURISTIC_BRIDGE", "confidence": null }));
    let result = evaluate_ref(vec![inferred], None);
    assert_eq!(result["vector"]["inferentialConfidence"], Value::Null);
    assert_eq!(result["admitted"]["confidence"], Value::Null);
    assert_eq!(confidence_for_provenance("HEURISTIC_BRIDGE", Some(&json!(0))).unwrap(), Some(0.0));
}

#[test]
fn admission_normalizes_tagged_legacy_confidence_without_mutating_input() {
    let legacy = fact(json!({ "provenance": "AUTHORITATIVE_SEMANTIC", "confidence": 1 }));
    let result = evaluate_ref(vec![legacy.clone()], None);
    assert_eq!(result["admitted"]["confidence"], Value::Null);
    assert_eq!(result["equivalentEvidence"][0]["confidence"], Value::Null);
    assert_eq!(legacy["confidence"], 1);
}

#[test]
fn lsp_verification_cannot_originate_or_outrank_canonical_graph_fact() {
    let verification = fact(json!({ "id": "lsp", "provenance": "LIVE_VERIFICATION" }));
    assert_eq!(evaluate_ref(vec![verification.clone()], None)["reason"], "verification_without_canonical_evidence");
    assert!(semantic_authority_rank_for_fact(&verification) > semantic_authority_rank_for_fact(&fact(json!({}))));
    let result = evaluate_ref(vec![verification, fact(json!({}))], None);
    assert_eq!(result["admitted"]["id"], "candidate");
    assert_eq!(result["verifications"].as_array().unwrap().len(), 1);
    assert_eq!(result["verifications"][0]["provenance"], "LIVE_VERIFICATION");
}

#[test]
fn coherent_lsp_disagreement_returns_resolution_conflict_without_rewriting_truth() {
    let canonical = fact(json!({}));
    let verification = fact(json!({ "id": "lsp", "target": "symbol:other", "provenance": "LIVE_VERIFICATION" }));
    let result = evaluate_ref(vec![canonical.clone(), verification.clone()], None);
    assert_eq!(result["state"], "unresolved_frontier");
    assert_eq!(result["reason"], "resolution_conflict");
    assert_eq!(result["admitted"], Value::Null);
    assert_eq!(canonical["target"], "symbol:current");

    let mut verification_stale = verification;
    verification_stale.as_object_mut().unwrap().insert("sourceRelation".to_string(), json!("stale"));
    let stale_check = evaluate_ref(vec![canonical, verification_stale], None);
    assert_eq!(stale_check["admitted"]["target"], "symbol:current");
    assert_eq!(stale_check["verifications"].as_array().unwrap().len(), 1, "stale check remains visible but cannot rewrite truth");
}
