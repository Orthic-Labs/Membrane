//! M8 acceptance for durable absorbed records.
//!
//! These tests are intentionally kept as an integration-owned fixture. They
//! exercise the same cursor, import, tombstone, and range contracts used by
//! the resident store without changing storage implementation code.

use cortex_store::{
    AbsorbedStore, AppendOutcome, Fts5Document, Fts5Projection, MemDb, ProjectionState,
    SessionEvent, ABSORBED_SCHEMA_VERSION,
};
use cortex_core::{LexicalHit, MemoryEntry, MemoryRegistry, MemoryRetriever, MemoryTier};
use serde_json::json;
use std::time::Instant;

fn event(session_id: &str, seq: u64, id: &str) -> SessionEvent {
    SessionEvent {
        schema_version: ABSORBED_SCHEMA_VERSION,
        session_id: session_id.into(),
        seq,
        event_id: id.into(),
        event_type: "message".into(),
        payload: json!({"seq": seq, "content": format!("event-{seq}")}),
        scope_id: "workspace".into(),
        authority: "A2".into(),
        influence_class: "reference".into(),
        lifecycle: "active".into(),
        retention: "session".into(),
        provenance: Vec::new(),
        content_hash: format!("sha256:event-{seq}"),
        occurred_at_ms: seq,
        recorded_at_ms: seq,
    }
}

#[test]
fn append_is_atomic_contiguous_and_idempotent() {
    let store = AbsorbedStore::new(MemDb::open_in_memory()).unwrap();
    assert!(matches!(store.append(&event("s1", 1, "e1")), Ok(AppendOutcome::Inserted(_))));
    assert!(matches!(store.append(&event("s1", 2, "e2")), Ok(AppendOutcome::Inserted(_))));
    assert!(matches!(store.append(&event("s1", 2, "e2")), Ok(AppendOutcome::AlreadyPresent(_))));
    assert_eq!(store.cursor("s1").unwrap().last_seq, 2);
    assert!(store.append(&event("s1", 4, "e4")).is_err());
    assert!(store.append(&event("s1", 1, "different")).is_err());
}

#[test]
fn range_resume_and_import_reject_gaps_reordering_and_reuse() {
    let db = MemDb::open_in_memory();
    let store = AbsorbedStore::new(db.clone()).unwrap();
    for seq in 1..=3 {
        store.append(&event("s1", seq, &format!("e{seq}"))).unwrap();
    }
    assert_eq!(store.range("s1", 2, 4).unwrap().iter().map(|e| e.seq).collect::<Vec<_>>(), vec![2, 3]);
    let resumed = AbsorbedStore::new(db).unwrap();
    assert_eq!(resumed.cursor("s1").unwrap().last_seq, 3);
    assert!(AbsorbedStore::validate_import(&[event("s2", 1, "x1"), event("s2", 3, "x3")]).is_err());
    assert!(AbsorbedStore::validate_import(&[event("s2", 2, "x2"), event("s2", 1, "x1")]).is_err());
    assert_eq!(
        store
            .import_events(&[event("s2", 1, "x1"), event("s2", 2, "x2")])
            .unwrap(),
        2
    );
    assert_eq!(store.cursor("s2").unwrap().last_seq, 2);
    let before_failed_import = store.cursor("s3").unwrap();
    assert!(store
        .import_events(&[event("s3", 1, "x1"), event("s3", 3, "x3")])
        .is_err());
    assert_eq!(store.cursor("s3").unwrap(), before_failed_import);
    assert!(store.range("s3", 1, 4).unwrap().is_empty());
    let fault_db = MemDb::open_in_memory();
    let fault_store = AbsorbedStore::new(fault_db.clone()).unwrap();
    fault_db
        .lock()
        .execute_batch(
            "CREATE TRIGGER absorbed_import_fault BEFORE INSERT ON absorbed_events
             WHEN NEW.seq=2 BEGIN SELECT RAISE(ABORT, 'injected import fault'); END;",
        )
        .unwrap();
    assert!(fault_store
        .import_events(&[event("fault", 1, "f1"), event("fault", 2, "f2")])
        .is_err());
    assert_eq!(fault_store.cursor("fault").unwrap().last_seq, 0);
    assert!(fault_store.range("fault", 1, 3).unwrap().is_empty());
    resumed.tombstone_event("s1", 2, 99).unwrap();
    assert!(resumed.append(&event("s1", 2, "reused")).is_err());
}

