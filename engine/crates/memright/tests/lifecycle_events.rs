use memright::{MemDb, MemoryLifecycleEventV1, MemoryPriorityError, MemoryStore};

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
