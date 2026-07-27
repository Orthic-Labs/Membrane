use memright::{doc_spine, MemDb};

fn doc_rows(db: &MemDb) -> i64 {
    db.lock()
        .query_row("SELECT COUNT(*) FROM doc_artifacts", [], |r| r.get(0))
        .unwrap()
}

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

#[test]
fn sync_excludes_health_at_any_depth_regardless_of_casing() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("health")).unwrap();
    std::fs::create_dir_all(temp.path().join("docs").join("HEALTH")).unwrap();
    std::fs::write(temp.path().join("health").join("one.md"), "private").unwrap();
    std::fs::write(temp.path().join("docs").join("HEALTH").join("two.md"), "private").unwrap();
    std::fs::write(temp.path().join("public.md"), "public").unwrap();

    let db = MemDb::open_in_memory();
    let report = doc_spine::sync(&db, temp.path()).unwrap();

    assert_eq!(report.registered, 1);
    assert!(report.excluded_health >= 2);
    assert_eq!(doc_rows(&db), 1);
}

#[test]
fn sync_discovers_tracked_and_nonignored_markdown_but_skips_gitignored_files() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join(".gitignore"), "ignored.md\n").unwrap();
    std::fs::write(temp.path().join("tracked.md"), "tracked").unwrap();
    std::fs::write(temp.path().join("untracked.md"), "nonignored").unwrap();
    std::fs::write(temp.path().join("ignored.md"), "ignored").unwrap();

    let db = MemDb::open_in_memory();
    let report = doc_spine::sync(&db, temp.path()).unwrap();

    assert_eq!(report.registered, 2);
    let paths: Vec<String> = {
        let conn = db.lock();
        let mut statement = conn
            .prepare("SELECT path FROM doc_artifacts WHERE lifecycle_state='active' ORDER BY path")
            .unwrap();
        statement
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(paths, ["tracked.md", "untracked.md"]);
}

#[test]
fn sync_refreshes_hash_and_worktree_revision_after_content_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("runbook.md");
    std::fs::write(&path, "first").unwrap();
    let db = MemDb::open_in_memory();
    doc_spine::sync(&db, temp.path()).unwrap();
    let first: (String, String) = db.lock().query_row(
        "SELECT content_hash, revision FROM doc_artifacts WHERE path='runbook.md'", [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();

    std::fs::write(&path, "second").unwrap();
    doc_spine::sync(&db, temp.path()).unwrap();
    let second: (String, String) = db.lock().query_row(
        "SELECT content_hash, revision FROM doc_artifacts WHERE path='runbook.md'", [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();

    assert_ne!(first.0, second.0);
    assert_eq!(second.1, "worktree");
}

#[test]
fn sync_preserves_document_identity_across_rename_and_tombstones_old_path() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("decision-old.md");
    let new = temp.path().join("decision-new.md");
    std::fs::write(&old, "# Decision\nretain identity").unwrap();
    let db = MemDb::open_in_memory();
    doc_spine::sync(&db, temp.path()).unwrap();
    let first_id: String = db.lock().query_row(
        "SELECT doc_id FROM doc_artifacts WHERE path='decision-old.md'", [], |r| r.get(0),
    ).unwrap();

    std::fs::rename(old, new).unwrap();
    let report = doc_spine::sync(&db, temp.path()).unwrap();
    assert_eq!(report.tombstoned, 1);
    let active_id: String = db.lock().query_row(
        "SELECT doc_id FROM doc_artifacts WHERE path='decision-new.md' AND lifecycle_state='active'", [], |r| r.get(0),
    ).unwrap();
    let old_state: String = db.lock().query_row(
        "SELECT lifecycle_state FROM doc_artifacts WHERE path='decision-old.md'", [], |r| r.get(0),
    ).unwrap();

    assert_eq!(active_id, first_id, "rename must retain a stable document alias");
    assert_eq!(old_state, "tombstoned");
}

#[test]
fn sync_never_admits_document_content_as_durable_memory() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("policy.md"), "never durable memory").unwrap();
    let db = MemDb::open_in_memory();

    doc_spine::sync(&db, temp.path()).unwrap();

    let durable: i64 = db.lock().query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0)).unwrap();
    assert_eq!(durable, 0);
}

#[test]
fn sync_persists_machine_local_lexical_projection_with_current_provenance() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("runbook.md"), "# Runbook\n\nverify install").unwrap();
    let db = MemDb::open_in_memory();

    doc_spine::sync(&db, temp.path()).unwrap();

    let row: (String, String, String, i64) = db.lock().query_row(
        "SELECT kind, source_content_hash, source_revision, index_generation FROM doc_projections",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).unwrap();
    assert_eq!(row.0, "lexical");
    assert!(!row.1.is_empty());
    assert_eq!(row.2, "worktree");
    assert!(row.3 > 0);
}

#[test]
fn sync_rolls_back_projection_when_one_artifact_write_fails() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("first.md"), "first").unwrap();
    std::fs::write(temp.path().join("second.md"), "second").unwrap();
    let db = MemDb::open_in_memory();
    {
        let conn = db.lock();
        conn.execute_batch(
            "CREATE TABLE doc_artifacts (doc_id TEXT PRIMARY KEY, repository_root TEXT NOT NULL, repository_id TEXT NOT NULL, revision TEXT NOT NULL, path TEXT NOT NULL, content_hash TEXT NOT NULL, parser_version TEXT NOT NULL, document_class TEXT NOT NULL, lifecycle_state TEXT NOT NULL, trust_label TEXT NOT NULL, influence_class TEXT NOT NULL, sensitivity TEXT NOT NULL, generated INTEGER NOT NULL, index_generation INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL, UNIQUE(repository_root, path));\n             CREATE TRIGGER reject_second BEFORE INSERT ON doc_artifacts WHEN NEW.path='second.md' BEGIN SELECT RAISE(ABORT, 'projection rejected'); END;",
        ).unwrap();
    }

    let err = doc_spine::sync(&db, temp.path()).expect_err("projection failure must surface");
    assert!(err.contains("projection rejected"));
    assert_eq!(doc_rows(&db), 0, "projection writes must commit atomically");
}