#[test]
fn persisted_event_cursor_and_range_survive_store_reopen() {
    let path = std::env::temp_dir().join(format!(
        "membrane-mem-i02-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let events_path = path.with_file_name(format!(
        "{}.membrane-events.sqlite3",
        path.file_stem().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&events_path);
    {
        let store = AbsorbedStore::new(MemDb::open(&path).unwrap()).unwrap();
        store.append(&event("restart", 1, "r1")).unwrap();
        store.append(&event("restart", 2, "r2")).unwrap();
    }
    let reopened = AbsorbedStore::new(MemDb::open(&path).unwrap()).unwrap();
    assert_eq!(reopened.cursor("restart").unwrap().last_seq, 2);
    assert_eq!(reopened.range("restart", 1, 3).unwrap().len(), 2);
    drop(reopened);
    std::fs::remove_file(&path).unwrap();
    let _ = std::fs::remove_file(&events_path);
}

#[test]
fn fts5_projection_preserves_lexical_control_and_governance_filters() {
    let db = MemDb::open_in_memory();
    let conn = db.lock();
    let projection = Fts5Projection::new(&conn);
    assert_eq!(projection.ensure_schema().unwrap(), ProjectionState::Rebuilt);
    projection
        .rebuild([
            Fts5Document {
                record_id: "m1".into(),
                record_type: "memory".into(),
                session_id: Some("s1".into()),
                scope_id: "workspace".into(),
                lifecycle: "active".into(),
                authority: "A2".into(),
                content: "rust async runtime".into(),
                keywords: "rust async".into(),
            },
            Fts5Document {
                record_id: "m2".into(),
                record_type: "memory".into(),
                session_id: Some("s1".into()),
                scope_id: "private".into(),
                lifecycle: "active".into(),
                authority: "A2".into(),
                content: "rust async private".into(),
                keywords: "rust async".into(),
            },
            Fts5Document {
                record_id: "m3".into(),
                record_type: "memory".into(),
                session_id: Some("s1".into()),
                scope_id: "workspace".into(),
                lifecycle: "deleted".into(),
                authority: "A2".into(),
                content: "rust async deleted".into(),
                keywords: "rust async".into(),
            },
        ])
        .unwrap();

    let hits = projection
        .search("rust async", Some("workspace"), Some("active"), Some("A2"), 10, 0)
        .unwrap();
    assert_eq!(hits.iter().map(|hit| hit.record_id.as_str()).collect::<Vec<_>>(), ["m1"]);
    assert!(hits[0].score.is_finite() && hits[0].score > 0.0);
    assert!(projection.search("rust OR async", None, None, None, 10, 0).is_ok());

    projection.delete("m1").unwrap();
    assert!(projection
        .search("rust async", Some("workspace"), Some("active"), Some("A2"), 10, 0)
        .unwrap()
        .is_empty());
}

#[test]
fn fts5_hybrid_matches_legacy_lexical_control_and_records_timing() {
    let db = MemDb::open_in_memory();
    let conn = db.lock();
    let projection = Fts5Projection::new(&conn);
    projection
        .rebuild([Fts5Document {
            record_id: "m1".into(),
            record_type: "memory".into(),
            session_id: Some("s1".into()),
            scope_id: "workspace".into(),
            lifecycle: "active".into(),
            authority: "A2".into(),
            content: "rust async runtime".into(),
            keywords: "rust async".into(),
        }])
        .unwrap();
    let fts_started = Instant::now();
    let fts_hits = projection.search("rust async", None, None, None, 10, 0).unwrap();
    let fts_ns = fts_started.elapsed().as_nanos();

    let mut registry = MemoryRegistry::new();
    registry.insert(MemoryEntry {
        id: "m1".into(),
        tier: MemoryTier::Working,
        content: "rust async runtime".into(),
        keywords: vec!["rust".into(), "async".into()],
        score: 0.0,
        created_at: "now".into(),
        access_count: 0,
        embedding: Some(vec![1.0, 0.0]),
        scope_id: "workspace".into(),
    });
    registry.insert(MemoryEntry {
        id: "m2".into(),
        tier: MemoryTier::Working,
        content: "deployment notes".into(),
        keywords: vec!["deployment".into()],
        score: 0.0,
        created_at: "now".into(),
        access_count: 0,
        embedding: Some(vec![0.0, 1.0]),
        scope_id: "workspace".into(),
    });
    let legacy_started = Instant::now();
    let legacy = MemoryRetriever::retrieve(&registry, "rust async", 1);
    let legacy_ns = legacy_started.elapsed().as_nanos();
    let lexical_hits = fts_hits
        .iter()
        .map(|hit| LexicalHit::new(hit.record_id.clone(), hit.score))
        .collect::<Vec<_>>();
    let hybrid = MemoryRetriever::retrieve_hybrid_with_lexical_hits(
        &registry,
        &lexical_hits,
        Some(&[1.0, 0.0]),
        1,
        Some(&["workspace"]),
    );
    eprintln!(
        "MEM-I02 hybrid measurement fts5Ns={fts_ns} legacyLexicalNs={legacy_ns} ftsHits={} legacyTop={} hybridTop={}",
        fts_hits.len(),
        legacy.first().map(|entry| entry.id.as_str()).unwrap_or("none"),
        hybrid.first().map(|entry| entry.id.as_str()).unwrap_or("none")
    );
    assert_eq!(fts_hits[0].record_id, "m1");
    assert_eq!(legacy[0].id, hybrid[0].id);
}
