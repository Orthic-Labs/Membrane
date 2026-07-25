#![recursion_limit = "256"]

use memright::context_telemetry::{
    canonical_event_bytes, parse_context_event, validate_context_event, ContextEventBatch,
    ContextTelemetryError, OperationAttribution,
};
use memright::installation_identity::{InstallationIdentity, StartupClaim};
use memright::memdb::{backout_v12_to_v11, backout_v13_to_v12};
use memright::{MemDb, MemoryStore};
use rusqlite::Connection;
use serde_json::{json, Value};
use sha2::Digest;

fn event(event_id: &str) -> Value {
    let digest32 =
        |value: &str| format!("{:x}", sha2::Sha256::digest(value.as_bytes()))[..32].to_string();
    let opaque_event_id = format!("evt-{:x}", sha2::Sha256::digest(event_id.as_bytes()));
    json!({
        "schema_version": 1,
        "event_id": opaque_event_id,
        "ts": "2026-07-20T08:00:00Z",
        "installation_id": "018f0d1e-9a1f-4bb8-8da7-52f152923102",
        "service_instance_id": "018f0d1e-9a1f-4bb8-8da7-52f152923103",
        "workspace_id": format!("ws.{}", digest32("workspace")),
        "client": "codex",
        "client_version": "1.2.3",
        "producer": "ingest_hook",
        "producer_version": "2.0.0",
        "session_id": format!("session-{}", digest32("session-7")),
        "turn_id": format!("turn-{}", digest32("turn-3")),
        "trace_id": format!("trace-{}", digest32("trace-9")),
        "span_id": format!("span-{}", digest32("span-2")),
        "parent_span_id": format!("span-{}", digest32("span-1")),
        "artifact_family": "memory",
        "provider": "memright",
        "provider_version": "0.1.1",
        "release_generation": format!("sha256:{}", "c".repeat(64)),
        "phase": "provider.started",
        "operation": "read",
        "status": "started",
        "scope_id": format!("scope-{}", digest32("D--Claude")),
        "artifact_id": format!("memory.{}", digest32("memory-42")),
        "artifact_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "traffic_class": "production",
        "policy_version": "rc-v1",
        "policy_activation_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "cohort": "candidate",
        "task_class": "coding",
        "source_generation": 4,
        "duration_ms": 2.5,
        "quantity": 1,
        "token_count": 12,
        "char_count": 48,
        "meta": {"cache_hit": false, "retry_count": 0, "transport": "http"},
        "measurements": [
            {"name": "latency", "value": 2.5, "unit": "ms"},
            {"name": "cache_hit", "value": false, "unit": "boolean"}
        ],
        "links": [
            {"relation": "attempt_of", "target_event_id": format!("evt-{}", digest32("evt-parent"))}
        ]
    })
}

fn batch(events: Vec<Value>) -> ContextEventBatch {
    serde_json::from_value(json!({"events": events})).unwrap()
}

const LOCAL_INSTALLATION_ID: &str = "018f0d1e-9a1f-4bb8-8da7-52f152923102";
const FIRST_SERVICE_ID: &str = "018f0d1e-9a1f-4bb8-8da7-52f152923103";
const SECOND_SERVICE_ID: &str = "018f0d1e-9a1f-4bb8-8da7-52f152923104";

fn startup(generation: u64, service_instance_id: &str) -> (InstallationIdentity, StartupClaim) {
    let claimed_at = format!("2026-07-20T08:00:{generation:02}Z");
    (
        InstallationIdentity {
            schema_version: 2,
            installation_id: LOCAL_INSTALLATION_ID.to_string(),
            created_at: "2026-07-19T08:00:00Z".to_string(),
            startup_generation: generation,
            legacy_labels: vec![],
            lineage: vec![],
            current_service_instance_id: Some(service_instance_id.to_string()),
            current_claimed_at: Some(claimed_at.clone()),
        },
        StartupClaim {
            schema_version: 1,
            installation_id: LOCAL_INSTALLATION_ID.to_string(),
            startup_generation: generation,
            service_instance_id: service_instance_id.to_string(),
            claimed_at,
        },
    )
}

