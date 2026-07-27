use memright::{doc_spine, MemDb};

#[test]
fn shadow_sync_registers_docs_excludes_health_and_tombstones_deletes() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("Health")).unwrap();
    std::fs::write(temp.path().join("runbook.md"), "# Runbook").unwrap();
    std::fs::write(temp.path().join("Health").join("private.md"), "never index").unwrap();
    let db = MemDb::open_in_memory();
    let first = doc_spine::sync(&db, temp.path()).unwrap();
    assert_eq!(first.registered, 1);
    assert!(first.excluded_health > 0);
    let row: (String, String, String) = db.lock().query_row(
        "SELECT document_class, lifecycle_state, influence_class FROM doc_artifacts WHERE path='runbook.md'",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).unwrap();
    assert_eq!(row, ("runbook".into(), "active".into(), "procedure".into()));
    std::fs::remove_file(temp.path().join("runbook.md")).unwrap();
    let second = doc_spine::sync(&db, temp.path()).unwrap();
    assert_eq!(second.tombstoned, 1);
}
