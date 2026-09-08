//! Public-owner integration coverage for installed Push contract preparation.
//!
//! These tests intentionally use only public runtime owners. They are focused
//! evidence for PSH-002/005/025/028/029, not installed-host qualification.

use membrane_runtime::push::{delivery, egress, recovery};
use membrane_runtime::push::delivery::{ContentKind, PrepareRequest};
use membrane_runtime::push::recovery::{RecoveryError, RecoveryScope, RecoveryStore, Selector};
use membrane_protocol::host_observation::{
    EstimatorBasisV1, HostObservationProvenanceV1, ObservedFieldV1,
    RemainingContextCeilingV1, TokenEstimateV1, REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
};
use serde_json::json;

fn scope(root: &std::path::Path, session: &str) -> RecoveryScope {
    RecoveryScope::new(root, session).expect("valid recovery scope")
}

fn request(text: String, token: Option<String>, max_bytes: usize) -> PrepareRequest {
    PrepareRequest {
        text,
        kind: ContentKind::Log,
        source_path: None,
        max_bytes,
        resolver_token: token,
        exact: false,
        optimize: true,
        protected_spans: Vec::new(),
    }
}

fn ceiling(tokens: u64) -> RemainingContextCeilingV1 {
    RemainingContextCeilingV1 {
        schema_version: REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
        ceiling_id: "ceiling-push-test".into(),
        session_id: "session-push-test".into(),
        task_id: ObservedFieldV1::complete("task-push-test".into()),
        requested_at_unix_ms: 1,
        remaining_tokens: TokenEstimateV1::complete(EstimatorBasisV1::new("o200k_base", "1"), tokens),
        provenance_receipt: HostObservationProvenanceV1::new(
            "receipt-push-test", "fixture", 1,
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    }
}

#[test]
fn recovery_and_consumer_proof_bind_task_session_scope_and_store() {
    let temp = tempfile::tempdir().unwrap();
    let store = RecoveryStore::at(temp.path());
    let first = scope(temp.path(), "session-a");
    let second = scope(temp.path(), "session-b");
    let token = delivery::resolver_probe(&store, &first).unwrap()["resolverToken"]
        .as_str().unwrap().to_owned();
    let body = "same event\n".repeat(1_000);
    let prepared = delivery::prepare(&store, &first, request(body.clone(), Some(token.clone()), 2_500)).unwrap();
    let reference = prepared.recovery.expect("lossy delivery must retain original");
    assert_eq!(store.resolve(&first, &reference.handle, &Selector::Whole, 20_000, 1).unwrap().bytes().unwrap(), body.as_bytes());
    assert!(delivery::prepare(&store, &second, request(body, Some(token), 2_500)).is_err(), "proof cannot cross session scope");

    let other_store = RecoveryStore::at(temp.path().join("other-store"));
    assert_ne!(store.identity().unwrap(), other_store.identity().unwrap());
    assert!(matches!(other_store.resolve(&first, &reference.handle, &Selector::Whole, 20_000, 1), Err(RecoveryError::NotFound)));
}

#[test]
fn request_capacity_binds_task_and_session_identity_before_selection() {
    let body = json!({"remainingContextCeiling": serde_json::to_value(ceiling(10_000)).unwrap()});
    assert!(membrane_runtime::push::selection::parse_request_time_h8(
        &body, "session-push-test", "task-push-test").is_ok());
    assert!(matches!(
        membrane_runtime::push::selection::parse_request_time_h8(&body, "other-session", "task-push-test"),
        Err(membrane_runtime::push::selection::RequestTimeH8Error::IdentityMismatch { field: "sessionId", .. })
    ));
    assert!(matches!(
        membrane_runtime::push::selection::parse_request_time_h8(&body, "session-push-test", "other-task"),
        Err(membrane_runtime::push::selection::RequestTimeH8Error::IdentityMismatch { field: "taskId", .. })
    ));
}

#[test]
fn opaque_handle_lifecycle_is_bounded_and_reads_or_duplicates_do_not_renew() {
    let temp = tempfile::tempdir().unwrap();
    let store = RecoveryStore::at(temp.path());
    let s = scope(temp.path(), "lease-session");
    let bytes = b"repeatable exact payload";
    let first = store.publish(&s, bytes, 100, 1).unwrap();
    assert_ne!(first.handle, format!("mr://anchor/{}", recovery::digest(bytes)));
    assert_eq!(store.resolve(&s, &first.handle, &Selector::Whole, 128, 50).unwrap().reference.expires_at, 101);
    assert_eq!(store.publish(&s, bytes, 1_000, 50).unwrap().expires_at, 101, "duplicate publication cannot renew");
    assert!(matches!(store.resolve(&s, &first.handle, &Selector::Whole, 128, 101), Err(RecoveryError::Expired)));

    let second = store.publish(&s, bytes, 100, 102).unwrap();
    assert_ne!(first.handle, second.handle, "expired handle generation must not resurrect");
    store.invalidate(&s, &second.handle).unwrap();
    assert!(matches!(store.resolve(&s, &second.handle, &Selector::Whole, 128, 103), Err(RecoveryError::Invalidated)));
    assert!(matches!(store.publish(&s, &vec![0u8; recovery::MAX_ARTIFACT_BYTES + 1], 100, 1), Err(RecoveryError::Limit)));
    assert!(matches!(store.resolve(&s, &second.handle, &Selector::Whole, recovery::MAX_RESTORE_BYTES + 1, 103), Err(RecoveryError::Limit)));
}

#[test]
fn final_model_facing_envelope_has_one_selected_body_and_enforced_bounds() {
    let selected = "selected representation only";
    let envelope = json!({"schemaVersion":1,"operation":"membrane_push_prepare","errorVersion":1,
        "result":{"kind":"success","data":{"text":selected,"representation":"reduced"}}});
    let fitted = egress::fit_native_response(envelope.clone(), &ceiling(10_000)).unwrap();
    let wire = membrane_mcp::tool_result(fitted.clone()).to_string();
    assert_eq!(fitted["result"]["data"]["text"], selected);
    assert_eq!(wire.matches(selected).count(), 1, "selected body is sole model-facing payload");
    assert_eq!(fitted["result"]["data"]["deliveryMeasurement"]["bytes"], wire.len());
    assert!(matches!(egress::fit_native_response(envelope, &ceiling(1)), Err(RecoveryError::Limit)));
}

#[test]
fn push_api_accepts_one_megabyte_utf8_text_before_authorization() {
    let text = "é".repeat(524_288);
    let result = membrane_runtime::push::api::execute(
        "membrane_push_prepare",
        &json!({"request":{"text":text,"maxBytes":49152}}),
    );
    assert_ne!(result["result"]["code"], "push_input_limit");
}

#[test]
fn push_api_rejects_text_over_one_megabyte_in_utf8_bytes() {
    let text = "é".repeat(524_289);
    let result = membrane_runtime::push::api::execute(
        "membrane_push_prepare",
        &json!({"request":{"text":text,"maxBytes":49152}}),
    );
    assert_eq!(result["result"]["code"], "push_input_limit");
}

#[test]
fn push_api_accepts_escaped_payload_within_eight_megabyte_serialized_bound() {
    let text = "\\\"".repeat(350_000);
    let serialized = serde_json::to_vec(&json!({"request":{"text":text,"maxBytes":49152}})).unwrap();
    assert!(serialized.len() > 1_048_576 && serialized.len() < 8 * 1_048_576);
    let result = membrane_runtime::push::api::execute(
        "membrane_push_prepare",
        &json!({"request":{"text":"\\\"".repeat(350_000),"maxBytes":49152}}),
    );
    assert_ne!(result["result"]["code"], "push_resource_limit");
    assert_ne!(result["result"]["code"], "push_input_limit");
}

#[test]
fn non_push_native_mcp_arguments_keep_the_sixty_four_kibibyte_cap() {
    let result = membrane_mcp::validate_arguments(
        "membrane_context",
        &json!({"task":"x".repeat(65_537)}),
    );
    assert!(result.unwrap_err().contains("65536"));
}

#[test]
fn push_schema_accepts_optional_canonical_h8_without_changing_legacy_shape() {
    let caller = json!({"root":"D:/workspace","repositoryId":"repo","scopeId":"session-push-test"});
    let base = json!({"repository":"repo","caller":caller,"request":{"text":"x","maxBytes":2048}});
    assert!(membrane_mcp::validate_arguments("membrane_push_prepare", &base).is_ok());
    let with_h8 = json!({"repository":"repo","caller":caller,"taskId":"task-push-test",
        "remainingContextCeiling":serde_json::to_value(ceiling(10_000)).unwrap(),
        "request":{"text":"x","maxBytes":2048}});
    assert!(membrane_mcp::validate_arguments("membrane_push_prepare", &with_h8).is_ok());
}

#[test]
fn push_api_prepare_and_resolve_fit_valid_h8_and_refuse_identity_mismatch() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let registry = temp.path().join("registry.json");
    std::fs::write(&registry, serde_json::json!({"schema_version":2,"bindings":{
        root.to_string_lossy().as_ref(): {"repository_id":"repo","scope_id":"session-api","grant_policy":{"level":"read-only"}}
    }}).to_string()).unwrap();
    let old_registry = std::env::var_os("MEMBRANE_PROJECT_REGISTRY");
    let old_anchor = std::env::var_os("MEMBRANE_ANCHOR_DIR");
    std::env::set_var("MEMBRANE_PROJECT_REGISTRY", &registry);
    std::env::set_var("MEMBRANE_ANCHOR_DIR", temp.path().join("store"));

    let caller = json!({"root":root,"repositoryId":"repo","scopeId":"session-api"});
    let probe = membrane_runtime::push::api::execute("membrane_push_resolve", &json!({
        "repository":"repo","caller":caller.clone(),"operation":"probe"}));
    let token = probe.pointer("/result/data/resolverToken").and_then(|v| v.as_str()).unwrap().to_owned();
    let mut h8 = serde_json::to_value(ceiling(100_000)).unwrap();
    h8["sessionId"] = json!("session-api");
    h8["taskId"]["value"] = json!("task-api");
    let original = "repeated event\n".repeat(500);
    let prepared = membrane_runtime::push::api::execute("membrane_push_prepare", &json!({
        "repository":"repo","caller":caller.clone(),"sessionId":"session-api","taskId":"task-api",
        "remainingContextCeiling":h8.clone(),"request":{"text":original,"kind":"log","maxBytes":2500,"resolverToken":token,"optimize":true}}));
    assert_eq!(prepared["result"]["kind"], "success", "{prepared}");
    let data = &prepared["result"]["data"];
    let handle = data["recovery"]["handle"].as_str().unwrap();
    assert!(data["deliveryMeasurement"]["bytes"].as_u64().unwrap() > 0);
    let resolved = membrane_runtime::push::api::execute("membrane_push_resolve", &json!({
        "repository":"repo","caller":caller.clone(),"sessionId":"session-api","taskId":"task-api",
        "remainingContextCeiling":h8.clone(),"operation":"resolve","handle":handle,"maxBytes":20_000}));
    assert_eq!(resolved["result"]["kind"], "success", "{resolved}");
    assert!(resolved["result"]["data"]["deliveryMeasurement"]["bytes"].as_u64().is_some());
    let refused = membrane_runtime::push::api::execute("membrane_push_resolve", &json!({
        "repository":"repo","caller":caller.clone(),"sessionId":"wrong-session","taskId":"task-api",
        "remainingContextCeiling":h8.clone(),"operation":"resolve","handle":handle,"maxBytes":20_000}));
    assert_eq!(refused["result"]["code"], "push_h8_invalid");
    let mut stale_h8 = h8;
    stale_h8["requestedAtUnixMs"] = json!(0);
    let stale = membrane_runtime::push::api::execute("membrane_push_resolve", &json!({
        "repository":"repo","caller":caller,"sessionId":"session-api","taskId":"task-api",
        "remainingContextCeiling":stale_h8,"operation":"resolve","handle":handle,"maxBytes":20_000}));
    assert_eq!(stale["result"]["code"], "push_h8_invalid");

    match old_registry { Some(v) => std::env::set_var("MEMBRANE_PROJECT_REGISTRY", v), None => std::env::remove_var("MEMBRANE_PROJECT_REGISTRY") }
    match old_anchor { Some(v) => std::env::set_var("MEMBRANE_ANCHOR_DIR", v), None => std::env::remove_var("MEMBRANE_ANCHOR_DIR") }
}