fn local_event(event_id: &str, service_instance_id: &str) -> Value {
    let mut value = event(event_id);
    value["installation_id"] = json!(LOCAL_INSTALLATION_ID);
    value["service_instance_id"] = json!(service_instance_id);
    value
}

fn route_with_startup(
    store: &MemoryStore,
    identity: &InstallationIdentity,
    claim: &StartupClaim,
    body: &str,
) -> (u16, String) {
    memright::serve::route_for_tests_with_startup_claim(
        store,
        identity,
        claim,
        "POST",
        "/v1/telemetry/events:batch",
        body,
    )
}

#[test]
fn shared_fixture_validates_and_matches_language_neutral_canonical_digest() {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../pipelines/memory/fixtures/context-telemetry-events-v1.json");
    let fixture: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
    let valid = fixture["valid_events"].as_array().unwrap();
    for raw in valid {
        let event = parse_context_event(raw.clone()).unwrap();
        validate_context_event(&event).unwrap();
    }
    let first = parse_context_event(valid[0].clone()).unwrap();
    let digest = format!(
        "{:x}",
        sha2::Sha256::digest(canonical_event_bytes(&first).unwrap())
    );
    assert_eq!(digest, fixture["expected_canonical_sha256"]);

    for case in fixture["invalid_events"].as_array().unwrap() {
        let mut raw = valid[0].clone();
        for (key, value) in case["patch"].as_object().unwrap() {
            raw[key] = value.clone();
        }
        assert!(parse_context_event(raw).is_err(), "{}", case["name"]);
    }
}

#[test]
fn checked_in_registry_controls_provider_family_and_identity_retirement() {
    let mut extension = event("evt-extension");
    extension["provider"] = json!("ext.acme.vector");
    extension["artifact_family"] = json!("ext.acme.embedding");
    parse_context_event(extension).unwrap();

    let mut provider_typo = event("evt-provider-typo");
    provider_typo["provider"] = json!("memrigth");
    assert!(parse_context_event(provider_typo).is_err());

    let mut family_typo = event("evt-family-typo");
    family_typo["artifact_family"] = json!("memroy");
    assert!(parse_context_event(family_typo).is_err());

    let mut retired = event("evt-retired");
    retired["phase"] = json!("identity.retired");
    retired["operation"] = json!("reconcile");
    retired["status"] = json!("success");
    parse_context_event(retired).unwrap();
}

#[test]
fn checked_in_registry_enforces_phase_byte_and_turn_cardinality_budgets() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let mut oversized = event("evt-oversized");
    oversized["span_id"] = json!("span-oversized");
    oversized["links"] = json!((0..32)
        .map(|index| json!({
            "relation": "attempt_of",
            "target_event_id": format!("target-{index}-{}", "a".repeat(145))
        }))
        .collect::<Vec<_>>());
    let oversized_batch: ContextEventBatch =
        serde_json::from_value(json!({"events": [oversized]})).unwrap();
    assert!(store.db().ingest_context_events(&oversized_batch).is_err());

    let events: Vec<Value> = (0..5)
        .map(|index| {
            let mut value = event(&format!("evt-cursor-{index}"));
            value["span_id"] = json!(format!("span-cursor-{index}"));
            value["provider"] = json!("git");
            value["artifact_family"] = json!("context");
            value["phase"] = json!("replication.cursor");
            value["operation"] = json!("sync");
            value["status"] = json!("success");
            value["links"] = json!([]);
            value
        })
        .collect();
    let cardinality_batch: ContextEventBatch =
        serde_json::from_value(json!({"events": events})).unwrap();
    assert!(store
        .db()
        .ingest_context_events(&cardinality_batch)
        .is_err());
    let count: i64 = store
        .db()
        .lock()
        .query_row("SELECT COUNT(*) FROM context_event_log", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn latest_schema_has_content_free_ledger_tables_and_indexes() {
    let db = MemDb::open_in_memory();
    let conn = db.lock();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 18);

    for table in [
        "context_installation",
        "context_event_log",
        "context_event_measurement",
        "context_event_link",
        "context_ingest_checkpoint",
        "transform_opportunity_log",
    ] {
        let present: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(present, 1, "missing {table}");
    }

    let opportunity_column: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('transform_log') WHERE name='opportunity_uid'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(opportunity_column, 1);

    let event_columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('context_event_log') ORDER BY cid")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for forbidden in [
        "prompt",
        "query",
        "content",
        "path",
        "reply",
        "arguments",
        "error",
    ] {
        assert!(!event_columns.iter().any(|column| column == forbidden));
    }
    for required in [
        "seq",
        "event_id",
        "installation_id",
        "session_id",
        "trace_id",
        "artifact_family",
        "provider",
        "phase",
        "operation",
        "status",
        "policy_activation_sha256",
        "release_generation",
        "canonical_sha256",
    ] {
        assert!(event_columns.iter().any(|column| column == required));
    }

    let analytical_indexes: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name IN (
                'idx_context_event_installation_ts',
                'idx_context_event_session_ts',
                'idx_context_event_family_phase_status',
                'idx_context_event_trace_span'
            )",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(analytical_indexes, 4);
}

