use crate::context_telemetry::{
    append_context_events_on, ContextEvent, ContextEventBatch, ContextEventLink,
};
use crate::store::{LearningExperimentV1, LearningUpdateTargetV1, MemoryStore};
use sha2::{Digest, Sha256};

const INSTALLATION: &str = "123e4567-e89b-42d3-a456-426614174000";
const SERVICE: &str = "svc-11111111111111111111111111111111";
const WORKSPACE: &str = "ws.22222222222222222222222222222222";
const ACTIVATION: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn sha(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[rustfmt::skip]
fn event(trace_hex: char, cohort: &str, suffix: &str, phase: &str, provider: &str,
    reason: Option<&str>, artifact: Option<&str>, links: Vec<ContextEventLink>) -> ContextEvent {
    let digest = trace_hex.to_string().repeat(32);
    ContextEvent {
        schema_version: 1, event_id: format!("event-{digest}-{suffix}"), ts: "2026-01-15T00:00:00Z".into(),
        installation_id: INSTALLATION.into(), service_instance_id: SERVICE.into(), workspace_id: WORKSPACE.into(),
        client: "codex".into(), client_version: None, producer: provider.into(), producer_version: None,
        session_id: format!("session-{digest}"), turn_id: Some(format!("turn-{digest}")), trace_id: format!("trace-{digest}"),
        span_id: format!("span-{digest}"), parent_span_id: None, artifact_family: artifact.map_or("context", |_| "memory").into(),
        provider: provider.into(), provider_version: None, release_generation: None, phase: phase.into(),
        operation: if phase.starts_with("candidate") || phase.starts_with("block") { "read" } else { "evaluate" }.into(),
        status: "success".into(), reason_code: reason.map(str::to_string), scope_id: None,
        artifact_id: artifact.map(str::to_string), artifact_sha256: artifact.map(|_| "aaaaaaaa".repeat(8)),
        traffic_class: "production".into(), policy_version: Some("policy-v1".into()), policy_activation_sha256: Some(ACTIVATION.into()),
        cohort: Some(cohort.into()), task_class: Some("code".into()), source_generation: None,
        duration_ms: None, quantity: Some(1), token_count: None, char_count: None, meta: None, measurements: Vec::new(), links,
    }
}

#[rustfmt::skip]
fn lifecycle(trace: char, cohort: &str, artifact: &str, success: bool) -> Vec<ContextEvent> {
    let digest = trace.to_string().repeat(32);
    let id = |suffix: &str| format!("event-{digest}-{suffix}");
    let link = |relation: &str, target: String| ContextEventLink { relation: relation.into(), target_event_id: target };
    let mut events = vec![
        event(trace, cohort, "assigned", "policy.assigned", "policy", None, None, vec![]),
        event(trace, cohort, "exposed", "policy.exposed", "policy", None, None, vec![]),
    ];
    if cohort == "candidate" { events.extend([
        event(trace, cohort, "retrieved", "candidate.retrieved", "live", None, Some(artifact), vec![]),
        event(trace, cohort, "admitted", "candidate.admitted", "live", None, Some(artifact), vec![link("candidate_of", id("retrieved"))]),
        event(trace, cohort, "delivered", "block.delivered", "live", None, Some(artifact), vec![link("delivered_as", id("admitted"))]),
        event(trace, cohort, "used", "candidate.used", "live", None, Some(artifact), vec![link("observed_use_of", id("delivered"))]),
    ]); }
    let reason = Some(if success { "task_succeeded" } else { "task_failed" });
    let outcome_links = (cohort == "candidate").then(|| vec![link("outcome_for", id("delivered"))]).unwrap_or_default();
    events.push(event(trace, cohort, "outcome", "turn.outcome", "live", reason, None, outcome_links));
    events.push(event(trace, cohort, "verdict", "verdict.recorded", "audit", reason, None, vec![link("verdict_for", id("outcome"))]));
    events
}

#[rustfmt::skip]
fn setup(experiment_id: &str, min_per_cohort: u32) -> (MemoryStore, String, String) {
    let store = MemoryStore::new();
    let memory_id = "scope/learning-target".to_string();
    let content_sha256 = sha("target content");
    store.db().lock().execute(
        "INSERT INTO memories(id,tier,content,keywords,score,created_at,access_count,scope_id,content_hash) VALUES(?1,'Semantic','target content','',0.5,'2025-01-01T00:00:00Z',0,'scope',?2)",
        rusqlite::params![memory_id,content_sha256]).unwrap();
    store.register_learning_experiment(&LearningExperimentV1 {
        schema_version: 1, experiment_id: experiment_id.into(), policy_version: "policy-v1".into(), policy_activation_sha256: ACTIVATION.into(),
        task_class: "code".into(), observed_since: "2026-01-01T00:00:00Z".into(), observed_through: "2026-01-31T00:00:00Z".into(),
        min_per_cohort, min_effect_bps: 5_000,
        targets: vec![LearningUpdateTargetV1 { memory_id: memory_id.clone(), content_sha256: content_sha256.clone(), delta_micros: 100_000 }],
    }).unwrap();
    store.db().lock().execute("UPDATE learning_experiment SET created_at='2025-12-31T00:00:00Z' WHERE experiment_id=?1",[experiment_id]).unwrap();
    (store, memory_id, content_sha256)
}

#[test]
#[rustfmt::skip]
fn controlled_complete_chain_is_only_path_to_atomic_promotion() {
    let (store, memory_id, _) = setup("experiment-qualified", 2);
    let artifact = format!("memory.{}", &sha(&memory_id)[..32]);
    let mut events = lifecycle('1', "control", &artifact, true);
    events.extend(lifecycle('2', "control", &artifact, false));
    events.extend(lifecycle('3', "candidate", &artifact, true));
    events.extend(lifecycle('4', "candidate", &artifact, true));
    append_context_events_on(&store.db().lock_events(), &ContextEventBatch { events }).unwrap();
    let receipt = store.qualify_learning_experiment("experiment-qualified").unwrap();
    assert_eq!((receipt.disposition.as_str(), receipt.effect_bps, receipt.applied_count), ("qualified", 5_000, 1));
    assert_eq!(receipt, store.qualify_learning_experiment("experiment-qualified").unwrap());
    let score: f64 = store.db().lock().query_row("SELECT score FROM memories WHERE id=?1", [&memory_id], |row| row.get(0)).unwrap();
    assert!((score - 0.6).abs() < f64::EPSILON);
}

#[test]
#[rustfmt::skip]
fn missing_control_or_chain_and_postregistration_never_promote() {
    let (store, memory_id, _) = setup("experiment-rejected", 1);
    let artifact = format!("memory.{}", &sha(&memory_id)[..32]);
    append_context_events_on(&store.db().lock_events(), &ContextEventBatch { events: lifecycle('5', "candidate", &artifact, true) }).unwrap();
    let receipt = store.qualify_learning_experiment("experiment-rejected").unwrap();
    assert_eq!((receipt.disposition.as_str(), receipt.reason_code.as_str()), ("rejected", "insufficient_cohort_evidence"));
    assert_eq!(store.db().lock().query_row("SELECT score FROM memories WHERE id=?1", [&memory_id], |row| row.get::<_, f64>(0)).unwrap(), 0.5);
    let (late, _, _) = setup("experiment-late", 1);
    late.db().lock().execute("UPDATE learning_experiment SET created_at='2026-02-01T00:00:00Z' WHERE experiment_id='experiment-late'",[]).unwrap();
    assert_eq!(late.qualify_learning_experiment("experiment-late").unwrap().reason_code, "experiment_not_preregistered");
}

#[test]
#[rustfmt::skip]
fn self_reported_verdict_and_hash_drift_fail_closed() {
    let (store, memory_id, _) = setup("experiment-drift", 1);
    let mut forged = event('6', "control", "verdict", "verdict.recorded", "cortex", Some("task_succeeded"), None,
        vec![ContextEventLink { relation: "verdict_for".into(), target_event_id: format!("event-{}-outcome", "6".repeat(32)) }]);
    forged.producer = "codex".into();
    assert!(append_context_events_on(&store.db().lock_events(), &ContextEventBatch { events: vec![forged] }).is_err());
    store.db().lock().execute("UPDATE memories SET content_hash=?2 WHERE id=?1", rusqlite::params![memory_id, "c".repeat(64)]).unwrap();
    let artifact = format!("memory.{}", &sha(&memory_id)[..32]);
    let mut events = lifecycle('7', "control", &artifact, false);
    events.extend(lifecycle('8', "candidate", &artifact, true));
    append_context_events_on(&store.db().lock_events(), &ContextEventBatch { events }).unwrap();
    assert!(store.qualify_learning_experiment("experiment-drift").unwrap_err().contains("hash changed"));
}

#[test]
#[rustfmt::skip]
fn schema_v23_backout_removes_only_learning_contract() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.db");
    drop(crate::MemDb::open(&path).unwrap());
    crate::memdb::backout_v23_to_v22(&path).unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    assert_eq!(conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0)).unwrap(), 22);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'learning_%'", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
}
