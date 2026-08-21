use cortex_core::MemoryTier;
use membrane_runtime::{
    MemDb, MemoryEventContext, MemoryLifecycleEventV1, MemoryLifecycleInputV1, MemoryPriorityError,
    MemoryStore,
};

fn insert_memory(store: &MemoryStore, id: &str, scope: &str) {
    store
        .db()
        .lock()
        .execute(
            "INSERT INTO memories (id, tier, content, keywords, score, created_at, updated_at, access_count, scope_id)\
             VALUES (?1, 'Semantic', 'fixture', 'fixture', 0.5, '2026-07-27T00:00:00Z', '2026-07-27T00:00:00Z', 0, ?2)",
            rusqlite::params![id, scope],
        )
        .unwrap();
}

fn supersession(
    event_id: &str,
    subject_id: &str,
    replacement_id: &str,
    scope_id: &str,
) -> MemoryLifecycleEventV1 {
    MemoryLifecycleEventV1::superseded(
        event_id,
        subject_id,
        replacement_id,
        scope_id,
        1_722_000_000_000,
        "human",
        "A1",
        "review:fixture",
        "origin:fixture",
    )
}

#[test]
fn supersession_is_transactional_and_idempotently_logged() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    insert_memory(&store, "scope/old", "scope");
    insert_memory(&store, "scope/new", "scope");
    let event = supersession("lifecycle:one", "scope/old", "scope/new", "scope");

    store.apply_lifecycle_event(&event).unwrap();
    store.apply_lifecycle_event(&event).unwrap();

    let conn = store.db().lock();
    let row: (String, Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT lifecycle_state, superseded_by, effective_until_ms FROM memories WHERE id='scope/old'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(row.0, "superseded");
    assert_eq!(row.1.as_deref(), Some("scope/new"));
    assert_eq!(row.2, Some(1_722_000_000_000));
    let event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_event_log WHERE event_uid='lifecycle:one'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(event_count, 1);
}

#[test]
fn supersession_rejects_cross_scope_without_mutating_subject() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    insert_memory(&store, "one/old", "one");
    insert_memory(&store, "two/new", "two");

    let error = store
        .apply_lifecycle_event(&supersession(
            "lifecycle:cross",
            "one/old",
            "two/new",
            "one",
        ))
        .unwrap_err();
    assert!(error.to_string().contains("same scope"));
    let state: (String, Option<String>) = store
        .db()
        .lock()
        .query_row(
            "SELECT lifecycle_state, superseded_by FROM memories WHERE id='one/old'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(state.0, "active");
    assert_eq!(state.1, None);
}

#[test]
fn supersession_rejects_cycles() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    insert_memory(&store, "scope/old", "scope");
    insert_memory(&store, "scope/new", "scope");
    store
        .apply_lifecycle_event(&supersession(
            "lifecycle:forward",
            "scope/old",
            "scope/new",
            "scope",
        ))
        .unwrap();

    let error = store
        .apply_lifecycle_event(&supersession(
            "lifecycle:back",
            "scope/new",
            "scope/old",
            "scope",
        ))
        .unwrap_err();
    assert!(error.to_string().contains("cycle"));
    let state: (String, Option<String>) = store
        .db()
        .lock()
        .query_row(
            "SELECT lifecycle_state, superseded_by FROM memories WHERE id='scope/new'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(state.0, "active");
    assert_eq!(state.1, None);
}

#[test]
fn ordinary_puts_keep_the_schema_default_active_recallable_lifecycle() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let id = store
        .try_put(
            "ordinary",
            "ordinary recall fixture",
            "scope",
            MemoryTier::Semantic,
        )
        .unwrap();

    let lifecycle: (String, String, Option<i64>, Option<i64>) = store
        .db()
        .lock()
        .query_row(
            "SELECT authority, lifecycle_state, effective_from_ms, effective_until_ms
             FROM memories WHERE id=?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(lifecycle.0, "A2");
    assert_eq!(lifecycle.1, "active");
    assert_eq!(lifecycle.2, None);
    assert_eq!(lifecycle.3, None);
    assert!(store.recall_eligible_ids_at(i64::MAX, false).contains(&id));
}

#[test]
fn supersession_replay_respects_event_time_for_as_of_recall() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    insert_memory(&store, "scope/old", "scope");
    insert_memory(&store, "scope/new", "scope");
    store
        .db()
        .lock()
        .execute(
            "UPDATE memories SET effective_from_ms=1722000000000 WHERE id='scope/new'",
            [],
        )
        .unwrap();
    store
        .apply_lifecycle_event(&supersession(
            "lifecycle:as-of",
            "scope/old",
            "scope/new",
            "scope",
        ))
        .unwrap();

    let before = store.recall_eligible_ids_at(1_721_999_999_999, false);
    let after = store.recall_eligible_ids_at(1_722_000_000_001, false);
    assert!(before.contains("scope/old"));
    assert!(!before.contains("scope/new"));
    assert!(!after.contains("scope/old"));
    assert!(after.contains("scope/new"));
}

