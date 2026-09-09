//! Behavioral qualification for committed Ledger residuals.
//!
//! These cases deliberately use the public Ledger spine, recall, and resolver
//! paths.  Direct SQL below only inspects persisted evidence emitted by those
//! paths; it does not manufacture Ledger rows.

use membrane_runtime::ledger::{
    doc_spine,
    index::{self, LedgerRecallMode},
    resolve::{resolve, ResolveRequest},
    session_projection::{
        build_session_projection, index_session_projection, SessionDocumentProjectionInputV1,
        SessionProjectionEventV1, SessionProjectionSourceCursor, SessionProjectionTaskV1,
    },
    LedgerDb,
};
use sha2::{Digest, Sha256};
use std::fs;

fn hash(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

fn root_text(root: &tempfile::TempDir) -> String {
    root.path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

fn artifact(db: &LedgerDb, root: &str, path: &str) -> (String, String, String, i64) {
    db.lock()
        .query_row(
            "SELECT doc_id, content_hash, lifecycle_state, index_generation
             FROM ledger_doc_artifacts WHERE repository_root=?1 AND path=?2",
            rusqlite::params![root, path],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap()
}

fn node_request(db: &LedgerDb, hit: &doc_spine::DocRecallHitV1) -> ResolveRequest {
    let node_id: String = db
        .lock()
        .query_row(
            "SELECT node_id FROM ledger_nodes WHERE doc_id=?1 AND anchor_id=?2 LIMIT 1",
            rusqlite::params![hit.doc_id, hit.anchor_id],
            |row| row.get(0),
        )
        .unwrap();
    ResolveRequest {
        doc_id: Some(hit.doc_id.clone()),
        node_id: Some(node_id),
        source_ref: hit.source_ref.clone(),
        anchor_id: hit.anchor_id.clone(),
        expected_content_hash: hit.expected_hash.clone(),
        expected_revision: None,
        expected_span_hash: None,
        ledger_generation: None,
        continuation_cursor: None,
        max_bytes: 12_000,
    }
}

#[test]
fn source_reconciliation_keeps_owner_scoped_identity_and_lifecycle() {
    // LDG-001, LDG-004, LDG-018, LDG-024, LDG-026: separate roots/copies,
    // source-owned refresh, qualified lifecycle, and no ambient-root lookup.
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    let markdown = "# Shared\n\nowner scoped evidence\n";
    fs::write(left.path().join("guide.md"), markdown).unwrap();
    fs::write(right.path().join("guide.md"), markdown).unwrap();
    let db = LedgerDb::open_in_memory();
    doc_spine::sync(&db, left.path()).unwrap();
    doc_spine::sync(&db, right.path()).unwrap();

    let left_root = root_text(&left);
    let right_root = root_text(&right);
    let left_first = artifact(&db, &left_root, "guide.md");
    let right_first = artifact(&db, &right_root, "guide.md");
    assert_ne!(left_first.0, right_first.0, "equal bytes must retain source identity");
    assert_eq!(left_first.1, hash(markdown.as_bytes()));

    fs::write(left.path().join("guide.md"), "# Shared\n\nchanged owner scoped evidence\n").unwrap();
    doc_spine::sync(&db, left.path()).unwrap();
    let left_changed = artifact(&db, &left_root, "guide.md");
    assert_ne!(left_changed.1, left_first.1);
    assert_eq!(artifact(&db, &right_root, "guide.md").1, right_first.1);

    fs::remove_file(right.path().join("guide.md")).unwrap();
    doc_spine::sync(&db, right.path()).unwrap();
    assert_eq!(artifact(&db, &right_root, "guide.md").2, "tombstoned");
    let hits = doc_spine::recall(&db, "owner scoped evidence", 10).unwrap();
    assert!(hits.iter().all(|hit| hit.doc_id != right_first.0));
    assert!(hits.iter().all(|hit| hit.source_ref.starts_with("doc://repo/worktree/")));

    let wrong_root_request = hits
        .iter()
        .find(|hit| hit.doc_id == left_changed.0)
        .map(|hit| node_request(&db, hit));
    if let Some(request) = wrong_root_request {
        assert!(resolve(&db, right.path(), &request).is_err());
    }
}

#[test]
fn one_indexed_ast_preserves_nested_ranges_projection_and_safe_query_lanes() {
    // LDG-002, LDG-003, LDG-007, LDG-009, LDG-010, LDG-011, LDG-013, LDG-020.
    let root = tempfile::tempdir().unwrap();
    let markdown = "# Root\n\n## Nested\n\n- outer\n  - inner Unicode 日本語🙂\n\n> quoted body\n\n| Name | Value |\n| --- | --- |\n| marker | HTTPServer |\n";
    fs::write(root.path().join("nested.md"), markdown).unwrap();
    let db = LedgerDb::open_in_memory();
    let report = doc_spine::sync(&db, root.path()).unwrap();
    assert_eq!(report.registered, 1);

    let root_text = root_text(&root);
    let rows: Vec<(String, String, i64, i64, String, String, String)> = {
        let conn = db.lock();
        let mut statement = conn
            .prepare(
                "SELECT n.node_kind,n.node_id,n.source_start_byte,n.source_end_byte,
                        n.span_hash,n.source_revision,n.projection_schema_version
                 FROM ledger_nodes n JOIN ledger_doc_artifacts a ON a.doc_id=n.doc_id
                 WHERE a.repository_root=?1 AND a.path='nested.md' ORDER BY n.ordinal",
            )
            .unwrap();
        statement
            .query_map([root_text], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert!(rows.len() >= 7, "sections and nested blocks must share one complete AST");
    assert!(rows.iter().any(|row| row.0 == "list_item"));
    assert!(rows.iter().any(|row| row.0 == "blockquote"));
    assert!(rows.iter().any(|row| row.0 == "table"));
    assert!(rows.iter().all(|row| {
        row.2 >= 0
            && row.3 >= row.2
            && row.3 as usize <= markdown.len()
            && row.2 as usize <= markdown.len()
            && row.4.len() == 64
            && row.5 == "worktree"
            && row.6 == index::PROJECTION_SCHEMA_VERSION
    }));

    assert_eq!(index::normalize_query("ＨＴＴＰServer"), "httpserver");
    assert!(index::query_terms("HTTPServer 日本語").iter().any(|term| term == "httpserver"));
    assert!(doc_spine::recall(&db, "HTTPServer", 3)
        .unwrap()
        .iter()
        .any(|hit| hit.source_ref.ends_with("/nested.md")));
    assert_eq!(index::recall_mode(&db).unwrap(), LedgerRecallMode::LegacyScan);
    assert_eq!(
        index::activate(&db, LedgerRecallMode::LedgerFts, None).unwrap_err(),
        "ledger_fts_requires_qualification"
    );
    index::activate(&db, LedgerRecallMode::Shadow, None).unwrap();
    assert_eq!(index::recall_mode(&db).unwrap(), LedgerRecallMode::Shadow);
}

#[test]
fn exact_search_returns_source_bound_resolution_and_refuses_drift() {
    // LDG-005, LDG-006, LDG-008, LDG-012, LDG-017, LDG-027: exact-first
    // search, typed source evidence, coherent generation, and drift refusal.
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("runbook.md"),
        "# Runbook\n\n## Deploy\n\nDeploy only from the enrolled worktree.\n\n## Rollback\n\nRestore the prior release.\n",
    )
    .unwrap();
    let db = LedgerDb::open_in_memory();
    doc_spine::sync(&db, root.path()).unwrap();

    let hits = doc_spine::recall(&db, "Deploy", 1).unwrap();
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.anchor_id, "sec:deploy:1");
    assert_eq!(hit.source_ref, "doc://repo/worktree/runbook.md");
    let resolved = resolve(&db, root.path(), &node_request(&db, hit)).unwrap();
    assert_eq!(resolved.doc_id, hit.doc_id);
    assert_eq!(resolved.raw_content_hash, hit.expected_hash);
    assert!(resolved.read.content.contains("Deploy only from the enrolled worktree."));

    fs::write(root.path().join("runbook.md"), "# Runbook\n\n## Deploy\n\nchanged bytes\n").unwrap();
    assert!(resolve(&db, root.path(), &node_request(&db, hit)).is_err());
    assert!(doc_spine::read_registered_section(&db, &hit.doc_id, &hit.anchor_id, 4096).is_err());
}

#[test]
fn graph_expansion_is_bounded_and_follows_current_active_lifecycle() {
    // LDG-015, LDG-016, LDG-017, LDG-018, LDG-022: link projection, strong
    // seed graph bounds, coherent source generations, and active-source scope.
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("source.md"),
        "# Source\n\nkey rotation procedure [details](target.md)\n",
    )
    .unwrap();
    fs::write(root.path().join("target.md"), "# Target\n\nlinked evidence for rotation\n").unwrap();
    let db = LedgerDb::open_in_memory();
    doc_spine::sync(&db, root.path()).unwrap();
    let receipt = doc_spine::recall_with_graph(
        &db,
        "key rotation",
        5,
        &doc_spine::LedgerRecallGraphPolicyV1::default(),
    )
    .unwrap();
    assert!(receipt.graph.edges.len() <= 6);
    assert!(receipt.hits.iter().any(|hit| hit.source_ref.ends_with("/source.md")));
    assert!(receipt
        .hits
        .iter()
        .any(|hit| hit.lane == "ledger_graph" && hit.source_ref.ends_with("/target.md")));

    fs::remove_file(root.path().join("target.md")).unwrap();
    doc_spine::sync(&db, root.path()).unwrap();
    let after_delete = doc_spine::recall_with_graph(
        &db,
        "key rotation",
        5,
        &doc_spine::LedgerRecallGraphPolicyV1::default(),
    )
    .unwrap();
    assert!(!after_delete.hits.iter().any(|hit| hit.source_ref.ends_with("/target.md")));
}

#[test]
fn session_projection_is_derived_non_recallable_and_explicitly_invalidated() {
    // LDG-021, LDG-025: generated session projection remains separate from
    // registered document truth, while cursor/hash changes invalidate it.
    let db = LedgerDb::open_in_memory();
    let input = SessionDocumentProjectionInputV1 {
        session_id: "session-1".into(),
        title: Some("Handoff".into()),
        source_cursor: SessionProjectionSourceCursor {
            session_id: "session-1".into(),
            last_seq: 2,
        },
        source_content_hash: "sha256:source".into(),
        events: vec![SessionProjectionEventV1 {
            event_id: "event-1".into(),
            seq: 1,
            event_type: "message".into(),
            content: "derived session evidence".into(),
            occurred_at_ms: 1,
        }],
        tasks: vec![SessionProjectionTaskV1 {
            task_id: "task-1".into(),
            title: "Continue qualification".into(),
            status: "open".into(),
            link: None,
        }],
        artifacts: Vec::new(),
        decisions: Vec::new(),
    };
    let projection = build_session_projection(&input).unwrap();
    assert!(projection.generated);
    assert!(projection
        .omissions
        .iter()
        .any(|omission| omission.contains("missing event sequence 2..3")));
    assert!(projection.markdown.contains("Continue qualification"));
    index_session_projection(&db, &projection, "session-rev", 1).unwrap();
    let artifact_count: i64 = db
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM ledger_doc_artifacts WHERE doc_id=?1",
            [&projection.document_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(artifact_count, 0, "session projection cannot become source authority");
    assert!(doc_spine::recall(&db, "derived session evidence", 5)
        .unwrap()
        .is_empty());
    assert!(projection.invalidated_by(
        &SessionProjectionSourceCursor {
            session_id: "session-1".into(),
            last_seq: 3,
        },
        "sha256:source",
    ));
    assert!(projection.invalidated_by(&input.source_cursor, "sha256:changed"));
}

#[test]
fn literal_projection_reads_exact_source_bytes_and_provider_pointers_keep_authority() {
    // LDG-019, LDG-030 plus provider-authority boundary for LDG-022: literal
    // verification uses persisted spans, while public hits remain resolvable
    // pointers to the enrolled source rather than copied authority.
    let root = tempfile::tempdir().unwrap();
    let markdown = "# Literal\n\nExact A+B punctuation and CaseSensitive text.\n\n```text\nA+B\n```\n\n| value |\n| --- |\n| MixedCase |\n";
    fs::write(root.path().join("literal.md"), markdown).unwrap();
    let db = LedgerDb::open_in_memory();
    doc_spine::sync(&db, root.path()).unwrap();
    let root_text = root_text(&root);
    let rows: Vec<(String, String, i64, i64)> = {
        let conn = db.lock();
        let mut statement = conn
            .prepare(
                "SELECT n.node_id,n.node_kind,n.source_start_byte,n.source_end_byte
                 FROM ledger_nodes n JOIN ledger_doc_artifacts a ON a.doc_id=n.doc_id
                 WHERE a.repository_root=?1 AND a.path='literal.md'
                   AND n.node_kind IN ('paragraph','fenced_code','table_cell')
                 ORDER BY n.ordinal",
            )
            .unwrap();
        statement
            .query_map([root_text], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert!(rows.iter().any(|row| row.1 == "fenced_code"));
    assert!(rows.iter().any(|row| row.1 == "table_cell"));

    let doc_id: String = db
        .lock()
        .query_row(
            "SELECT doc_id FROM ledger_doc_artifacts WHERE path='literal.md'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let (content_hash, revision, generation): (String, String, i64) = db
        .lock()
        .query_row(
            "SELECT content_hash,revision,index_generation FROM ledger_doc_artifacts WHERE doc_id=?1",
            [&doc_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    for (node_id, kind, start, end) in rows {
        let request = ResolveRequest {
            doc_id: Some(doc_id.clone()),
            node_id: Some(node_id),
            source_ref: "doc://repo/worktree/literal.md".into(),
            anchor_id: "document".into(),
            expected_content_hash: content_hash.clone(),
            expected_revision: Some(revision.clone()),
            expected_span_hash: None,
            ledger_generation: Some(generation),
            continuation_cursor: None,
            max_bytes: 12_000,
        };
        let read = resolve(&db, root.path(), &request).unwrap();
        assert_eq!(read.read.content, &markdown[start as usize..end as usize]);
        assert!(!read.read.content.is_empty(), "empty {kind} projection is not eligible");
    }
    let hit = doc_spine::recall(&db, "CaseSensitive", 1).unwrap().remove(0);
    assert!(hit.source_ref.starts_with("doc://repo/worktree/"));
    assert_eq!(hit.expected_hash, content_hash);
    assert!(resolve(&db, tempfile::tempdir().unwrap().path(), &node_request(&db, &hit)).is_err());
}
