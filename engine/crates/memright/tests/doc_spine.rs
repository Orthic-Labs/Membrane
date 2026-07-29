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
    std::fs::write(
        temp.path().join("docs").join("HEALTH").join("two.md"),
        "private",
    )
    .unwrap();
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
fn sync_hard_excludes_memory_pointers_exports_and_dependency_trees() {
    let temp = tempfile::tempdir().unwrap();
    for path in [
        "MEMORY.md",
        "memory-mirror/export.md",
        "node_modules/package.md",
        ".cache/generated.md",
        "target/generated.md",
        ".venv/generated.md",
        "vendor/generated.md",
    ] {
        let file = temp.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "excluded").unwrap();
    }
    std::fs::write(temp.path().join("public.md"), "included").unwrap();

    let db = MemDb::open_in_memory();
    let report = doc_spine::sync(&db, temp.path()).unwrap();
    assert_eq!(report.registered, 1);
    let path: String = db.lock().query_row(
        "SELECT path FROM doc_artifacts WHERE lifecycle_state='active'", [], |r| r.get(0),
    ).unwrap();
    assert_eq!(path, "public.md");
}

#[test]
fn sync_refreshes_hash_and_worktree_revision_after_content_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("runbook.md");
    std::fs::write(&path, "first").unwrap();
    let db = MemDb::open_in_memory();
    doc_spine::sync(&db, temp.path()).unwrap();
    let first: (String, String) = db
        .lock()
        .query_row(
            "SELECT content_hash, revision FROM doc_artifacts WHERE path='runbook.md'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();

    std::fs::write(&path, "second").unwrap();
    doc_spine::sync(&db, temp.path()).unwrap();
    let second: (String, String) = db
        .lock()
        .query_row(
            "SELECT content_hash, revision FROM doc_artifacts WHERE path='runbook.md'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();

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
    let first_id: String = db
        .lock()
        .query_row(
            "SELECT doc_id FROM doc_artifacts WHERE path='decision-old.md'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    std::fs::rename(old, new).unwrap();
    let report = doc_spine::sync(&db, temp.path()).unwrap();
    assert_eq!(report.tombstoned, 1);
    let active_id: String = db.lock().query_row(
        "SELECT doc_id FROM doc_artifacts WHERE path='decision-new.md' AND lifecycle_state='active'", [], |r| r.get(0),
    ).unwrap();
    let old_state: String = db
        .lock()
        .query_row(
            "SELECT lifecycle_state FROM doc_artifacts WHERE path='decision-old.md'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(
        active_id, first_id,
        "rename must retain a stable document alias"
    );
    assert_eq!(old_state, "tombstoned");
}

#[test]
fn sync_never_admits_document_content_as_durable_memory() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("policy.md"), "never durable memory").unwrap();
    let db = MemDb::open_in_memory();

    doc_spine::sync(&db, temp.path()).unwrap();

    let durable: i64 = db
        .lock()
        .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(durable, 0);
}

#[test]
fn sync_persists_machine_local_lexical_projection_with_current_provenance() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("runbook.md"),
        "# Runbook\n\nverify install",
    )
    .unwrap();
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
fn sync_frontmatter_projects_metadata_and_supersedes_declared_document() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("old.md"), "# Old\n").unwrap();
    let db = MemDb::open_in_memory();
    doc_spine::sync(&db, temp.path()).unwrap();

    std::fs::write(
        temp.path().join("new.md"),
        "---\nmembrane:\n  title: New Decision\n  summary: Replacement policy\n  keywords: [policy, replacement]\n  status: draft\n  supersedes: old.md\n---\n# New\n",
    )
    .unwrap();
    doc_spine::sync(&db, temp.path()).unwrap();

    let row: (String, String, String, String, String) = db.lock().query_row(
        "SELECT title, summary, keywords_json, lifecycle_state, doc_id FROM doc_artifacts WHERE path='new.md'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    ).unwrap();
    assert_eq!(row.0, "New Decision");
    assert_eq!(row.1, "Replacement policy");
    assert_eq!(row.2, "[\"policy\",\"replacement\"]");
    assert_eq!(row.3, "draft");
    let old: (String, String) = db
        .lock()
        .query_row(
            "SELECT lifecycle_state, superseded_by FROM doc_artifacts WHERE path='old.md'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(old, ("superseded".into(), row.4.clone()));
    let projection: String = db
        .lock()
        .query_row(
            "SELECT content FROM doc_projections WHERE parent_doc_id=?1 AND kind='lexical'",
            [&row.4],
            |r| r.get(0),
        )
        .unwrap();
    assert!(projection.contains("New Decision"));
    assert!(projection.contains("Replacement policy"));
    assert_eq!(
        db.lock()
            .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn sync_frontmatter_ignores_flat_framework_metadata() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("framework.md"),
        "---\ntitle: Framework Title\nstatus: draft\n---\n# Body\n",
    )
    .unwrap();
    let db = MemDb::open_in_memory();

    doc_spine::sync(&db, temp.path()).unwrap();

    let row: (String, String) = db
        .lock()
        .query_row(
            "SELECT COALESCE(title, ''), lifecycle_state FROM doc_artifacts WHERE path='framework.md'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, ("".into(), "active".into()));
}

#[test]
fn sync_frontmatter_uses_only_membrane_namespace() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("namespaced.md"),
        "---\ntitle: Framework Title\nstatus: active\nmembrane:\n  title: Membrane Title\n  status: draft\n---\n# Body\n",
    )
    .unwrap();
    let db = MemDb::open_in_memory();

    doc_spine::sync(&db, temp.path()).unwrap();

    let row: (String, String) = db
        .lock()
        .query_row(
            "SELECT title, lifecycle_state FROM doc_artifacts WHERE path='namespaced.md'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, ("Membrane Title".into(), "draft".into()));
}

#[test]
fn sync_frontmatter_parses_membrane_keyword_list() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("keywords.md"),
        "---\nmembrane:\n  keywords: [policy, replacement]\n---\n# Body\n",
    )
    .unwrap();
    let db = MemDb::open_in_memory();

    doc_spine::sync(&db, temp.path()).unwrap();

    let keywords: String = db
        .lock()
        .query_row(
            "SELECT keywords_json FROM doc_artifacts WHERE path='keywords.md'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(keywords, "[\"policy\",\"replacement\"]");
}

