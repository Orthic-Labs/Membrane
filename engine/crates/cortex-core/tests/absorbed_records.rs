#[path = "../src/absorbed.rs"]
mod absorbed;

use absorbed::{content_hash, event_range, validate_event_import, ProvenanceRef, SessionEvent, ABSORBED_SCHEMA_VERSION};

fn event(seq: u64, id: &str) -> SessionEvent {
    SessionEvent {
        schema_version: ABSORBED_SCHEMA_VERSION,
        session_id: "session-1".into(),
        seq,
        event_id: id.into(),
        event_type: "turn".into(),
        payload: serde_json::json!({"seq": seq}),
        scope_id: "scope-1".into(),
        authority: "A1".into(),
        influence_class: "reference".into(),
        lifecycle: "active".into(),
        retention: "session".into(),
        provenance: vec![ProvenanceRef {
                source: "fixture".into(),
                source_event_ids: vec![id.into()],
                producer: Some("test".into()),
            }],
        occurred_at_ms: seq,
        recorded_at_ms: seq,
        content_hash: format!("hash-{seq}"),
    }
}

#[test]
fn imports_only_contiguous_ordered_events() {
    let events = vec![event(1, "e1"), event(2, "e2")];
    assert!(validate_event_import(&events).is_ok());
    assert!(validate_event_import(&[event(1, "e1"), event(3, "e3")]).is_err());
    assert!(validate_event_import(&[event(2, "e2"), event(1, "e1")]).is_err());
}

#[test]
fn range_is_inclusive_exclusive_and_stable() {
    let events = vec![event(1, "e1"), event(2, "e2"), event(3, "e3")];
    let page = event_range(&events, 1, 3);
    assert_eq!(page.iter().map(|item| item.seq).collect::<Vec<_>>(), vec![1, 2]);
}

#[test]
fn content_hash_is_deterministic() {
    assert_eq!(content_hash(&serde_json::json!({"a": 1})).unwrap(), content_hash(&serde_json::json!({"a": 1})).unwrap());
}
