//! Parity for legacy `mcp/host/host-boundary.test.mjs`.

use membrane_mcp::host_candidate_set::consume_candidate_set;
use membrane_mcp::host_capability_matrix::{
    capability_for, host_capability_matrix, validate_capability_matrix,
};
use membrane_mcp::host_continuity::{host_reference, ContinuityClient, ContinuityService};
use membrane_mcp::host_evidence_interceptor::{intercept_evidence, is_honest_delivery};
use serde_json::{json, Value};
use std::cell::RefCell;

#[test]
fn host_capability_matrix_covers_only_observed_claude_codex_seams() {
    let matrix = host_capability_matrix();
    assert!(validate_capability_matrix(&matrix).valid);
    assert_eq!(capability_for("claude_code", "delegated_agent_egress"), "projection");
    assert_eq!(capability_for("codex", "delegated_agent_egress"), "unavailable");
    assert_eq!(capability_for("unknown", "UserPromptSubmit"), "unavailable");
}

#[test]
fn evidence_interception_never_transports_directive_and_proves_delivery() {
    let input = json!({"directive": "change the plan", "evidence": {"traceId": "t1"}});
    let degraded = intercept_evidence(
        &input,
        "claude_code",
        "tool_result_egress",
        |evidence| Some(json!({"traceId": evidence["traceId"], "blocks": []})),
        None::<fn(&Value, &Value) -> bool>,
    );
    assert_eq!(degraded.state, "degraded");
    assert_eq!(degraded.reason, "delivery_unverified");

    let delivered = intercept_evidence(
        &input,
        "claude_code",
        "tool_result_egress",
        |evidence| {
            Some(json!({"traceId": evidence["traceId"], "blocks": [{"id": "evidence", "text": "data"}]}))
        },
        Some(|_packet: &Value, receipt: &Value| receipt["traceId"] == "t1"),
    );
    assert_eq!(delivered.state, "context_enforced");
    assert!(delivered.directive.is_none());
    assert_eq!(delivered.receipt["verified"], true);
    assert!(is_honest_delivery(&delivered));
}

struct RecordingService {
    calls: RefCell<Vec<String>>,
}
impl ContinuityService for RecordingService {
    fn call(&self, operation: &str, payload: Value) -> Option<Value> {
        self.calls.borrow_mut().push(operation.to_string());
        Some(json!({
            "ok": true,
            "checkpoint": payload.get("checkpoint").cloned().unwrap_or_else(|| json!({"id": payload["id"], "restored": true})),
        }))
    }
}

#[test]
fn continuity_sends_host_transcript_references_through_current_service() {
    let service = RecordingService { calls: RefCell::new(vec![]) };
    let client = ContinuityClient::new(Some(&service), || "2026-08-20T00:00:00.000Z".to_string());
    let saved = client.checkpoint(&json!({
        "sessionId": "s1",
        "transcriptRef": {"id": "host-log-1", "digest": "sha256:x", "host": "codex"},
        "trigger": "PreCompact",
    }));
    assert_eq!(saved.state, "available");
    let checkpoint = saved.checkpoint.unwrap();
    assert_eq!(checkpoint["transcriptRef"]["id"], "host-log-1");
    assert!(checkpoint["transcriptRef"].get("bytes").is_none());
    let restored = client.restore(&json!({"id": checkpoint["id"]}));
    assert_eq!(restored.state, "available");
    let calls = service.calls.borrow();
    assert_eq!(calls[0], "membrane_checkpoint_save");
    assert_eq!(calls[1], "membrane_checkpoint_load");
}

#[test]
fn continuity_reports_typed_degradation_instead_of_fallback_when_service_absent() {
    let client: ContinuityClient<'_, RecordingService> = ContinuityClient::new(None, || "now".to_string());
    let result = client.checkpoint(&json!({"transcriptRef": {"id": "host-log-1"}}));
    assert_eq!(result.state, "degraded");
    assert_eq!(result.reason, "membrane_service_unavailable");
    assert!(host_reference(&json!({})).is_none());
}

#[test]
fn candidate_consumer_requires_blueprint_trace_and_index_identity() {
    let consumed = consume_candidate_set(
        &json!({"traceId": "t1", "freshness": {"indexedAt": "2026-08-20T00:00:00.000Z"}, "candidates": [], "omissions": []}),
        None,
    );
    assert_eq!(consumed.state, "available");
    assert_eq!(consumed.candidate_set.unwrap()["indexedAt"], "2026-08-20T00:00:00.000Z");
    assert!(consumed.receipt.unwrap()["candidateSetDigest"].as_str().unwrap().starts_with("sha256:"));
    assert_eq!(consume_candidate_set(&json!({"traceId": "t1", "freshness": {}}), None).state, "degraded");
}
