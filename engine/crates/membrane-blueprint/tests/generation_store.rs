use membrane_blueprint::store::{
    load_generation, load_generation_pinned, open_store, read_generation_envelope,
    save_generation, Generation, StoreError,
};
use rusqlite::Connection;
use serde_json::{json, Value};

fn generation(id: &str) -> Generation {
    Generation {
        schema_version: Some(20),
        provider: Some(json!({"id": "native", "version": "1"})),
        manifest: Some(json!({
            "generationId": id,
            "manifestDigest": "sha256:test",
            "complete": true,
            "counts": {"files": 1, "symbols": 1, "edges": 1}
        })),
        repo_root: Some(json!("/repo")),
        augmentation: Some(json!({"enabled": false})),
        source_observation: Some(json!({"head": "abc", "dirty": false})),
        nodes: vec![
            json!({"id": "file:a.ts", "kind": "file", "labels": ["File"], "name": null,
                   "qualifiedName": null, "path": "a.ts", "confidence": null, "evidence": null,
                   "generationId": id, "providerExtra": {"kept": true}}),
            json!({"id": "symbol:a.ts::run", "kind": "function", "labels": ["Function"],
                   "name": "run", "qualifiedName": "a.ts::run", "path": "a.ts",
                   "confidence": 0.75, "evidence": [{"path": "a.ts", "line": 1}],
                   "generationId": id, "custom": "preserve"}),
        ],
        edges: vec![json!({
            "id": "edge:contains", "kind": "CONTAINS", "source": "file:a.ts",
            "target": "symbol:a.ts::run", "confidence": null, "confidenceTier": null,
            "evidence": null, "resolved": false, "specifier": null,
            "generationId": id, "edgeExtra": {"origin": "test"}
        })],
        file_reports: Vec::new(),
        documents: None,
        claims: None,
        claim_code_edges: None,
        document_supersession: None,
        extra: [("futureEnvelope".to_owned(), json!({"kept": true}))].into_iter().collect(),
    }
}

#[test]
fn typed_generation_round_trips_envelope_body_extras_and_nullable_fields() {
    let mut db = open_store(None).unwrap();
    let expected = generation("g-1");
    save_generation(&mut db, &expected).unwrap();

    let loaded = load_generation(&db).unwrap().unwrap();
    assert_eq!(loaded, expected);
    assert_eq!(read_generation_envelope(&db).unwrap().unwrap().generation_id(), Some("g-1"));
    assert_eq!(db.query_row("SELECT value FROM generation WHERE key='futureEnvelope'", [], |row| row.get::<_, String>(0)).unwrap(), r#"{"kept":true}"#);
}

#[test]
fn replacement_is_atomic_on_invalid_row_and_committed_generation_is_pinned() {
    let mut db = open_store(None).unwrap();
    let first = generation("g-1");
    save_generation(&mut db, &first).unwrap();

    let mut invalid = generation("g-2");
    invalid.edges[0] = json!({"id": "bad", "kind": "CALLS", "source": "file:a.ts", "confidence": "not-a-number"});
    let error = save_generation(&mut db, &invalid).unwrap_err();
    assert!(matches!(error, StoreError::InvalidGeneration(_)));
    assert_eq!(load_generation_pinned(&db, "g-1").unwrap(), first);
    assert!(matches!(load_generation_pinned(&db, "g-2"), Err(StoreError::GenerationMismatch { .. })));
}

#[test]
fn missing_generation_and_generation_mismatch_are_typed() {
    let db = open_store(None).unwrap();
    assert!(matches!(load_generation_pinned(&db, "missing"), Err(StoreError::GenerationNotFound)));

    let mut db = Connection::open_in_memory().unwrap();
    membrane_blueprint::migrations::migrate(&mut db).unwrap();
    db.execute("INSERT INTO generation(key,value) VALUES ('manifest', ?1)", [r#"{"generationId":"observed"}"#]).unwrap();
    assert!(matches!(load_generation_pinned(&db, "expected"), Err(StoreError::GenerationMismatch { expected, observed }) if expected == "expected" && observed == "observed"));
}

#[test]
fn node_and_edge_order_follows_generation_body_order() {
    let mut db = open_store(None).unwrap();
    let mut value = generation("ordered");
    value.nodes.reverse();
    value.edges.push(json!({"id":"edge:second","kind":"IMPORTS","source":"file:a.ts","target":null,"confidence":0.2,"evidence":[]}));
    save_generation(&mut db, &value).unwrap();
    let loaded = load_generation(&db).unwrap().unwrap();
    assert_eq!(loaded, value);
    assert_eq!(loaded.nodes.iter().map(|node| node["id"].clone()).collect::<Vec<Value>>(), value.nodes.iter().map(|node| node["id"].clone()).collect::<Vec<Value>>());
    assert_eq!(loaded.edges.iter().map(|edge| edge["id"].clone()).collect::<Vec<Value>>(), value.edges.iter().map(|edge| edge["id"].clone()).collect::<Vec<Value>>());
}

#[test]
fn document_projections_and_file_reports_round_trip_then_republish() {
    let mut db = open_store(None).unwrap();
    let mut value = generation("docs-1");
    value.file_reports = vec![json!({
        "path": "a.ts", "contentHash": null, "language": "typescript",
        "provider": "native", "parseStatus": "ok", "errorNodeCount": 0,
        "grammar": {"name": "typescript"}, "error": null
    })];
    value.documents = Some(vec![json!({"id":"doc-1","path":"README.md","contentHash":null,"generationId":"docs-1"})]);
    value.claims = Some(vec![json!({"id":"claim-1","documentId":"doc-1","source":null,"line":null,"text":"claim","status":null,"sha1":null,"generationId":"docs-1"})]);
    value.claim_code_edges = Some(vec![json!({
        "id":"join-1","claimId":"claim-1","kind":"implements","source":"doc-1","target":"symbol:a.ts::run",
        "confidence":null,"confidenceClass":null,"reason":null,
        "evidence":{"docRef":{"path":null,"line":null,"sha1":null},"codeRef":{"path":null,"exists":null},"codeNode":{"id":null,"contentHash":null}},
        "generationId":"docs-1"
    })]);
    value.document_supersession = Some(vec![json!({
        "id":"sup-1","sourceKind":"supersedes","sourceDoc":null,"sourceLine":null,"sourceText":null,
        "sourceExternal":true,"targetDoc":null,"targetExternal":true,"targetMatch":null,"supersededOn":null,"generationId":"docs-1"
    })]);
    save_generation(&mut db, &value).unwrap();
    let loaded = load_generation(&db).unwrap().unwrap();
    assert_eq!(loaded, value);

    save_generation(&mut db, &loaded).unwrap();
    assert_eq!(load_generation(&db).unwrap().unwrap(), value);

    let graph_only = generation("docs-2");
    save_generation(&mut db, &graph_only).unwrap();
    let retained = load_generation(&db).unwrap().unwrap();
    assert_eq!(retained.documents, value.documents);
    assert_eq!(retained.claims, value.claims);
    assert_eq!(retained.claim_code_edges, value.claim_code_edges);
    assert_eq!(retained.document_supersession, value.document_supersession);
}
