//! M8 acceptance for deterministic transcript projection and Guide ledger.

use cortex_core::absorbed::SessionEvent;
use cortex_core::transcript::{
    build_transcript_chunks, retrieve_transcript_chunks, TranscriptChunk, TranscriptChunkConfig,
};
use cortex_store::{MemDb, TranscriptChunkRecord, TranscriptStore};
use membrane_runtime::guide::ledger::{
    build_session_ledger, index_session_ledger, LedgerDecisionV1, LedgerEventV1,
    LedgerSourceCursor, LedgerTaskV1, SessionLedgerInputV1,
};
use membrane_runtime::guide::GuideDb;
use serde_json::json;

fn event(seq: u64, content: &str) -> SessionEvent {
    event_for("s1", seq, content)
}

fn event_for(session_id: &str, seq: u64, content: &str) -> SessionEvent {
    SessionEvent {
        schema_version: 1,
        session_id: session_id.into(),
        seq,
        event_id: format!("e{seq}"),
        event_type: "message".into(),
        payload: json!({"role":"user","speaker":"adrian","content":content}),
        scope_id: "workspace".into(),
        authority: "A2".into(),
        influence_class: "reference".into(),
        lifecycle: "active".into(),
        retention: "session".into(),
        provenance: Vec::new(),
        occurred_at_ms: seq,
        recorded_at_ms: seq,
        content_hash: format!("sha256:{seq}"),
    }
}

#[test]
fn transcript_split_is_rebuildable_and_scope_safe() {
    let events = vec![event(1, "alpha"), event(2, "beta")];
    let chunks = build_transcript_chunks("s1", &events, TranscriptChunkConfig { max_chars: 6 }).unwrap();
    assert_eq!(chunks.iter().map(|chunk| chunk.seq_start).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(retrieve_transcript_chunks(&chunks, "beta", None, 1)[0].chunk.chunk_id, "s1:2:3");
    assert!(retrieve_transcript_chunks(&chunks, "alpha", Some("other"), 10).is_empty());
}

#[test]
fn ledger_retains_cursor_links_and_explicit_omissions() {
    let input = SessionLedgerInputV1 {
        session_id: "s1".into(),
        title: Some("Handoff".into()),
        source_cursor: LedgerSourceCursor { session_id: "s1".into(), last_seq: 3 },
        source_content_hash: "sha256:source".into(),
        events: vec![LedgerEventV1 { event_id: "e1".into(), seq: 1, event_type: "message".into(), content: "started".into(), occurred_at_ms: 1 }],
        tasks: vec![LedgerTaskV1 { task_id: "t1".into(), title: "Continue".into(), status: "open".into(), link: None }],
        artifacts: Vec::new(),
        decisions: vec![LedgerDecisionV1 { decision_id: "d1".into(), title: "Keep raw events".into(), content: "Ledger is derived".into(), link: None }],
    };
    let document = build_session_ledger(&input).unwrap();
    let replay = build_session_ledger(&input).unwrap();
    assert_eq!(document.content_hash, replay.content_hash);
    assert!(document.markdown.contains("Continue"));
    assert!(document.omissions.iter().any(|value| value.contains("missing event sequence 2..4")));
    assert_eq!(document.links.len(), 2);
    index_session_ledger(&GuideDb::open_in_memory(), &document, "rev-1", 1).unwrap();
}

fn stored_chunk(chunk: &TranscriptChunk) -> TranscriptChunkRecord {
    TranscriptChunkRecord {
        schema_version: chunk.schema_version,
        chunk_id: chunk.chunk_id.clone(),
        session_id: chunk.session_id.clone(),
        seq_start: chunk.seq_start,
        seq_end: chunk.seq_end,
        role: chunk.role.clone(),
        speaker: chunk.speaker.clone(),
        started_at_ms: chunk.started_at_ms,
        ended_at_ms: chunk.ended_at_ms,
        authority: chunk.authority.clone(),
        scope_id: chunk.scope_id.clone(),
        content_hash: chunk.content_hash.clone(),
        model_provenance: chunk.model_provenance.clone(),
        source_event_ids: chunk.source_event_ids.clone(),
        content: chunk.content.clone(),
        omissions: chunk.omissions.clone(),
    }
}

#[test]
fn transcript_projection_rebuilds_from_events_after_persisted_restart() {
    let chunks = build_transcript_chunks(
        "restart",
        &[event_for("restart", 1, "alpha"), event_for("restart", 2, "beta")],
        TranscriptChunkConfig { max_chars: 6 },
    )
    .unwrap();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("membrane-mem-i02-transcript-{stamp}.sqlite3"));
    let events_path = path.with_file_name(format!(
        "{}.membrane-events.sqlite3",
        path.file_stem().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&events_path);
    {
        let store = TranscriptStore::new(MemDb::open(&path).unwrap()).unwrap();
        for chunk in &chunks {
            store.put(&stored_chunk(chunk)).unwrap();
        }
    }
    let reopened = TranscriptStore::new(MemDb::open(&path).unwrap()).unwrap();
    assert_eq!(reopened.list_session("restart").unwrap().len(), 2);
    let hits = reopened.search("beta", Some("restart"), Some("workspace"), 10, 0).unwrap();
    assert_eq!(hits[0].chunk.chunk_id, "restart:2:3");
    drop(reopened);
    std::fs::remove_file(&path).unwrap();
    let _ = std::fs::remove_file(&events_path);
}

#[test]
fn ledger_index_reopens_with_same_hash_bound_projection() {
    let input = SessionLedgerInputV1 {
        session_id: "persisted-ledger".into(),
        title: Some("Replay".into()),
        source_cursor: LedgerSourceCursor {
            session_id: "persisted-ledger".into(),
            last_seq: 1,
        },
        source_content_hash: "sha256:source".into(),
        events: vec![LedgerEventV1 {
            event_id: "e1".into(),
            seq: 1,
            event_type: "message".into(),
            content: "stable".into(),
            occurred_at_ms: 1,
        }],
        tasks: Vec::new(),
        artifacts: Vec::new(),
        decisions: Vec::new(),
    };
    let document = build_session_ledger(&input).unwrap();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("membrane-mem-i02-guide-{stamp}.sqlite3"));
    let _ = std::fs::remove_file(&path);
    {
        let guide = GuideDb::open(&path).unwrap();
        index_session_ledger(&guide, &document, "rev-1", 1).unwrap();
    }
    let reopened = GuideDb::open(&path).unwrap();
    let count: i64 = reopened
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM guide_doc_projections WHERE parent_doc_id=?1 AND source_content_hash=?2",
            [&document.document_id, &document.source_content_hash],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    drop(reopened);
    std::fs::remove_file(&path).unwrap();
}
