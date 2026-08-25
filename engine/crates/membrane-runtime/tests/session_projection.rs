use cortex_core::absorbed::SessionEvent;
use cortex_core::transcript::{
    build_transcript_chunks, retrieve_transcript_chunks, TranscriptChunkConfig,
};
use membrane_runtime::ledger::session_projection::{
    build_session_projection, index_session_projection, SessionDocumentProjectionInputV1,
    SessionProjectionDecisionV1, SessionProjectionEventV1, SessionProjectionSourceCursor,
    SessionProjectionTaskV1,
};
use membrane_runtime::ledger::LedgerDb;
use serde_json::json;

fn event(session_id: &str, seq: u64, event_id: &str, content: &str) -> SessionEvent {
    SessionEvent {
        schema_version: 1,
        session_id: session_id.to_owned(),
        seq,
        event_id: event_id.to_owned(),
        event_type: "message".to_owned(),
        payload: json!({"role": "user", "speaker": "adrian", "content": content}),
        scope_id: "workspace".to_owned(),
        authority: "A2".to_owned(),
        influence_class: "reference".to_owned(),
        lifecycle: "active".to_owned(),
        retention: "session".to_owned(),
        provenance: Vec::new(),
        occurred_at_ms: seq,
        recorded_at_ms: seq,
        content_hash: format!("sha256:{seq}"),
    }
}

#[test]
fn transcript_boundaries_and_retrieval_are_deterministic() {
    let events = vec![event("s1", 1, "e1", "alpha"), event("s1", 2, "e2", "beta")];
    let chunks = build_transcript_chunks(
        "s1",
        &events,
        TranscriptChunkConfig { max_chars: 6 },
    )
    .unwrap();
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].seq_end, 2);
    assert_eq!(retrieve_transcript_chunks(&chunks, "beta", None, 1)[0].chunk.chunk_id, "s1:2:3");
}

#[test]
fn session_projection_keeps_cursor_links_and_explicit_omissions() {
    let input = SessionDocumentProjectionInputV1 {
        session_id: "s1".to_owned(),
        title: Some("Handoff".to_owned()),
        source_cursor: SessionProjectionSourceCursor {
            session_id: "s1".to_owned(),
            last_seq: 3,
        },
        source_content_hash: "sha256:source".to_owned(),
        events: vec![SessionProjectionEventV1 {
            event_id: "e1".to_owned(),
            seq: 1,
            event_type: "message".to_owned(),
            content: "started".to_owned(),
            occurred_at_ms: 1,
        }],
        tasks: vec![SessionProjectionTaskV1 {
            task_id: "t1".to_owned(),
            title: "Continue".to_owned(),
            status: "open".to_owned(),
            link: None,
        }],
        artifacts: Vec::new(),
        decisions: vec![SessionProjectionDecisionV1 {
            decision_id: "d1".to_owned(),
            title: "Keep raw events".to_owned(),
            content: "Session projection is derived".to_owned(),
            link: None,
        }],
    };
    let document = build_session_projection(&input).unwrap();
    assert!(document.markdown.contains("Continue"));
    assert!(document.omissions.iter().any(|value| value.contains("missing event sequence 2..4")));
    assert_eq!(document.links.len(), 2);
    assert!(!document.invalidated_by(&input.source_cursor, "sha256:source"));
    assert!(document.invalidated_by(
        &SessionProjectionSourceCursor {
            session_id: "s1".to_owned(),
            last_seq: 4,
        },
        "sha256:source"
    ));
    index_session_projection(&LedgerDb::open_in_memory(), &document, "rev-1", 1).unwrap();
}