#[test]
fn failed_lifecycle_event_preserves_subject_event_log_and_private_metadata() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    insert_memory(&store, "scope/old", "scope");
    insert_memory(&store, "scope/new", "scope");
    store
        .db()
        .lock()
        .execute(
            "CREATE TRIGGER reject_lifecycle_log BEFORE INSERT ON memory_event_log
             WHEN NEW.event_uid='lifecycle:privacy-rollback'
             BEGIN SELECT RAISE(ABORT, 'forced lifecycle log failure'); END",
            [],
        )
        .unwrap();

    let error = store
        .apply_lifecycle_event(&supersession(
            "lifecycle:privacy-rollback",
            "scope/old",
            "scope/new",
            "scope",
        ))
        .unwrap_err();
    assert!(error.to_string().contains("forced lifecycle log failure"));

    let conn = store.db().lock();
    let state: (String, Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT lifecycle_state, superseded_by, effective_until_ms
             FROM memories WHERE id='scope/old'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(state, ("active".into(), None, None));
    let log_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_event_log WHERE event_uid='lifecycle:privacy-rollback'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(log_count, 0);
}

#[test]
fn priority_protection_requires_authority_and_emits_one_audit_event() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    insert_memory(&store, "scope/pinned", "scope");

    let error = store
        .set_priority_class("scope/pinned", "protected", "agent", "A4", "pin:fixture")
        .unwrap_err();
    assert!(matches!(error, MemoryPriorityError::Unauthorized(_, _)));

    store
        .set_priority_class("scope/pinned", "protected", "human", "A1", "pin:fixture")
        .unwrap();
    let conn = store.db().lock();
    let priority: String = conn
        .query_row(
            "SELECT priority_class FROM memories WHERE id='scope/pinned'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(priority, "protected");
    let events: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_event_log WHERE event_kind='priority_protected' AND memory_id='scope/pinned'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(events, 1);
}

#[test]
fn lifecycle_authoring_rejects_invalid_fields_and_protected_rows_stay_gated() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let context = MemoryEventContext::new("test");
    let invalid = MemoryLifecycleInputV1 {
        effective_from_ms: Some(20),
        effective_until_ms: Some(20),
        ..Default::default()
    };
    assert!(store
        .try_put_attributed_lifecycle_observed(
            "invalid",
            "body",
            "scope",
            MemoryTier::Semantic,
            "memory",
            "test",
            "memory",
            &context,
            &invalid
        )
        .unwrap_err()
        .contains("precede"));

    let protected_expired = MemoryLifecycleInputV1 {
        expires_at_ms: Some(0),
        priority_class: Some("protected".into()),
        ..Default::default()
    };
    let id = store
        .try_put_attributed_lifecycle_observed(
            "protected",
            "expired body",
            "scope",
            MemoryTier::Semantic,
            "memory",
            "test",
            "memory",
            &context,
            &protected_expired,
        )
        .unwrap();
    assert!(!store.recall_eligible_ids_at(1, false).contains(&id));
}

#[test]
fn lifecycle_curation_quarantines_expired_and_excludes_other_gated_states() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let context = MemoryEventContext::new("test");
    let expired = store
        .try_put_attributed_lifecycle_observed(
            "expired",
            "expired source",
            "scope",
            MemoryTier::Semantic,
            "memory",
            "test",
            "memory",
            &context,
            &MemoryLifecycleInputV1 {
                expires_at_ms: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
    for name in ["future", "superseded", "draft", "retired", "invalidated"] {
        store
            .try_put(
                name,
                &format!("{name} source"),
                "scope",
                MemoryTier::Semantic,
            )
            .unwrap();
    }
    store.db().lock().execute_batch(
        "UPDATE memories SET effective_from_ms=9223372036854775807 WHERE id='scope/future';
         UPDATE memories SET lifecycle_state='superseded', superseded_by='scope/future', effective_until_ms=0 WHERE id='scope/superseded';
         UPDATE memories SET lifecycle_state='draft' WHERE id='scope/draft';
         UPDATE memories SET lifecycle_state='retired' WHERE id='scope/retired';
         UPDATE memories SET lifecycle_state='invalidated' WHERE id='scope/invalidated';",
    ).unwrap();

    let status = store.dream_now("2026-07-28").unwrap();
    assert_eq!(status.read_count, 0);
    assert_eq!(status.quarantined_count, 1);
    assert_eq!(store.quarantined_ids(), vec![expired]);
    let remaining: i64 = store
        .db()
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM memories WHERE id='scope/future'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 1);
}

#[test]
fn lifecycle_metrics_are_content_free_and_count_gated_rows() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    for (id, state) in [
        ("active", "active"),
        ("superseded", "superseded"),
        ("retired", "retired"),
        ("invalidated", "invalidated"),
        ("draft", "draft"),
    ] {
        insert_memory(&store, &format!("scope/{id}"), "scope");
        store
            .db()
            .lock()
            .execute(
                "UPDATE memories SET lifecycle_state=?1 WHERE id=?2",
                rusqlite::params![state, format!("scope/{id}")],
            )
            .unwrap();
    }
    store
        .db()
        .lock()
        .execute(
            "UPDATE memories SET effective_from_ms=?1 WHERE id='scope/active'",
            [i64::MAX],
        )
        .unwrap();
    let metrics = store.metrics_json();
    assert_eq!(metrics["lifecycle"]["total"], 5);
    assert_eq!(metrics["lifecycle"]["future"], 1);
    assert_eq!(metrics["lifecycle"]["superseded"], 1);
    assert_eq!(metrics["lifecycle"]["retired"], 1);
    assert_eq!(metrics["lifecycle"]["invalidated"], 1);
    assert_eq!(metrics["lifecycle"]["draft"], 1);
    assert_eq!(metrics["lifecycle"]["gated_out"], 5);
    assert!(metrics.to_string().contains("gated_out"));
    assert!(!metrics.to_string().contains("fixture"));
}
