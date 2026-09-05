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

#[test]
fn unrelated_sibling_insertion_does_not_churn_section_identity() {
    let dir = tempfile::tempdir().unwrap();
    let db = LedgerDb::open_in_memory();
    let path = dir.path().join("stable.md");
    fs::write(&path, "# First\n\nalpha\n\n# Target\n\nstable body\n").unwrap();
    doc_spine::sync(&db, dir.path()).unwrap();
    let before: String = db.lock().query_row(
        "SELECT n.node_id FROM ledger_nodes n JOIN ledger_doc_artifacts a ON a.doc_id=n.doc_id WHERE a.path='stable.md' AND n.heading='Target' AND n.node_kind='section'",
        [], |r| r.get(0),
    ).unwrap();
    fs::write(&path, "# First\n\nalpha\n\n# Inserted\n\nunrelated\n\n# Target\n\nstable body\n").unwrap();
    doc_spine::sync(&db, dir.path()).unwrap();
    let after: String = db.lock().query_row(
        "SELECT n.node_id FROM ledger_nodes n JOIN ledger_doc_artifacts a ON a.doc_id=n.doc_id WHERE a.path='stable.md' AND n.heading='Target' AND n.node_kind='section'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(before, after);
}

#[test]
fn syncing_one_root_does_not_advance_another_roots_publication() {
    let root_a = tempfile::tempdir().unwrap();
    let root_b = tempfile::tempdir().unwrap();
    let db = LedgerDb::open_in_memory();
    fs::write(root_a.path().join("a.md"), "# A\n\nalpha\n").unwrap();
    fs::write(root_b.path().join("b.md"), "# B\n\nbeta\n").unwrap();
    doc_spine::sync(&db, root_a.path()).unwrap();
    let root_a_key = root_a.path().canonicalize().unwrap().to_str().unwrap().replace('\\', "/");
    let before: (i64, i64) = db.lock().query_row(
        "SELECT a.index_generation,n.ledger_generation FROM ledger_doc_artifacts a JOIN ledger_nodes n ON n.doc_id=a.doc_id WHERE a.repository_root=?1 ORDER BY n.ordinal LIMIT 1",
        [&root_a_key], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();
    doc_spine::sync(&db, root_b.path()).unwrap();
    let after: (i64, i64) = db.lock().query_row(
        "SELECT a.index_generation,n.ledger_generation FROM ledger_doc_artifacts a JOIN ledger_nodes n ON n.doc_id=a.doc_id WHERE a.repository_root=?1 ORDER BY n.ordinal LIMIT 1",
        [&root_a_key], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();
    assert_eq!(after, before);
}