#[test]
fn v11_database_upgrades_in_place_without_changing_legacy_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v11-to-v12.db");
    let db = MemDb::open(&path).unwrap();
    drop(db);
    backout_v13_to_v12(&path).unwrap();
    backout_v12_to_v11(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO memory_event_log (ts,event_kind,surface,quantity,meta)
         VALUES ('2026-07-20T00:00:00Z','put','legacy',1,'{}')",
        [],
    )
    .unwrap();
    drop(conn);

    let upgraded = MemDb::open(&path).unwrap();
    let conn = upgraded.lock();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        18
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM memory_event_log", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        1
    );
}

#[test]
fn v12_backout_drops_only_ledger_tables_and_preserves_legacy_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v12-backout.db");
    let db = MemDb::open(&path).unwrap();
    db.lock()
        .execute(
            "INSERT INTO memory_event_log (ts,event_kind,surface,quantity,meta)
             VALUES ('2026-07-20T00:00:00Z','put','legacy',1,'{}')",
            [],
        )
        .unwrap();
    db.ingest_context_events(&batch(vec![event("evt-before-backout")]))
        .unwrap();
    drop(db);

    backout_v13_to_v12(&path).unwrap();
    backout_v12_to_v11(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        11
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM memory_event_log", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type='table' AND name LIKE 'context_event_%'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn v13_backout_and_reupgrade_preserve_ledger_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v13-round-trip.db");
    let db = MemDb::open(&path).unwrap();
    db.ingest_context_events(&batch(vec![event("evt-v13-round-trip")]))
        .unwrap();
    drop(db);

    backout_v13_to_v12(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        12
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM context_event_log", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('context_event_log') WHERE name='policy_activation_sha256'",
        [], |row| row.get::<_, i64>(0),
    ).unwrap(), 0);
    drop(conn);

    let upgraded = MemDb::open(&path).unwrap();
    let conn = upgraded.lock();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        18
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM context_event_log", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('context_event_log') WHERE name='policy_activation_sha256'",
        [], |row| row.get::<_, i64>(0),
    ).unwrap(), 1);
}