#[test]
fn sync_frontmatter_rejects_duplicate_or_dangling_supersession_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("invalid.md"),
        "---\nmembrane:\n  title: One\n  title: Two\n  supersedes: missing.md\n---\n# Invalid\n",
    )
    .unwrap();
    let db = MemDb::open_in_memory();
    let err =
        doc_spine::sync(&db, temp.path()).expect_err("duplicate frontmatter must fail closed");
    assert!(err.contains("duplicate frontmatter key"));
    assert_eq!(doc_rows(&db), 0);
    let projection_rows = db
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='doc_projections'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    assert!(
        projection_rows == 0
            || db
                .lock()
                .query_row("SELECT COUNT(*) FROM doc_projections", [], |r| r
                    .get::<_, i64>(0))
                .unwrap()
                == 0
    );
}

#[test]
fn sync_frontmatter_failure_matrix_is_atomic() {
    let cases: Vec<(&str, Vec<(&str, String)>, &str)> = vec![
        (
            "unclosed",
            vec![(
                "bad.md",
                "---\nmembrane:\n  title: missing close\n# Body\n".to_owned(),
            )],
            "unclosed frontmatter",
        ),
        (
            "malformed",
            vec![(
                "bad.md",
                "---\nmembrane:\n  title without colon\n---\n# Body\n".to_owned(),
            )],
            "malformed membrane frontmatter",
        ),
        (
            "missing",
            vec![(
                "bad.md",
                "---\nmembrane:\n  supersedes: absent.md\n---\n# Body\n".to_owned(),
            )],
            "target missing",
        ),
        (
            "self",
            vec![(
                "bad.md",
                "---\nmembrane:\n  supersedes: bad.md\n---\n# Body\n".to_owned(),
            )],
            "supersedes self",
        ),
        (
            "cycle",
            vec![
                (
                    "one.md",
                    "---\nmembrane:\n  supersedes: two.md\n---\n# One\n".to_owned(),
                ),
                (
                    "two.md",
                    "---\nmembrane:\n  supersedes: one.md\n---\n# Two\n".to_owned(),
                ),
            ],
            "supersedes cycle",
        ),
        (
            "oversized",
            vec![(
                "bad.md",
                format!(
                    "---\nmembrane:\n  title: {}\n---\n# Body\n",
                    "x".repeat(4 * 1024 + 1)
                ),
            )],
            "invalid frontmatter value",
        ),
        (
            "malformed_keywords",
            vec![(
                "bad.md",
                "---\nmembrane:\n  keywords: [policy\n---\n# Body\n".to_owned(),
            )],
            "invalid frontmatter keywords",
        ),
    ];
    for (name, files, expected) in cases {
        let temp = tempfile::tempdir().unwrap();
        for (path, content) in files {
            std::fs::write(temp.path().join(path), content).unwrap();
        }
        let db = MemDb::open_in_memory();
        let err = doc_spine::sync(&db, temp.path()).expect_err(name);
        assert!(err.contains(expected), "{name}: {err}");
        assert_eq!(doc_rows(&db), 0, "{name} must not write artifacts");
        let projections: i64 = db
            .lock()
            .query_row("SELECT COUNT(*) FROM doc_projections", [], |r| r.get(0))
            .unwrap_or(0);
        assert_eq!(projections, 0, "{name} must not write projections");
    }
}

#[test]
fn isolated_doc_recall_returns_a_hash_bound_pointer_to_the_matching_section() {
    let temp = tempfile::tempdir().unwrap();
    let markdown = "# Runbook\n\n## Deploy\n\nDoc Spine fetches the requested anchor as source-bound content.\n\n## Rollback\n\nRestore the prior release.\n";
    std::fs::write(temp.path().join("runbook.md"), markdown).unwrap();
    let db = MemDb::open_in_memory();

    doc_spine::sync(&db, temp.path()).unwrap();
    let hits = doc_spine::recall(&db, "requested anchor", 1).unwrap();

    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.source_ref, "doc://repo/worktree/runbook.md");
    assert_eq!(hit.anchor_id, "sec:deploy:1");
    let read = memright::outline::read_section(
        &hit.source_ref,
        markdown,
        &hit.anchor_id,
        &hit.expected_hash,
        12_000,
    )
    .expect("recall pointer fetches its exact source section");
    assert_eq!(
        read.content,
        "## Deploy\n\nDoc Spine fetches the requested anchor as source-bound content.\n\n"
    );
    let durable: i64 = db
        .lock()
        .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(durable, 0);
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
