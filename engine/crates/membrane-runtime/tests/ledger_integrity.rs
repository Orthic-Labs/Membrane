//! Regression fixtures for the actual sync/recall path; no alternate indexer.
use membrane_runtime::ledger::{doc_spine, LedgerDb};
use std::fs;

fn state(db: &LedgerDb, path: &str) -> String {
    db.lock().query_row("SELECT lifecycle_state FROM ledger_doc_artifacts WHERE path=?1", [path], |r| r.get(0)).unwrap()
}

#[test]
fn copy_identity_and_divergent_edit_do_not_replace_original_projection() {
    let dir = tempfile::tempdir().unwrap();
    let db = LedgerDb::open_in_memory();
    fs::write(dir.path().join("a.md"), "# Setup\n\noriginalneedle\n").unwrap();
    doc_spine::sync(&db, dir.path()).unwrap();
    fs::copy(dir.path().join("a.md"), dir.path().join("b.md")).unwrap();
    doc_spine::sync(&db, dir.path()).unwrap();
    let identities: i64 = db.lock().query_row("SELECT COUNT(DISTINCT doc_id) FROM ledger_doc_artifacts", [], |r| r.get(0)).unwrap();
    assert_eq!(identities, 2);
    fs::write(dir.path().join("b.md"), "# Setup\n\ndivergentneedle\n").unwrap();
    doc_spine::sync(&db, dir.path()).unwrap();
    let original = doc_spine::recall(&db, "originalneedle", 10).unwrap();
    assert_eq!(original.len(), 1);
    assert!(original[0].source_ref.ends_with("/a.md"));
    let divergent = doc_spine::recall(&db, "divergentneedle", 10).unwrap();
    assert_eq!(divergent.len(), 1);
    assert!(divergent[0].source_ref.ends_with("/b.md"));
    fs::remove_file(dir.path().join("a.md")).unwrap();
    doc_spine::sync(&db, dir.path()).unwrap();
    assert_eq!(doc_spine::recall(&db, "divergentneedle", 10).unwrap().len(), 1);
}

#[test]
fn identical_bytes_reappear_after_tombstone() {
    let dir = tempfile::tempdir().unwrap();
    let db = LedgerDb::open_in_memory();
    let path = dir.path().join("return.md");
    let bytes = "# Return\n\nresurrectionneedle\n";
    fs::write(&path, bytes).unwrap();
    doc_spine::sync(&db, dir.path()).unwrap();
    fs::remove_file(&path).unwrap();
    doc_spine::sync(&db, dir.path()).unwrap();
    assert_eq!(state(&db, "return.md"), "tombstoned");
    fs::write(&path, bytes).unwrap();
    doc_spine::sync(&db, dir.path()).unwrap();
    assert_eq!(state(&db, "return.md"), "active");
    assert_eq!(doc_spine::recall(&db, "resurrectionneedle", 3).unwrap().len(), 1);
}

#[test]
fn complete_projection_is_independent_of_outline_page_size() {
    for count in [255usize, 256, 257, 1000] {
        let dir = tempfile::tempdir().unwrap();
        let db = LedgerDb::open_in_memory();
        let mut markdown = String::new();
        for index in 0..count {
            markdown.push_str(&format!("# Heading {index}\n\nbody {index}\n\n"));
        }
        markdown.push_str("uniquelastsectionneedle\n");
        fs::write(dir.path().join("long.md"), markdown).unwrap();
        doc_spine::sync(&db, dir.path()).unwrap();
        let sections: i64 = db.lock().query_row("SELECT COUNT(*) FROM ledger_nodes WHERE node_kind='section'", [], |r| r.get(0)).unwrap();
        assert_eq!(sections, count as i64);
        let hits = doc_spine::recall(&db, "uniquelastsectionneedle", 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].anchor_id, format!("sec:heading-{}:1", count - 1));
    }
}

#[test]
fn malformed_utf8_does_not_publish_a_successful_lossy_generation() {
    let dir = tempfile::tempdir().unwrap();
    let db = LedgerDb::open_in_memory();
    fs::write(dir.path().join("valid.md"), "# Keep\n\nknownneedle\n").unwrap();
    doc_spine::sync(&db, dir.path()).unwrap();
    fs::write(dir.path().join("invalid.md"), b"# Bad\n\xff").unwrap();
    assert!(doc_spine::sync(&db, dir.path()).unwrap_err().contains("unsupported_encoding"));
    assert_eq!(doc_spine::recall(&db, "knownneedle", 1).unwrap().len(), 1);
}
