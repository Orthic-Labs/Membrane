//! Parity for legacy `mcp/host/host-context-adapter.test.mjs`, scoped to the
//! pure identity/envelope logic this lane ports (see `host_context_adapter`
//! module docs for the transport/render scope note).

use membrane_mcp::host_context_adapter::{build_request, default_client, CLIENT_IDENTITIES};
use membrane_mcp::host_observable_event::{build_observable_event, validate_observable_event, BuildObservableEvent};

#[test]
fn host_adapter_emits_typed_client_identity_never_a_gateway_derived_alias() {
    let typed = ["claude_code", "codex", "mcp", "api_worker", "other"];
    assert!(typed.contains(&default_client(None, None, None)));
    assert_eq!(default_client(None, None, None), "claude_code");
    assert_eq!(default_client(None, Some("thread-1"), None), "codex");
    assert_eq!(default_client(Some("mcp"), None, None), "mcp");
    assert_eq!(default_client(Some("ccx"), None, None), "other");
    for id in CLIENT_IDENTITIES {
        assert!(typed.contains(id));
    }
}

#[test]
fn packet_delivery_char_count_is_bounded_packet_only_and_content_free() {
    let event = build_observable_event(BuildObservableEvent {
        installation_id: "installation-1",
        client_id: "codex",
        session_id: "session-1",
        task_id: "task-1",
        turn_id: "turn-1",
        trace_id: "trace-1",
        event_type: "packet_delivered",
        origin: "host",
        content: "private content",
        char_count: Some(42),
        timestamp: "2026-08-01T00:00:00.000Z".to_string(),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(event["char_count"], 42);
    let mut too_big = event.clone();
    too_big["char_count"] = serde_json::json!(30001);
    assert!(validate_observable_event(&too_big).unwrap_err().contains("char_count"));
    let mut wrong_type = event;
    wrong_type["event_type"] = serde_json::json!("tool_receipt");
    assert!(validate_observable_event(&wrong_type).unwrap_err().contains("packet_delivered"));
}

#[test]
fn host_adapter_builds_a_typed_task_and_turn_envelope_for_canonical_request() {
    let request = build_request(
        Some("session-1"),
        Some("inspect current graph"),
        std::env::temp_dir().to_str().unwrap(),
        "claude_code",
        Some("turn-1"),
        None,
        4242,
    );
    assert_eq!(request.session, "session-1");
    assert_eq!(request.turn_envelope_turn_id, "turn-1");
    assert!(!request.task_envelope_task_id.is_empty());
}
