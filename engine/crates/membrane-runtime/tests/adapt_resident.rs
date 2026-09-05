use membrane_protocol::host_observation::*;
use membrane_runtime::{adapt_observations::*, adapt_service, MemDb, MemoryStore};
use serde_json::{json, Value};
fn observation(id: &str, kind: &str, exit: Option<i32>) -> ExecutionObservationV1 {
    let unavailable = json!({"coverage":"unavailable","unavailableReason":"not_instrumented"});
    let complete = |value: Value| json!({"coverage":"complete","value":value});
    let mut v = json!({"schemaVersion":1,"observationId":id,"sessionId":"session","taskId":complete(json!("task")),
        "observedAtUnixMs":100,"model":"model","provider":"provider","client":"test","observationKind":kind,
        "scope":complete(json!("repo")),"callId":complete(json!("test-call")),
        "exitCode":exit.map(|e|complete(json!(e))).unwrap_or(unavailable.clone()),
        "provenanceReceipt":{"schemaVersion":1,"receiptId":format!("receipt-{id}"),"source":"host","observedAtUnixMs":100,"receiptDigest":format!("sha256:{}","a".repeat(64))}});
    for field in [
        "parentTaskId",
        "agentId",
        "agentRole",
        "routePolicy",
        "subjectId",
        "tool",
        "outcome",
        "durationMs",
        "usage",
        "toolCost",
        "assetCost",
        "repository",
        "artifactRefs",
        "evidenceRefs",
        "completion",
    ] {
        v[field] = unavailable.clone();
    }
    serde_json::from_value(v).unwrap()
}
fn window(id: &str, cursor: u64, events: Vec<ExecutionObservationV1>) -> AdaptObservationRequestV1 {
    AdaptObservationRequestV1::Analyze {
        scope: "repo".into(),
        window_id: id.into(),
        session_id: "session".into(),
        task_id: "task".into(),
        expected_cursor: cursor,
        required_call_ids: vec!["test-call".into()],
        observations: events
            .into_iter()
            .enumerate()
            .map(|(i, o)| SequencedObservationV1 {
                sequence: cursor + i as u64 + 1,
                observation: o,
            })
            .collect(),
    }
}
#[test]
fn live_status_distinguishes_missing_producer_from_empty_work() {
    let store = MemoryStore::new();
    let before = adapt_service::status(&store, "repo", None).unwrap();
    assert_eq!(
        before["lanes"]["insights"]["reason"],
        "producer_progress_unavailable"
    );
    assert_eq!(before["qualified"], false);
    let result = execute(
        &store,
        window(
            "w1",
            0,
            vec![
                observation("e1", "verification_result", Some(1)),
                observation("e2", "completion_claim_emitted", None),
            ],
        ),
    )
    .unwrap();
    assert_eq!(result["coverage"]["state"], "ran");
    assert_eq!(result["coverage"]["episodes"].as_array().unwrap().len(), 1);
    let after = adapt_service::status(&store, "repo", None).unwrap();
    assert!(after["lanes"]["insights"]["last_receipt"].is_object());
    assert_eq!(
        adapt_service::status(&store, "other", None).unwrap()["lanes"]["insights"]["last_receipt"],
        Value::Null
    );
}
#[test]
fn windows_resume_and_replay_without_duplicate_coverage() {
    let t = tempfile::tempdir().unwrap();
    let db = t.path().join("cortex.db");
    let store = MemoryStore::open(MemDb::open(&db).unwrap());
    execute(
        &store,
        window(
            "w1",
            0,
            vec![observation("e1", "verification_result", Some(1))],
        ),
    )
    .unwrap();
    drop(store);
    let store = MemoryStore::open(MemDb::open(&db).unwrap());
    let req = window(
        "w2",
        1,
        vec![observation("e2", "completion_claim_emitted", None)],
    );
    let result = execute(&store, req.clone()).unwrap();
    let replay = execute(&store, req).unwrap();
    assert_eq!(result["coverage"], replay["coverage"]);
    assert_eq!(
        store
            .db()
            .reference_events("repo", "adapt.detector_coverage", 128)
            .unwrap()
            .events
            .len(),
        2
    );
    assert!(execute(
        &store,
        window(
            "w3",
            2,
            vec![observation("e2", "completion_claim_emitted", None)]
        )
    )
    .is_err());
}
#[test]
fn missing_result_is_unknown_and_honest_failure_report_is_not_a_completion() {
    let store = MemoryStore::new();
    let result = execute(
        &store,
        window(
            "missing",
            0,
            vec![observation("claim", "completion_claim_emitted", None)],
        ),
    )
    .unwrap();
    assert_eq!(result["coverage"]["state"], "unavailable");
    assert!(result["coverage"]["episodes"]
        .as_array()
        .unwrap()
        .is_empty());
    let store = MemoryStore::new();
    let result = execute(
        &store,
        window(
            "honest",
            0,
            vec![observation("failure", "verification_result", Some(1))],
        ),
    )
    .unwrap();
    assert!(result["coverage"]["episodes"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[test]
fn passing_retry_clears_failed_verification() {
    let store = MemoryStore::new();
    let result = execute(
        &store,
        window(
            "retry",
            0,
            vec![
                observation("fail", "verification_result", Some(1)),
                observation("pass", "verification_result", Some(0)),
                observation("claim", "completion_claim_emitted", None),
            ],
        ),
    )
    .unwrap();
    assert_eq!(result["coverage"]["state"], "ran");
    assert!(result["coverage"]["episodes"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[test]
fn scoped_inspection_cannot_mutate() {
    let store = MemoryStore::new();
    let reply =
        adapt_service::inspect_preferences(&store, "global", Default::default(), None, None, 16)
            .unwrap();
    assert_eq!(reply["inspection_only"], true);
    assert_eq!(reply["exposure_recorded"], false);
    assert!(
        !store
            .db()
            .reference_events("global", "adapt.packet_emitted", 16)
            .unwrap()
            .available
    );
    assert!(adapt_service::inspect_preferences(
        &store,
        "global",
        Default::default(),
        None,
        None,
        33
    )
    .is_err());
}
#[test]
fn operator_dispatch_rejects_offline_commands_and_unknown_fields() {
    let store = MemoryStore::new();
    assert_ne!(
        adapt_service::operator_response(
            &store,
            r#"{"command":"mine","transcripts":[],"host":null,"scope":"global","min_recurrence":2}"#
        )
        .0,
        200
    );
    assert_ne!(
        adapt_service::operator_response(
            &store,
            r#"{"command":"status","scope":"global","approve":true}"#
        )
        .0,
        200
    );
    let result =
        adapt_service::operator_response(&store, r#"{"command":"status","scope":"global"}"#);
    assert_eq!(result.0, 200);
}
