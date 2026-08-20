use cortex::{MemDb, MemoryStore};
use serde_json::{json, Value};

fn lifecycle_rows(store: &MemoryStore) -> Vec<(String, String, String, String, Option<String>)> {
    store
        .db()
        .lock()
        .prepare(
            "SELECT session_id, trace_id, phase, status, reason_code
               FROM context_event_log ORDER BY seq",
        )
        .and_then(|mut statement| {
            statement
                .query_map([], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                })
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        })
        .unwrap()
}

fn canonical_correlation(value: &str, prefix: &str) -> bool {
    let bytes = value.as_bytes();
    let uuid = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    let opaque = value
        .strip_prefix(&format!("{prefix}-"))
        .is_some_and(|digest| {
            digest.len() == 32
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        });
    uuid || opaque
}

fn item(item_id: &str, name: &str, content: &str) -> Value {
    json!({
        "item_id": item_id,
        "name": name,
        "content": content,
        "scope": "D--Claude",
        "tier": "Semantic",
        "artifact_family": "skill_output",
        "producer": "architect",
        "record_type": "architect_concept",
        "client": "codex",
        "session_id": "architect-job-1",
        "trace_id": "architect-trace-1"
    })
}

fn request(batch_id: &str, items: Vec<Value>) -> String {
    json!({"batch_id": batch_id, "items": items}).to_string()
}

#[test]
fn memories_batch_commits_every_item_and_returns_truthful_receipts() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let body = request(
        "architect-report-1",
        vec![
            {
                let mut value = item(
                    "concept-1",
                    "plan--identity",
                    "Opaque installation identity.",
                );
                value.as_object_mut().unwrap().insert(
                    "source_ids".into(),
                    json!([
                        "install:one:codex:session-a",
                        "install:two:claude:session-b"
                    ]),
                );
                value
            },
            item("concept-2", "plan--ledger", "Content-free event ledger."),
        ],
    );

    let (status, payload) =
        cortex::serve::route_for_tests(&store, "POST", "/v1/memories:batch", &body);
    assert_eq!(status, 201, "{payload}");
    let receipt: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(receipt["batch_id"], "architect-report-1");
    assert_eq!(receipt["inserted"], 2);
    assert_eq!(receipt["duplicates"], 0);
    assert_eq!(receipt["complete"], true);
    assert_eq!(receipt["receipts"].as_array().unwrap().len(), 2);

    let conn = store.db().lock();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    let dimensions: (String, String, String) = conn
        .query_row(
            "SELECT artifact_family, producer, record_type FROM memories WHERE id='D--Claude/plan--identity'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        dimensions,
        (
            "skill_output".into(),
            "architect".into(),
            "architect_concept".into()
        )
    );
    let source_ids: String = conn
        .query_row(
            "SELECT source_ids FROM memories WHERE id='D--Claude/plan--identity'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&source_ids).unwrap(),
        vec![
            "install:one:codex:session-a".to_string(),
            "install:two:claude:session-b".to_string(),
        ]
    );
}

#[test]
fn identical_retry_is_noop_and_conflicting_batch_id_is_atomic_409() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let body = request(
        "architect-report-2",
        vec![
            item("concept-1", "plan--one", "First concept."),
            item("concept-2", "plan--two", "Second concept."),
        ],
    );
    assert_eq!(
        cortex::serve::route_for_tests(&store, "POST", "/v1/memories:batch", &body).0,
        201
    );

    let (retry_status, retry_payload) =
        cortex::serve::route_for_tests(&store, "POST", "/v1/memories:batch", &body);
    assert_eq!(retry_status, 200, "{retry_payload}");
    let retry: Value = serde_json::from_str(&retry_payload).unwrap();
    assert_eq!(retry["inserted"], 0);
    assert_eq!(retry["duplicates"], 2);

    let changed = request(
        "architect-report-2",
        vec![
            item("concept-1", "plan--one", "Changed concept."),
            item("concept-3", "plan--three", "Must not partially commit."),
        ],
    );
    let (conflict_status, conflict_payload) =
        cortex::serve::route_for_tests(&store, "POST", "/v1/memories:batch", &changed);
    assert_eq!(conflict_status, 409, "{conflict_payload}");
    let conn = store.db().lock();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        conn.query_row(
            "SELECT content FROM memories WHERE id='D--Claude/plan--one'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "First concept."
    );
}

