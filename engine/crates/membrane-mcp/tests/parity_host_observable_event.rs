//! Parity for legacy `mcp/host/host-observable-event.test.mjs`.

use membrane_mcp::host_observable_event::{build_observable_event, BuildObservableEvent, EVENT_TYPES};

#[test]
fn observable_event_factory_covers_every_required_event_class_and_preserves_lineage() {
    let required = [
        "user", "assistant", "model", "packet_delivered", "tool_receipt", "git", "file", "test",
        "error", "retry", "gate", "delegation", "cost", "correction",
    ];
    for r in required {
        assert!(EVENT_TYPES.contains(&r), "missing required event type: {r}");
    }
    let event = build_observable_event(BuildObservableEvent {
        installation_id: "i",
        client_id: "claude_code",
        session_id: "s",
        task_id: "t",
        turn_id: "u",
        trace_id: "x",
        event_type: "correction",
        origin: "user",
        content: "private",
        completeness: vec![("receipt", true)],
        timestamp: "2026-08-01T00:00:00.000Z".to_string(),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(event["schema"], "membrane.observable-event.v1");
    assert!(event["event_id"].as_str().unwrap().starts_with("observable-"));
    assert!(event["content_ref_or_digest"].as_str().unwrap().starts_with("sha256:"));
    assert!(!serde_json::to_string(&event).unwrap().contains("private"));

    let err = build_observable_event(BuildObservableEvent {
        installation_id: "i",
        client_id: "c",
        session_id: "s",
        task_id: "t",
        turn_id: "u",
        trace_id: "x",
        event_type: "unknown",
        origin: "host",
        ..Default::default()
    })
    .unwrap_err();
    assert!(err.contains("unsupported observable event type"));
}
