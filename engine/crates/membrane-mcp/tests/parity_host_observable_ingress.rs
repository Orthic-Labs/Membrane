//! Parity for legacy `mcp/host/host-observable-ingress.test.mjs`.

use membrane_mcp::host_observable_event::{build_observable_event, BuildObservableEvent};
use membrane_mcp::host_observable_ingress::{append_observable_event, AppendOutcome};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn observable_ingress_fsyncs_one_membrane_owned_content_free_record() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("events.jsonl");
    let event = build_observable_event(BuildObservableEvent {
        installation_id: "install-1",
        client_id: "codex",
        session_id: "session-1",
        task_id: "task-1",
        turn_id: "turn-1",
        trace_id: "trace-1",
        event_type: "tool_receipt",
        origin: "tool",
        content: "private command output",
        completeness: vec![("receipt", true)],
        timestamp: "2026-08-01T00:00:00.000Z".to_string(),
        ..Default::default()
    })
    .unwrap();
    let outcome = append_observable_event(&event, Some(&target), None).unwrap();
    assert!(matches!(outcome, AppendOutcome::Persisted));
    let record: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
    assert_eq!(record["observable_events"][0], event);

    let mut tampered = event.clone();
    tampered["content"] = json!("private command output");
    let err = append_observable_event(&tampered, Some(&target), None).unwrap_err();
    assert!(err.contains("forbidden field"));
}
