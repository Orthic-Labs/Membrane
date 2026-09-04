//! Exact registered-source resolution, including imported snapshots and cursors.
use membrane_runtime::ledger::{doc_spine, document_conversion::*, resolve::*, LedgerDb};
use std::fs;

fn request(db: &LedgerDb, path: &str, kind: &str) -> ResolveRequest {
    let conn = db.lock();
    conn.query_row(
        "SELECT a.doc_id,n.node_id,a.content_hash,a.revision,a.index_generation,n.span_hash
         FROM ledger_doc_artifacts a JOIN ledger_nodes n ON n.doc_id=a.doc_id
         WHERE a.path=?1 AND n.node_kind=?2 ORDER BY n.ordinal LIMIT 1",
        [path, kind], |r| Ok(ResolveRequest {
            doc_id: Some(r.get(0)?), node_id: Some(r.get(1)?),
            source_ref: format!("doc://repo/worktree/{path}"), anchor_id: r.get(1)?,
            expected_content_hash: r.get(2)?, expected_revision: Some(r.get(3)?),
            ledger_generation: Some(r.get(4)?), expected_span_hash: Some(r.get(5)?),
            continuation_cursor: None, max_bytes: 4,
        }),
    ).unwrap()
}

#[test]
fn exact_code_node_and_consumable_utf8_cursor_do_not_expand_to_parent() {
    let root = tempfile::tempdir().unwrap();
    let db = LedgerDb::open_in_memory();
    let markdown = "# Setup\n\nprose outside the requested block\n\n```text\n日本語🙂\n```\n";
    fs::write(root.path().join("setup.md"), markdown).unwrap();
    doc_spine::sync(&db, root.path()).unwrap();
    let mut req = request(&db, "setup.md", "fenced_code");
    let mut recovered = String::new();
    for _ in 0..100 {
        let response = resolve(&db, root.path(), &req).unwrap();
        recovered.push_str(&response.read.content);
        assert!(!response.read.content.contains("prose outside"));
        req.continuation_cursor = response.read.continuation_cursor;
        if req.continuation_cursor.is_none() { break; }
    }
    assert!(req.continuation_cursor.is_none());
    let node = db.lock().query_row("SELECT searchable_text FROM ledger_nodes WHERE node_id=?1",
        [req.node_id.as_ref().unwrap()], |r| r.get::<_, String>(0)).unwrap();
    assert_eq!(recovered, node);
}

#[test]
fn unicode_final_byte_and_too_small_page_are_safe() {
    let root = tempfile::tempdir().unwrap();
    let db = LedgerDb::open_in_memory();
    fs::write(root.path().join("unicode.md"), "# Name\n\n日本語").unwrap();
    doc_spine::sync(&db, root.path()).unwrap();
    let mut req = request(&db, "unicode.md", "paragraph");
    req.max_bytes = 1;
    assert_eq!(resolve(&db, root.path(), &req).unwrap_err(), ResolveError::BudgetExhausted);
    req.max_bytes = 12_000;
    assert_eq!(resolve(&db, root.path(), &req).unwrap().read.content, "日本語");
}

#[test]
fn stale_cursor_and_cross_root_document_id_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let db = LedgerDb::open_in_memory();
    fs::write(root.path().join("setup.md"), "# Setup\n\nlong original section body\n").unwrap();
    doc_spine::sync(&db, root.path()).unwrap();
    let mut req = request(&db, "setup.md", "section");
    req.continuation_cursor = resolve(&db, root.path(), &req).unwrap().read.continuation_cursor;
    assert!(resolve(&db, other.path(), &req).is_err());
    fs::write(root.path().join("setup.md"), "# Setup\n\nchanged\n").unwrap();
    assert_eq!(resolve(&db, root.path(), &req).unwrap_err(), ResolveError::Stale);
}

#[test]
fn converted_snapshot_survives_markdown_sync_and_detects_normalized_tampering() {
    let root = tempfile::tempdir().unwrap();
    let db = LedgerDb::open_in_memory();
    let grant = DocumentConversionGrantV1::new([DocumentInputFormatV1::PlainText], 4096);
    let artifact = doc_spine::ingest_granted_document(&db, &grant, doc_spine::GrantedDocumentIngestV1 {
        repository_root: root.path().canonicalize().unwrap().to_str().unwrap().replace('\\', "/"),
        repository_id: "test-repo".into(), revision: "import-1".into(), path: "import.txt".into(),
        title: "Imported".into(), document: DocumentConversionInputV1 {
            source_ref: "snapshot://import-1".into(), format: DocumentInputFormatV1::PlainText,
            raw_input: b"# Imported\n\nimmutable snapshot needle\n".to_vec(),
        },
    }).unwrap();
    fs::write(root.path().join("ordinary.md"), "# Ordinary\n\nother words\n").unwrap();
    doc_spine::sync(&db, root.path()).unwrap();
    let mut req = request(&db, "import.txt", "section");
    req.source_ref = format!("ledger://doc/{}", artifact.doc_id);
    req.max_bytes = 12_000;
    let read = resolve(&db, root.path(), &req).unwrap();
    assert_eq!(read.source_kind, "imported_snapshot");
    assert!(read.read.content.contains("immutable snapshot needle"));
    db.lock().execute("UPDATE ledger_document_conversions SET markdown='forged' WHERE doc_id=?1", [&artifact.doc_id]).unwrap();
    assert_eq!(resolve(&db, root.path(), &req).unwrap_err(), ResolveError::Stale);
}
