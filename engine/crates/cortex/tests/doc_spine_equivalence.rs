use cortex::{doc_spine, MemDb};

#[test]
fn unchanged_sync_preserves_logical_artifact_and_projection_semantics() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("runbook.md"),
        "---\nclass: runbook\ninfluence: procedure\n---\n# Runbook\n",
    )
    .unwrap();
    let db = MemDb::open_in_memory();

    let first = doc_spine::sync(&db, temp.path()).unwrap();
    let first_row: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
    ) = db
        .lock()
        .query_row(
            "SELECT a.doc_id, a.content_hash, a.parser_version, a.lifecycle_state, \
                    a.document_class, a.influence_class, p.kind, p.source_revision, \
                    p.index_generation \
             FROM doc_artifacts a JOIN doc_projections p ON p.parent_doc_id = a.doc_id \
             WHERE a.path='runbook.md'",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .unwrap();

    let second = doc_spine::sync(&db, temp.path()).unwrap();
    let second_row: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
    ) = db
        .lock()
        .query_row(
            "SELECT a.doc_id, a.content_hash, a.parser_version, a.lifecycle_state, \
                    a.document_class, a.influence_class, p.kind, p.source_revision, \
                    p.index_generation \
             FROM doc_artifacts a JOIN doc_projections p ON p.parent_doc_id = a.doc_id \
             WHERE a.path='runbook.md'",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(first.registered, 1);
    assert_eq!(second.registered, 1);
    assert_eq!(second.tombstoned, 0);
    assert_eq!(
        (
            &first_row.0,
            &first_row.1,
            &first_row.2,
            &first_row.3,
            &first_row.4,
            &first_row.5,
            &first_row.6,
            &first_row.7,
        ),
        (
            &second_row.0,
            &second_row.1,
            &second_row.2,
            &second_row.3,
            &second_row.4,
            &second_row.5,
            &second_row.6,
            &second_row.7,
        )
    );
    assert!(second_row.8 > first_row.8);
    let durable: i64 = db
        .lock()
        .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(durable, 0);
}
