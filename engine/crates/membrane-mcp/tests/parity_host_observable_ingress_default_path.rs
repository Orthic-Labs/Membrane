//! Parity for legacy `mcp/host/host-observable-ingress-default-path.test.mjs`.

use membrane_mcp::host_observable_event::{build_observable_event, BuildObservableEvent};
use membrane_mcp::host_observable_ingress::{
    append_observable_event, resolve_default_ingress_target, resolve_runtime_config_identity,
    AppendOutcome, ResolvedIngressTarget,
};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn sample_event(trace_id: &str) -> serde_json::Value {
    build_observable_event(BuildObservableEvent {
        installation_id: "install-1",
        client_id: "codex",
        session_id: "session-1",
        task_id: "task-1",
        turn_id: "turn-1",
        trace_id,
        event_type: "tool_receipt",
        origin: "tool",
        content: "private command output",
        completeness: vec![("receipt", true)],
        timestamp: "2026-08-01T00:00:00.000Z".to_string(),
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn a_membrane_local_v1_runtime_json_resolves_the_default_ingress_identity() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("runtime.json");
    fs::write(&config_path, json!({"schemaVersion": 1, "serviceId": "membrane-local-v1", "host": "127.0.0.1", "port": 47851}).to_string()).unwrap();
    let resolved = resolve_default_ingress_target(&config_path, None).unwrap();
    assert!(resolved.target.to_string_lossy().ends_with("context-telemetry-ingress.jsonl"));
    assert_eq!(resolved.target.parent(), resolved.db_path.parent());
}

#[test]
fn default_path_is_used_and_honored_when_unset_and_db_exists() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("cortex-engine.db");
    fs::write(&db_path, "").unwrap(); // simulate: resident service has initialized its db
    let resolved = ResolvedIngressTarget { target: dir.path().join("context-telemetry-ingress.jsonl"), db_path };
    let event = sample_event("trace-default-ok");
    let outcome = append_observable_event(&event, None, Some(&resolved)).unwrap();
    assert!(matches!(outcome, AppendOutcome::Persisted));
    let record: serde_json::Value = serde_json::from_str(&fs::read_to_string(&resolved.target).unwrap()).unwrap();
    assert_eq!(record["observable_events"][0], event);
}

#[test]
fn resolved_but_service_not_running_reports_distinct_honest_reason_never_a_false_persisted() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("cortex-engine.db"); // deliberately never created
    let resolved = ResolvedIngressTarget { target: dir.path().join("context-telemetry-ingress.jsonl"), db_path };
    let outcome = append_observable_event(&sample_event("trace-no-service"), None, Some(&resolved)).unwrap();
    assert!(matches!(outcome, AppendOutcome::Unavailable("telemetry_ingress_drain_service_not_running")));
    assert!(!resolved.target.exists());
}

#[test]
fn explicit_target_still_overrides_default_bypassing_the_drain_evidence_gate() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("events.jsonl");
    let event = sample_event("trace-explicit");
    let outcome = append_observable_event(&event, Some(&target), None).unwrap();
    assert!(matches!(outcome, AppendOutcome::Persisted));
    let record: serde_json::Value = serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
    assert_eq!(record["observable_events"][0], event);
}

#[test]
fn unresolvable_config_missing_file_returns_none_never_panics() {
    let missing: PathBuf = Path::new("forge-does-not-exist").join("runtime.json");
    assert!(resolve_default_ingress_target(&missing, None).is_none());
}

#[test]
fn unresolvable_config_malformed_json_returns_none() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("runtime.json");
    fs::write(&config_path, "{ not valid json").unwrap();
    assert!(resolve_default_ingress_target(&config_path, None).is_none());
}

#[test]
fn unresolvable_config_wrong_identity_returns_none_for_each_mismatch() {
    let dir = tempdir().unwrap();
    let cases = [
        json!({"schemaVersion": 1, "serviceId": "not-membrane-local-v1", "host": "127.0.0.1", "port": 47851}),
        json!({"schemaVersion": 2, "serviceId": "membrane-local-v1", "host": "127.0.0.1", "port": 47851}),
        json!({"schemaVersion": 1, "serviceId": "membrane-local-v1", "host": "0.0.0.0", "port": 47851}),
    ];
    for (index, case) in cases.iter().enumerate() {
        let config_path = dir.path().join(format!("runtime-{index}.json"));
        fs::write(&config_path, case.to_string()).unwrap();
        assert!(resolve_default_ingress_target(&config_path, None).is_none());
        assert!(resolve_runtime_config_identity(&config_path).is_err());
    }
}

#[test]
fn unresolvable_default_resolution_surfaces_as_honest_unavailable() {
    let outcome = append_observable_event(&sample_event("trace-unresolvable"), None, None).unwrap();
    assert!(matches!(outcome, AppendOutcome::Unavailable("telemetry_ingress_unconfigured")));
}

#[test]
fn forbidden_field_validation_still_rejects_tampered_records() {
    let mut event = sample_event("trace-forbidden");
    event["extra_field"] = json!("nope");
    let err = append_observable_event(&event, Some(Path::new("ignored.jsonl")), None).unwrap_err();
    assert!(err.contains("forbidden field"));
}
