use crypt::{CheckpointError, CheckpointSourceRefV1, CheckpointV1, MemDb, MemoryStore};

fn checkpoint() -> CheckpointV1 {
    CheckpointV1 {
        checkpoint_id: "checkpoint/session-1/100".into(),
        installation_id: "550e8400-e29b-41d4-a716-446655440000".into(),
        client: "codex".into(),
        session_id: "session-1".into(),
        repository_id: "repo-1".into(),
        worktree_rev: "rev-1".into(),
        scope_id: "repo-1".into(),
        summary: "continue phase four".into(),
        goal_snapshot: Some("finish checkpoint contract".into()),
        task_snapshot: Some("add source-reference outcomes".into()),
        created_at_ms: 100,
        expires_at_ms: 200,
        source_refs: vec![],
    }
}

#[test]
fn checkpoint_rows_are_a0_session_records_and_never_ordinary_recall_candidates() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let checkpoint = checkpoint();
    store.save_checkpoint(&checkpoint).unwrap();
    assert_eq!(
        store
            .load_checkpoint(&checkpoint.checkpoint_id, 150)
            .unwrap()
            .summary,
        checkpoint.summary
    );
    assert!(matches!(
        store.load_checkpoint(&checkpoint.checkpoint_id, 200),
        Err(CheckpointError::Expired(_))
    ));
    let row: (String, String, String, String) = store.db().lock().query_row(
        "SELECT artifact_family, record_type, authority, influence_class FROM memories WHERE id=?1",
        [&checkpoint.checkpoint_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap();
    assert_eq!(
        row,
        (
            "session".into(),
            "checkpoint".into(),
            "A0".into(),
            "orientation".into()
        )
    );
    assert!(!store
        .recall_eligible_ids_at(150, false)
        .contains(&checkpoint.checkpoint_id));
}

#[test]
fn closing_checkpoint_removes_resume_door_without_deleting_row() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let checkpoint = checkpoint();
    store.save_checkpoint(&checkpoint).unwrap();
    store.close_checkpoint(&checkpoint.checkpoint_id).unwrap();
    assert!(store.list_checkpoints("repo-1", 150).unwrap().is_empty());
}

#[test]
fn checkpoint_rows_never_enter_markdown_mirror() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    store.save_checkpoint(&checkpoint()).unwrap();
    let output = tempfile::tempdir().unwrap();
    assert_eq!(store.export_md(output.path()), 0);
}

#[test]
fn checkpoint_source_refs_use_hash_bound_doc_read_statuses() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("guide.md"), "# Guide\nbody").unwrap();
    let source = "doc://repo/worktree/guide.md";
    let outline = crypt::outline::build_outline(source, "# Guide\nbody", "comrak-0.54.0");
    let mut value = checkpoint();
    value.source_refs = vec![CheckpointSourceRefV1 {
        source_ref: source.into(),
        expected_content_hash: outline.content_hash,
        anchor_id: "sec:guide:1".into(),
        label: "guide".into(),
    }];
    assert_eq!(
        crypt::checkpoint::resolve_source_refs(&value, temp.path())[0].status,
        "ok"
    );
    std::fs::write(temp.path().join("guide.md"), "# Guide\nchanged").unwrap();
    assert_eq!(
        crypt::checkpoint::resolve_source_refs(&value, temp.path())[0].status,
        "changed"
    );
    std::fs::remove_file(temp.path().join("guide.md")).unwrap();
    assert_eq!(
        crypt::checkpoint::resolve_source_refs(&value, temp.path())[0].status,
        "missing"
    );
    std::fs::write(temp.path().join("moved-guide.md"), "# Guide\nbody").unwrap();
    assert_eq!(
        crypt::checkpoint::resolve_source_refs(&value, temp.path())[0].status,
        "relocated"
    );
    value.source_refs[0].source_ref = "../guide.md".into();
    assert_eq!(
        crypt::checkpoint::resolve_source_refs(&value, temp.path())[0].status,
        "deny"
    );
}

#[test]
fn checkpoint_preserves_installation_lineage_and_optional_goal_task_snapshots() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let checkpoint = checkpoint();
    store.save_checkpoint(&checkpoint).unwrap();

    let loaded = store
        .load_checkpoint(&checkpoint.checkpoint_id, 150)
        .unwrap();
    assert_eq!(loaded.installation_id, checkpoint.installation_id);
    assert_eq!(loaded.goal_snapshot, checkpoint.goal_snapshot);
    assert_eq!(loaded.task_snapshot, checkpoint.task_snapshot);
}

#[test]
fn checkpoints_cannot_promote_their_summary_into_durable_memory() {
    let store = MemoryStore::open(MemDb::open_in_memory());
    let checkpoint = checkpoint();
    store.save_checkpoint(&checkpoint).unwrap();

    let durable_rows: i64 = store
        .db()
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM memories
             WHERE id=?1 AND artifact_family NOT IN ('session')",
            [&checkpoint.checkpoint_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        durable_rows, 0,
        "checkpoint save must never promote summary"
    );
}