#[test]
fn generic_put_persists_validated_write_attribution_and_rejects_family_typos() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let body = json!({
        "name": "adapt-rule",
        "content": "Prefer deterministic context accounting.",
        "scope": "D--Claude",
        "tier": "Semantic",
        "client": "codex",
        "artifactFamily": "adapt",
        "producer": "adapt",
        "recordType": "preference"
    })
    .to_string();
    let (status, payload) = cortex::serve::route_for_tests(&store, "POST", "/put", &body);
    assert_eq!(status, 200, "{payload}");

    let dimensions: (String, String, String) = store.db().lock().query_row(
        "SELECT artifact_family, producer, record_type FROM memories WHERE id='D--Claude/adapt-rule'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    assert_eq!(
        dimensions,
        ("adapt".into(), "adapt".into(), "preference".into())
    );

    let event_dimensions: (String, String) = store.db().lock().query_row(
        "SELECT artifact_family, producer FROM context_event_log WHERE artifact_id IS NOT NULL ORDER BY seq DESC LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(event_dimensions, ("adapt".into(), "adapt".into()));

    let typo = json!({
        "name": "bad-rule",
        "content": "Must not persist.",
        "scope": "D--Claude",
        "artifactFamily": "adpat",
        "producer": "adapt",
        "recordType": "preference"
    })
    .to_string();
    assert_eq!(
        cortex::serve::route_for_tests(&store, "POST", "/put", &typo).0,
        400
    );
    assert_eq!(
        store
            .db()
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE id='D--Claude/bad-rule'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn generic_put_round_trips_camel_case_lifecycle_input() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let body = json!({
        "name": "http-lifecycle",
        "content": "HTTP lifecycle metadata.",
        "scope": "D--Claude",
        "effectiveFromMs": 100,
        "effectiveUntilMs": 200,
        "expiresAtMs": 300,
        "reviewAfterMs": 150,
        "priorityClass": "protected",
        "confidence": 0.75,
        "confidenceBasis": "reviewed evidence",
    })
    .to_string();
    assert_eq!(
        cortex::serve::route_for_tests(&store, "POST", "/put", &body).0,
        200
    );
    let row: (i64, i64, i64, i64, String, f64, String) = store.db().lock().query_row(
        "SELECT effective_from_ms, effective_until_ms, expires_at_ms, review_after_ms, priority_class, confidence, confidence_basis FROM memories WHERE id='D--Claude/http-lifecycle'",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
    ).unwrap();
    assert_eq!(
        row,
        (
            100,
            200,
            300,
            150,
            "protected".into(),
            0.75,
            "reviewed evidence".into()
        )
    );
    let get = cortex::serve::route_for_tests(
        &store,
        "POST",
        "/get",
        r#"{"id":"D--Claude/http-lifecycle"}"#,
    );
    assert_eq!(get.0, 200);
    let body: Value = serde_json::from_str(&get.1).unwrap();
    assert_eq!(body["lifecycle"]["effectiveFromMs"], 100);
    assert_eq!(body["lifecycle"]["priorityClass"], "protected");
    assert_eq!(body["lifecycle"]["confidenceBasis"], "reviewed evidence");
}

#[test]
fn batch_lifecycle_input_persists_and_omitted_update_preserves_values() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let mut first = item("lifecycle-1", "lifecycle-rule", "First lifecycle body.");
    let fields = first.as_object_mut().unwrap();
    fields.insert("effectiveFromMs".into(), json!(100));
    fields.insert("effectiveUntilMs".into(), json!(200));
    fields.insert("priorityClass".into(), json!("protected"));
    fields.insert("confidence".into(), json!(0.8));
    fields.insert("confidenceBasis".into(), json!("reviewed evidence"));
    assert_eq!(
        cortex::serve::route_for_tests(
            &store,
            "POST",
            "/v1/memories:batch",
            &request("lifecycle-first", vec![first])
        )
        .0,
        201
    );

    let values: (i64, i64, String, f64, String) = store.db().lock().query_row(
        "SELECT effective_from_ms, effective_until_ms, priority_class, confidence, confidence_basis FROM memories WHERE id='D--Claude/lifecycle-rule'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    ).unwrap();
    assert_eq!(
        values,
        (
            100,
            200,
            "protected".into(),
            0.8,
            "reviewed evidence".into()
        )
    );

    assert_eq!(
        cortex::serve::route_for_tests(
            &store,
            "POST",
            "/v1/memories:batch",
            &request(
                "lifecycle-update",
                vec![item(
                    "lifecycle-2",
                    "lifecycle-rule",
                    "Updated lifecycle body."
                )]
            )
        )
        .0,
        201
    );
    let preserved: (i64, i64, String, f64, String) = store.db().lock().query_row(
        "SELECT effective_from_ms, effective_until_ms, priority_class, confidence, confidence_basis FROM memories WHERE id='D--Claude/lifecycle-rule'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    ).unwrap();
    assert_eq!(preserved, values);
}

#[test]
fn batch_lifecycle_supersession_transitions_old_row_in_same_transaction() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    assert_eq!(
        cortex::serve::route_for_tests(
            &store,
            "POST",
            "/v1/memories:batch",
            &request("old-row", vec![item("old", "old-rule", "Old rule.")])
        )
        .0,
        201
    );
    let mut replacement = item("replacement", "new-rule", "New rule.");
    replacement
        .as_object_mut()
        .unwrap()
        .insert("supersedes".into(), json!("D--Claude/old-rule"));
    replacement
        .as_object_mut()
        .unwrap()
        .insert("effectiveFromMs".into(), json!(123));
    assert_eq!(
        cortex::serve::route_for_tests(
            &store,
            "POST",
            "/v1/memories:batch",
            &request("new-row", vec![replacement])
        )
        .0,
        201
    );
    let old: (String, String, i64) = store.db().lock().query_row(
        "SELECT lifecycle_state, superseded_by, effective_until_ms FROM memories WHERE id='D--Claude/old-rule'",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).unwrap();
    assert_eq!(old, ("superseded".into(), "D--Claude/new-rule".into(), 123));
    assert_eq!(
        store
            .db()
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM memory_event_log WHERE event_kind='superseded'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn lifecycle_route_rejects_bad_confidence_and_priority_without_writes() {
    for body in [
        r#"{"name":"bad-confidence","content":"body","scope":"D--Claude","confidence":1.1}"#,
        r#"{"name":"bad-priority","content":"body","scope":"D--Claude","priorityClass":"pinned"}"#,
    ] {
        let store = MemoryStore::open(MemDb::open_in_memory());
        let response = cortex::serve::route_for_tests(&store, "POST", "/put", body);
        assert_eq!(response.0, 400, "{}", response.1);
        assert_eq!(
            store
                .db()
                .lock()
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn batch_supersession_log_failure_rolls_back_new_and_old_rows() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    assert_eq!(
        cortex::serve::route_for_tests(
            &store,
            "POST",
            "/v1/memories:batch",
            &request("rollback-old", vec![item("old", "old-rule", "Old rule.")])
        )
        .0,
        201
    );
    store.db().lock().execute_batch(
        "CREATE TRIGGER reject_supersession_event BEFORE INSERT ON memory_event_log
         WHEN NEW.event_kind='superseded' BEGIN SELECT RAISE(ABORT, 'forced supersession event failure'); END;",
    ).unwrap();
    let mut replacement = item("replacement", "new-rule", "New rule.");
    replacement
        .as_object_mut()
        .unwrap()
        .insert("supersedes".into(), json!("D--Claude/old-rule"));
    let response = cortex::serve::route_for_tests(
        &store,
        "POST",
        "/v1/memories:batch",
        &request("rollback-new", vec![replacement]),
    );
    assert_eq!(response.0, 500, "{}", response.1);
    let old: (String, Option<String>) = store
        .db()
        .lock()
        .query_row(
            "SELECT lifecycle_state, superseded_by FROM memories WHERE id='D--Claude/old-rule'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(old, ("active".into(), None));
    assert_eq!(
        store
            .db()
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE id='D--Claude/new-rule'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

#[test]
fn invalid_middle_item_rejects_whole_batch_before_embedding_or_commit() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let mut invalid = item("concept-2", "plan--two", "Second concept.");
    invalid.as_object_mut().unwrap().remove("session_id");
    let body = request(
        "architect-report-invalid",
        vec![item("concept-1", "plan--one", "First concept."), invalid],
    );

    let (status, payload) =
        cortex::serve::route_for_tests(&store, "POST", "/v1/memories:batch", &body);
    assert_eq!(status, 400, "{payload}");
    assert_eq!(
        store
            .db()
            .lock()
            .query_row("SELECT COUNT(*) FROM memories", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn http_put_get_list_delete_emit_complete_linked_lifecycles_with_opaque_identity() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let attribution =
        r#""client":"codex","session":"background sync job","trace_id":"unknown trace""#;
    let put =
        format!(r#"{{"name":"edge","content":"Durable body.","scope":"global",{attribution}}}"#);
    assert_eq!(
        cortex::serve::route_for_tests(&store, "POST", "/put", &put).0,
        200
    );
    let get = format!(r#"{{"id":"global/edge",{attribution}}}"#);
    assert_eq!(
        cortex::serve::route_for_tests(&store, "POST", "/get", &get).0,
        200
    );
    let list = format!(r#"{{"scope":"global",{attribution}}}"#);
    assert_eq!(
        cortex::serve::route_for_tests(&store, "POST", "/list", &list).0,
        200
    );
    let delete = format!(r#"{{"id":"global/edge",{attribution}}}"#);
    assert_eq!(
        cortex::serve::route_for_tests(&store, "POST", "/delete", &delete).0,
        200
    );

    let rows = lifecycle_rows(&store);
    assert!(rows
        .iter()
        .all(|row| canonical_correlation(&row.0, "session")));
    assert!(rows
        .iter()
        .all(|row| canonical_correlation(&row.1, "trace")));
    for phase in [
        "write.requested",
        "validation.started",
        "validation.terminal",
        "embedding.started",
        "embedding.terminal",
        "commit.started",
        "commit.terminal",
        "provider.started",
        "candidate.retrieved",
        "provider.terminal",
    ] {
        assert!(rows.iter().any(|row| row.2 == phase), "missing {phase}");
    }
    let linked_terminal_count: i64 = store
        .db()
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM context_event_link WHERE relation='terminal_of'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(linked_terminal_count >= 6);
}

#[test]
fn http_validation_missing_targets_and_empty_lists_close_with_typed_terminals() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let cases = [
        ("/put", "{", 400),
        (
            "/put",
            r#"{"name":"bad","content":"body","artifactFamily":"adpat"}"#,
            400,
        ),
        ("/get", r#"{"id":"global/missing"}"#, 404),
        ("/delete", r#"{"id":"global/missing"}"#, 404),
        ("/list", r#"{"scope":"global"}"#, 200),
    ];
    for (path, body, expected) in cases {
        assert_eq!(
            cortex::serve::route_for_tests(&store, "POST", path, body).0,
            expected,
            "{path}"
        );
    }
    let rows = lifecycle_rows(&store);
    assert!(rows.iter().any(|row| {
        row.2 == "validation.terminal"
            && row.3 == "failed"
            && row.4.as_deref() == Some("malformed_json")
    }));
    assert!(rows.iter().any(|row| {
        row.2 == "validation.terminal"
            && row.3 == "failed"
            && row.4.as_deref() == Some("invalid_attribution")
    }));
    assert!(rows.iter().any(|row| {
        row.2 == "provider.terminal"
            && row.3 == "empty"
            && row.4.as_deref() == Some("target_not_found")
    }));
    assert!(rows.iter().any(|row| {
        row.2 == "commit.terminal"
            && row.3 == "empty"
            && row.4.as_deref() == Some("target_not_found")
    }));
    assert!(rows.iter().any(|row| {
        row.2 == "provider.terminal" && row.3 == "empty" && row.4.as_deref() == Some("no_results")
    }));
}

#[test]
fn memories_batch_accounts_success_duplicate_conflict_and_invalid_envelope() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let body = request(
        "accounted-batch",
        vec![item("one", "one", "First."), item("two", "two", "Second.")],
    );
    assert_eq!(
        cortex::serve::route_for_tests(&store, "POST", "/v1/memories:batch", &body).0,
        201
    );
    assert_eq!(
        cortex::serve::route_for_tests(&store, "POST", "/v1/memories:batch", &body).0,
        200
    );
    let conflict = request("accounted-batch", vec![item("one", "one", "Changed.")]);
    assert_eq!(
        cortex::serve::route_for_tests(&store, "POST", "/v1/memories:batch", &conflict).0,
        409
    );
    assert_eq!(
        cortex::serve::route_for_tests(&store, "POST", "/v1/memories:batch", "{").0,
        400
    );

    let rows = lifecycle_rows(&store);
    assert!(rows.iter().any(|row| {
        row.2 == "commit.terminal"
            && row.3 == "skipped"
            && row.4.as_deref() == Some("idempotent_duplicate")
    }));
    assert!(rows.iter().any(|row| {
        row.2 == "validation.terminal"
            && row.3 == "failed"
            && row.4.as_deref() == Some("batch_conflict")
    }));
    assert!(rows.iter().any(|row| {
        row.2 == "validation.terminal"
            && row.3 == "failed"
            && row.4.as_deref() == Some("malformed_json")
    }));
}

#[test]
fn http_put_commit_failure_rolls_back_memory_and_records_failed_terminal() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    store
        .db()
        .lock()
        .execute_batch(
            "CREATE TRIGGER fail_external_put BEFORE INSERT ON memories
             BEGIN SELECT RAISE(ABORT, 'forced external commit failure'); END;",
        )
        .unwrap();
    let (status, _) = cortex::serve::route_for_tests(
        &store,
        "POST",
        "/put",
        r#"{"name":"must-rollback","content":"body","scope":"global"}"#,
    );
    assert_eq!(status, 500);
    let conn = store.db().lock();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM context_event_log
              WHERE phase='commit.terminal' AND status='failed'
                AND reason_code='commit_failed'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
}

#[test]
fn http_get_provider_failure_is_not_reported_as_an_empty_result() {
    let store = MemoryStore::new();
    let entry = store.remember("provider failure target", vec![]);
    store
        .db()
        .lock()
        .execute_batch(
            "CREATE TRIGGER fail_external_get BEFORE UPDATE ON memories
             BEGIN SELECT RAISE(ABORT, 'forced external provider failure'); END;",
        )
        .unwrap();

    let (status, _) = cortex::serve::route_for_tests(
        &store,
        "POST",
        "/get",
        &serde_json::json!({ "id": entry.id }).to_string(),
    );
    assert_eq!(status, 500);

    let conn = store.db().lock();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM context_event_log
              WHERE phase='provider.terminal' AND status='failed'
                AND reason_code='provider_failed'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM context_event_log
              WHERE phase='provider.terminal' AND status='empty'
                AND reason_code='target_not_found'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
}

#[test]
fn http_list_provider_failure_is_not_reported_as_an_empty_result() {
    let store = MemoryStore::new();
    store
        .db()
        .lock()
        .execute_batch("DROP TABLE memories;")
        .unwrap();

    let (status, _) =
        cortex::serve::route_for_tests(&store, "POST", "/list", r#"{"scope":"global"}"#);
    assert_eq!(status, 500);

    let conn = store.db().lock();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM context_event_log
              WHERE phase='provider.terminal' AND status='failed'
                AND reason_code='provider_failed'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM context_event_log
              WHERE phase='provider.terminal' AND status='empty'
                AND reason_code='no_results'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
}