#[test]
fn batch_commit_persists_envelope_measurements_and_links_atomically() {
    let db = MemDb::open_in_memory();
    let receipt = db
        .ingest_context_events(&batch(vec![event("evt-1"), event("evt-2")]))
        .unwrap();
    assert_eq!((receipt.inserted, receipt.duplicates), (2, 0));
    assert!(receipt.complete);

    let conn = db.lock();
    assert_eq!(
        conn.query_row(
            "SELECT policy_activation_sha256 FROM context_event_log ORDER BY seq LIMIT 1",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "a".repeat(64)
    );
    assert_eq!(
        conn.query_row(
            "SELECT release_generation FROM context_event_log ORDER BY seq LIMIT 1",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        format!("sha256:{}", "c".repeat(64))
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM context_event_log", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM context_event_measurement",
            [],
            |row| { row.get::<_, i64>(0) }
        )
        .unwrap(),
        4
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM context_event_link", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn invalid_event_rejects_the_whole_batch_before_any_insert() {
    let db = MemDb::open_in_memory();
    let mut invalid = event("evt-invalid");
    invalid["status"] = json!("invented-status");
    let error = db
        .ingest_context_events(&batch(vec![event("evt-valid"), invalid]))
        .unwrap_err();
    assert!(matches!(error, ContextTelemetryError::Invalid(_)));
    assert_eq!(
        db.lock()
            .query_row("SELECT COUNT(*) FROM context_event_log", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn identical_replay_is_noop_and_changed_replay_conflicts_without_partial_commit() {
    let db = MemDb::open_in_memory();
    let original = event("evt-stable");
    let expected_event_id = original["event_id"].as_str().unwrap().to_string();
    db.ingest_context_events(&batch(vec![original.clone()]))
        .unwrap();
    let replay = db
        .ingest_context_events(&batch(vec![original.clone()]))
        .unwrap();
    assert_eq!((replay.inserted, replay.duplicates), (0, 1));

    let mut changed = original;
    changed["quantity"] = json!(2);
    let error = db
        .ingest_context_events(&batch(vec![event("evt-would-rollback"), changed]))
        .unwrap_err();
    assert!(matches!(
        error,
        ContextTelemetryError::Conflict { ref event_id } if event_id == &expected_event_id
    ));
    let conn = db.lock();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM context_event_log", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn validator_rejects_content_free_contract_violations() {
    let db = MemDb::open_in_memory();

    let mut terminal_without_reason = event("evt-no-reason");
    terminal_without_reason["status"] = json!("failed");
    assert!(matches!(
        db.ingest_context_events(&batch(vec![terminal_without_reason])),
        Err(ContextTelemetryError::Invalid(_))
    ));

    let mut arbitrary_meta = event("evt-meta");
    arbitrary_meta["meta"] = json!({"query": "raw user text"});
    assert!(matches!(
        db.ingest_context_events(&batch(vec![arbitrary_meta])),
        Err(ContextTelemetryError::Invalid(_))
    ));

    let oversized = "x".repeat(161);
    let mut oversized_session = event("evt-long");
    oversized_session["session_id"] = json!(oversized);
    assert!(matches!(
        db.ingest_context_events(&batch(vec![oversized_session])),
        Err(ContextTelemetryError::Invalid(_))
    ));

    let mut invalid_activation = event("evt-invalid-activation");
    invalid_activation["policy_activation_sha256"] = json!("A".repeat(64));
    assert!(matches!(
        db.ingest_context_events(&batch(vec![invalid_activation])),
        Err(ContextTelemetryError::Invalid(_))
    ));
}

#[test]
fn http_batch_endpoint_maps_created_replay_conflict_and_content_rejection() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let (identity, claim) = startup(1, FIRST_SERVICE_ID);
    let original = json!({"events": [local_event("evt-http", FIRST_SERVICE_ID)]}).to_string();
    let (created_status, created_body) = route_with_startup(&store, &identity, &claim, &original);
    assert_eq!(created_status, 201, "{created_body}");
    assert_eq!(
        serde_json::from_str::<Value>(&created_body).unwrap()["inserted"],
        1
    );

    let (replay_status, replay_body) = route_with_startup(&store, &identity, &claim, &original);
    assert_eq!(replay_status, 200, "{replay_body}");

    let mut changed = local_event("evt-http", FIRST_SERVICE_ID);
    changed["quantity"] = json!(9);
    let conflict_body = json!({"events": [changed]}).to_string();
    let (conflict_status, _) = route_with_startup(&store, &identity, &claim, &conflict_body);
    assert_eq!(conflict_status, 409);

    let mut leaked = local_event("evt-content", FIRST_SERVICE_ID);
    leaked["prompt"] = json!("must never be stored");
    let leaked_body = json!({"events": [leaked]}).to_string();
    let (rejected_status, _) = route_with_startup(&store, &identity, &claim, &leaked_body);
    assert_eq!(rejected_status, 400);
}

#[test]
fn telemetry_endpoint_requires_an_active_startup_lease() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let body = json!({"events": [local_event("evt-no-lease", FIRST_SERVICE_ID)]}).to_string();
    let (status, _) =
        memright::serve::route_for_tests(&store, "POST", "/v1/telemetry/events:batch", &body);
    assert_eq!(status, 503);
    assert_eq!(
        store
            .db()
            .lock()
            .query_row("SELECT COUNT(*) FROM context_event_log", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn telemetry_endpoint_binds_local_identity_and_populates_installation_metadata() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let (identity, claim) = startup(1, FIRST_SERVICE_ID);
    let body = json!({"events": [local_event("evt-bound-local", FIRST_SERVICE_ID)]}).to_string();
    let (status, response) = route_with_startup(&store, &identity, &claim, &body);
    assert_eq!(status, 201, "{response}");

    let conn = store.db().lock();
    let installation: (String, String, String, i64) = conn
        .query_row(
            "SELECT installation_id, os_type, arch, installation_epoch
               FROM context_installation",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(installation.0, LOCAL_INSTALLATION_ID);
    assert_eq!(installation.1, std::env::consts::OS);
    assert_eq!(installation.2, std::env::consts::ARCH);
    assert_eq!(installation.3, 1);
}

#[test]
fn telemetry_endpoint_accepts_queued_producer_after_same_installation_restart() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let (identity, restarted_claim) = startup(2, SECOND_SERVICE_ID);
    let body = json!({
        "events": [local_event("evt-queued-before-restart", FIRST_SERVICE_ID)]
    })
    .to_string();

    let (status, response) = route_with_startup(&store, &identity, &restarted_claim, &body);
    assert_eq!(status, 201, "{response}");
    let producer: String = store
        .db()
        .lock()
        .query_row(
            "SELECT service_instance_id FROM context_event_log LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(producer, FIRST_SERVICE_ID);
}

#[test]
fn telemetry_endpoint_rejects_foreign_identity_and_mixed_batch_atomically() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let (identity, claim) = startup(1, FIRST_SERVICE_ID);
    let mut foreign = local_event("evt-foreign", FIRST_SERVICE_ID);
    foreign["installation_id"] = json!("018f0d1e-9a1f-4bb8-8da7-52f152923105");
    let body = json!({
        "events": [local_event("evt-local-would-rollback", FIRST_SERVICE_ID), foreign]
    })
    .to_string();
    let (status, _) = route_with_startup(&store, &identity, &claim, &body);
    assert_eq!(status, 403);

    let conn = store.db().lock();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM context_event_log", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let installation_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM context_installation", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(installation_rows, 0);
}

#[test]
fn telemetry_endpoint_accepts_same_installation_producer_after_restart() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let (current_identity, current_claim) = startup(2, SECOND_SERVICE_ID);
    let stale_body = json!({"events": [local_event("evt-stale", FIRST_SERVICE_ID)]}).to_string();
    let (stale_status, _) =
        route_with_startup(&store, &current_identity, &current_claim, &stale_body);
    assert_eq!(stale_status, 201);

    let current_body =
        json!({"events": [local_event("evt-current", SECOND_SERVICE_ID)]}).to_string();
    let (current_status, response) =
        route_with_startup(&store, &current_identity, &current_claim, &current_body);
    assert_eq!(current_status, 201, "{response}");
    assert_eq!(
        store
            .db()
            .lock()
            .query_row("SELECT COUNT(*) FROM context_event_log", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        2
    );
}

#[test]
fn telemetry_endpoint_rejects_a_stale_startup_claim() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let (current_identity, _) = startup(2, SECOND_SERVICE_ID);
    let (_, stale_claim) = startup(1, FIRST_SERVICE_ID);
    let body = json!({"events": [local_event("evt-stale-lease", FIRST_SERVICE_ID)]}).to_string();
    let (status, _) = route_with_startup(&store, &current_identity, &stale_claim, &body);
    assert_eq!(status, 503);
    assert_eq!(
        store
            .db()
            .lock()
            .query_row("SELECT COUNT(*) FROM context_event_log", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

fn operation_store() -> MemoryStore {
    MemoryStore::open_with_operation_attribution(
        MemDb::open_in_memory(),
        OperationAttribution::new(
            "018f0d1e-9a1f-4bb8-8da7-52f152923102",
            "018f0d1e-9a1f-4bb8-8da7-52f152923103",
            "ws.0123456789abcdef0123456789abcdef",
        )
        .unwrap(),
    )
}

#[test]
fn manual_put_get_delete_dual_write_complete_canonical_attribution() {
    use memright::store::MemoryEventContext;
    use memright_core::MemoryTier;

    let store = operation_store();
    let context = MemoryEventContext::new("codex")
        .with_session("session-manual-1")
        .with_trace("trace-manual-1");
    let id = store
        .try_put_observed(
            "canonical-lifecycle",
            "Body that must never enter telemetry.",
            "D--Claude",
            MemoryTier::Semantic,
            &context,
        )
        .unwrap();
    store
        .try_put_observed(
            "canonical-lifecycle",
            "Updated body that also stays out of telemetry.",
            "D--Claude",
            MemoryTier::Semantic,
            &context,
        )
        .unwrap();
    store
        .record_injections_observed(std::slice::from_ref(&id), &context)
        .unwrap();
    store.get_full_observed(&id, &context).unwrap();
    assert!(store.try_delete_observed(&id, &context).unwrap());

    let conn = store.db().lock();
    let rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    )> = conn
        .prepare(
            "SELECT installation_id, service_instance_id, workspace_id, client, session_id,
                    trace_id, artifact_family, producer, provider, operation
               FROM context_event_log ORDER BY seq",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(rows.len(), 25);
    for (operation, expected) in [
        ("write", 7),
        ("update", 7),
        ("read", 4),
        ("delete", 5),
        ("evaluate", 2),
    ] {
        assert_eq!(
            rows.iter().filter(|row| row.9 == operation).count(),
            expected,
            "unexpected {operation} lifecycle cardinality"
        );
    }
    let opaque_session = rows[0].4.clone();
    let opaque_trace = rows[0].5.clone();
    assert!(opaque_session.starts_with("session-"));
    assert_eq!(opaque_session.len(), "session-".len() + 32);
    assert_ne!(opaque_session, "session-manual-1");
    assert!(opaque_trace.starts_with("trace-"));
    assert_eq!(opaque_trace.len(), "trace-".len() + 32);
    assert_ne!(opaque_trace, "trace-manual-1");
    for row in rows {
        assert_eq!(row.0, "018f0d1e-9a1f-4bb8-8da7-52f152923102");
        assert_eq!(row.1, "018f0d1e-9a1f-4bb8-8da7-52f152923103");
        assert_eq!(row.2, "ws.0123456789abcdef0123456789abcdef");
        assert_eq!(row.3, "codex");
        assert_eq!(row.4, opaque_session);
        assert_eq!(row.5, opaque_trace);
        assert_eq!(row.6, "memory");
        assert_eq!(row.7, "manual");
        assert_eq!(row.8, "memright");
    }
    let leaked: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM context_event_log WHERE meta LIKE '%Body that%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(leaked, 0);
    let delivered: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM context_event_log WHERE phase='block.delivered'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(delivered, 1);
}

#[test]
fn memory_batch_dual_write_preserves_each_items_family_producer_and_session() {
    use memright::store::{MemoryBatchItem, MemoryBatchRequest};

    let store = operation_store();
    let request = MemoryBatchRequest {
        batch_id: "batch-canonical-1".into(),
        items: vec![MemoryBatchItem {
            item_id: "item-canonical-1".into(),
            name: "batch-memory".into(),
            content: "Content-free telemetry keeps this body out.".into(),
            scope: "D--Claude".into(),
            tier: "Semantic".into(),
            artifact_family: "skill_output".into(),
            producer: "architect".into(),
            record_type: "architect_concept".into(),
            client: "claude".into(),
            session_id: "adapt-job-7".into(),
            turn_id: "adapt-turn-7".into(),
            trace_id: "adapt-trace-7".into(),
            source_ids: vec!["source-event-1".into()],
        }],
    };
    store.try_put_batch(&request).unwrap();
    store
        .try_put_observed(
            "batch-memory",
            "Updated without changing its artifact family.",
            "D--Claude",
            memright_core::MemoryTier::Semantic,
            &memright::store::MemoryEventContext::new("claude")
                .with_session("adapt-job-7")
                .with_trace("adapt-trace-7"),
        )
        .unwrap();

    let context = memright::store::MemoryEventContext::new("claude")
        .with_session("adapt-job-7")
        .with_trace("adapt-trace-7");
    store
        .get_full_observed("D--Claude/batch-memory", &context)
        .unwrap();
    assert!(store
        .try_delete_observed("D--Claude/batch-memory", &context)
        .unwrap());

    let conn = store.db().lock();
    let row: (String, String, String, String, String, String, String) = conn
        .query_row(
            "SELECT artifact_family, producer, client, session_id, trace_id, operation, artifact_id
               FROM context_event_log ORDER BY seq LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(&row.0, "skill_output");
    assert_eq!(&row.1, "architect");
    assert_eq!(&row.2, "claude");
    assert!(row.3.starts_with("session-") && row.3.len() == 40);
    assert_ne!(&row.3, "adapt-job-7");
    assert!(row.4.starts_with("trace-") && row.4.len() == 38);
    assert_ne!(&row.4, "adapt-trace-7");
    assert_eq!(&row.5, "write");
    assert!(row.6.starts_with("memory."));
    assert!(!row.6.contains('/'));
    let collapsed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM context_event_log WHERE artifact_family != 'skill_output'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        collapsed, 0,
        "family must survive delete feedback after row removal"
    );
}

#[test]
fn canonical_telemetry_failure_rolls_back_legacy_event_and_memory_mutation() {
    use memright::store::MemoryEventContext;
    use memright_core::MemoryTier;

    let store = operation_store();
    store
        .db()
        .lock()
        .execute_batch(
            "CREATE TRIGGER reject_operation_telemetry
             BEFORE INSERT ON context_event_log
             BEGIN SELECT RAISE(ABORT, 'forced canonical telemetry failure'); END;",
        )
        .unwrap();
    let result = store.try_put_observed(
        "must-rollback",
        "Mutation and telemetry form one commit.",
        "D--Claude",
        MemoryTier::Semantic,
        &MemoryEventContext::new("codex").with_session("session-rollback"),
    );
    assert!(result.is_err());

    let conn = store.db().lock();
    for table in ["memories", "memory_event_log", "context_event_log"] {
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "{table} must roll back with canonical telemetry");
    }
}

#[test]
fn feedback_write_emits_content_free_canonical_event() {
    use memright::feedback::{FeedbackRecord, FeedbackSource};
    use memright_core::Outcome;

    let store = operation_store();
    store
        .record_feedback(&FeedbackRecord {
            trace_id: "trace-feedback-1".into(),
            candidate_id: "D--Claude/candidate-one".into(),
            content_sha256: "a".repeat(64),
            outcome: Outcome::Contradicted,
            source: FeedbackSource::ObservedAction,
            verdict_ref: None,
            scope_id: "D--Claude".into(),
        })
        .unwrap();

    let conn = store.db().lock();
    let rows: Vec<(String, String, String, String, String, String)> = conn
        .prepare(
            "SELECT phase, operation, status, trace_id, artifact_sha256, provider
               FROM context_event_log ORDER BY seq",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        vec!["feedback.recorded", "candidate.contradicted"]
    );
    assert!(rows.iter().all(|row| row.1 == "evaluate"));
    assert_eq!(rows[1].2, "contradicted");
    assert!(rows
        .iter()
        .all(|row| row.3.starts_with("trace-") && row.3.len() == 38));
    assert!(rows.iter().all(|row| row.3 != "trace-feedback-1"));
    assert!(rows.iter().all(|row| row.4 == "a".repeat(64)));
    assert!(rows.iter().all(|row| row.5 == "memright"));
    let outcome_links: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM context_event_link WHERE relation='outcome_for'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        outcome_links, 0,
        "no retrieval receipt existed for this fixture"
    );
}
