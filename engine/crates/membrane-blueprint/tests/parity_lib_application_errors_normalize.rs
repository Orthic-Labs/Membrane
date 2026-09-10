// Parity tests for legacy blueprint/src/lib/application/errors.mjs and
// blueprint/src/lib/application/normalize.mjs, ported natively as
// engine/crates/membrane-blueprint/src/lib_application_errors.rs and
// lib_application_normalize.rs.
//
// No legacy .test.mjs file exists for either module (confirmed by search);
// these tests exercise the concrete behaviors documented in the legacy
// source comments and code directly: the retryable/remediation lookup table
// in errors.mjs, and the clamp/trim/fallback semantics in normalize.mjs.

use membrane_blueprint::lib_application_errors::BlueprintError;
use membrane_blueprint::lib_application_normalize::{
    clamp_int, normalize_anchor_input, normalize_query_input, AnchorDefaults, ClampBounds, QueryDefaults,
};
use serde_json::json;

#[test]
fn known_retryable_code_carries_summary_and_next_operation() {
    let error = BlueprintError::new("schema_mismatch", "sealed generation mismatch", None);
    assert!(error.retryable);
    let remediation = error.remediation.expect("remediation present for known code");
    assert_eq!(remediation.summary, "The sealed Blueprint generation does not match the current schema; rebuild it.");
    assert_eq!(remediation.next_operation, "blueprint build");
}

#[test]
fn known_non_retryable_code_with_no_next_operation_has_no_remediation() {
    // request_cancelled has retryable:false and no summary/nextOperation.
    let error = BlueprintError::new("request_cancelled", "request cancelled", None);
    assert!(!error.retryable);
    assert!(error.remediation.is_none());
}

#[test]
fn unknown_code_defaults_to_non_retryable_with_no_remediation() {
    let error = BlueprintError::new("totally_unknown_code", "x", None);
    assert!(!error.retryable);
    assert!(error.remediation.is_none());
}

#[test]
fn explicit_details_remediation_is_preserved_untouched() {
    let details = json!({"remediation": {"summary": "custom", "nextOperation": "do x", "arguments": {"a": 1}}});
    let error = BlueprintError::new("root_not_enrolled", "not enrolled", Some(details));
    let remediation = error.remediation.expect("explicit remediation present");
    assert_eq!(remediation.summary, "custom");
    assert_eq!(remediation.next_operation, "do x");
}

#[test]
fn clamp_int_clamps_to_bounds_and_falls_back_on_non_finite() {
    let bounds = ClampBounds { min: 1, max: 100, fallback: 20 };
    assert_eq!(clamp_int(&json!(50), &bounds), 50);
    assert_eq!(clamp_int(&json!(500), &bounds), 100);
    assert_eq!(clamp_int(&json!(-5), &bounds), 1);
    assert_eq!(clamp_int(&json!("not a number"), &bounds), 20);
    assert_eq!(clamp_int(&json!(null), &bounds), 20);
    assert_eq!(clamp_int(&json!(12.9), &bounds), 12); // Math.trunc, not round
}

#[test]
fn normalize_query_input_requires_non_empty_query_and_trims() {
    let err = normalize_query_input(&json!({}), &QueryDefaults::default()).unwrap_err();
    assert_eq!(err.code, "query_required");

    let err2 = normalize_query_input(&json!({"query": "   "}), &QueryDefaults::default()).unwrap_err();
    assert_eq!(err2.code, "query_required");

    let ok = normalize_query_input(&json!({"query": "  find x  ", "limit": 5}), &QueryDefaults::default()).unwrap();
    assert_eq!(ok.query, "find x");
    assert_eq!(ok.limit, 5);
    assert!(ok.anchors.is_empty());
}

#[test]
fn normalize_query_input_falls_back_to_task_when_query_absent() {
    let ok = normalize_query_input(&json!({"task": "do the thing"}), &QueryDefaults::default()).unwrap();
    assert_eq!(ok.query, "do the thing");
    assert_eq!(ok.limit, 20); // default fallback
}

#[test]
fn normalize_anchor_input_requires_non_empty_anchor_with_defaults() {
    let err = normalize_anchor_input(&json!({}), &AnchorDefaults::default()).unwrap_err();
    assert_eq!(err.code, "anchor_required");

    let ok = normalize_anchor_input(&json!({"anchor": "src/a.ts", "depth": 3, "budget": 500}), &AnchorDefaults::default()).unwrap();
    assert_eq!(ok.anchor, "src/a.ts");
    assert_eq!(ok.depth, 3);
    assert_eq!(ok.budget, 500);
    assert_eq!(ok.direction, "both");
    assert!(ok.cursor.is_none());
}
