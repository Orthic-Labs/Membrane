use membrane_runtime::ledger::{
    doc_spine,
    index::{
        activate, normalize_query, query_terms, recall_mode, LedgerQualificationReceiptV1,
        LedgerRecallMode, FTS_SCHEMA_VERSION, PROJECTION_SCHEMA_VERSION, QUERY_NORMALIZER_VERSION,
        TOKENIZER_ID,
    },
    LedgerDb,
};

fn passing_receipt() -> LedgerQualificationReceiptV1 {
    LedgerQualificationReceiptV1 {
        receipt_sha256: "a".repeat(64),
        corpus_version: "ledger-index-mechanics-v1".to_owned(),
        exact_resolution_passed: true,
        stale_refusal_passed: true,
        production_fts_path_passed: true,
    }
}

#[test]
fn sync_publishes_source_positioned_nodes_and_fts_in_one_generation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("architecture.md"),
        "# Runtime\n\nintro\n\n## Activation Gate\n\nUse `LedgerDb`.\n",
    )
    .unwrap();
    let db = LedgerDb::open_in_memory();
    let report = doc_spine::sync(&db, root.path()).unwrap();

    let conn = db.lock();
    let rows: Vec<(String, String, i64, String, String, String, String)> = conn
        .prepare(
            "SELECT node_kind, heading, ledger_generation, projection_schema_version,
                    fts_schema_version, tokenizer_id, query_normalizer_version
             FROM ledger_nodes ORDER BY ordinal",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
                row.get(5)?, row.get(6)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(rows.len() >= 2);
    assert_eq!(rows[1].0, "section");
    assert_eq!(rows[1].1, "Activation Gate");
    assert_eq!(rows[1].2, report.index_generation);
    assert_eq!(rows[1].3, PROJECTION_SCHEMA_VERSION);
    assert_eq!(rows[1].4, FTS_SCHEMA_VERSION);
    assert_eq!(rows[1].5, TOKENIZER_ID);
    assert_eq!(rows[1].6, QUERY_NORMALIZER_VERSION);
    let fts_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM ledger_node_fts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(fts_rows, rows.len() as i64);
}

#[test]
fn ast_projection_covers_required_markdown_block_families_with_source_spans() {
    let root = tempfile::tempdir().unwrap();
    let markdown = "# Blocks\n\nparagraph [link](target.md) with note[^note]\n\n> quoted\n\n- item\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\n    indented\n\n<div>html</div>\n\n---\n\n[^note]: footnote\n";
    std::fs::write(root.path().join("blocks.md"), markdown).unwrap();
    let db = LedgerDb::open_in_memory();
    doc_spine::sync(&db, root.path()).unwrap();
    let conn = db.lock();
    let mut statement = conn
        .prepare(
            "SELECT node_kind, source_start_byte, source_end_byte, span_hash
             FROM ledger_nodes WHERE node_kind <> 'section' ORDER BY ordinal",
        )
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let kinds = rows.iter().map(|row| row.0.as_str()).collect::<std::collections::BTreeSet<_>>();
    for expected in [
        "paragraph", "link", "blockquote", "list", "list_item", "table", "table_row",
        "table_cell", "fenced_code", "indented_code", "html_block", "thematic_break",
        "footnote_definition",
    ] {
        assert!(kinds.contains(expected), "missing {expected}: {kinds:?}");
    }
    for (_, start, end, span_hash) in rows {
        assert!(start < end && end as usize <= markdown.len());
        assert_eq!(span_hash.len(), 64);
    }
}

#[test]
fn unicode_and_identifier_queries_never_collapse_to_zero_terms() {
    assert_eq!(normalize_query("ＬｅｄｇｅｒＤｂ"), "ledgerdb");
    for query in ["設計", "文档导航", "LedgerDb", "doc_spine", "src/ledger/doc-spine.rs"] {
        assert!(!query_terms(query).is_empty(), "{query}");
    }
    let terms = query_terms("HTTPServerV2 doc_spine src/ledger/doc-spine.rs");
    for expected in ["httpserverv2", "http", "server", "v2", "doc", "spine", "src", "ledger"] {
        assert!(terms.iter().any(|term| term == expected), "missing {expected}: {terms:?}");
    }
}

#[test]
fn fts_activation_is_fail_closed_and_supports_rollback() {
    let db = LedgerDb::open_in_memory();
    assert_eq!(recall_mode(&db).unwrap(), LedgerRecallMode::LegacyScan);
    assert_eq!(
        activate(&db, LedgerRecallMode::LedgerFts, None).unwrap_err(),
        "ledger_fts_requires_qualification"
    );
    let mut bad = passing_receipt();
    bad.stale_refusal_passed = false;
    assert_eq!(
        activate(&db, LedgerRecallMode::LedgerFts, Some(&bad)).unwrap_err(),
        "ledger_fts_qualification_failed"
    );
    activate(&db, LedgerRecallMode::LedgerFts, Some(&passing_receipt())).unwrap();
    assert_eq!(recall_mode(&db).unwrap(), LedgerRecallMode::LedgerFts);
    activate(&db, LedgerRecallMode::LegacyScan, None).unwrap();
    assert_eq!(recall_mode(&db).unwrap(), LedgerRecallMode::LegacyScan);
}

#[test]
fn activated_production_recall_executes_fts_for_cjk_identifiers_and_escaped_operators() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("ledger-runtime.md"),
        "# 設計文書\n\n文档导航 uses `LedgerDb` and `doc_spine`.\n",
    )
    .unwrap();
    let db = LedgerDb::open_in_memory();
    let first = doc_spine::sync(&db, root.path()).unwrap();
    activate(&db, LedgerRecallMode::LedgerFts, Some(&passing_receipt())).unwrap();

    for query in ["設計", "文档导航", "LedgerDb", "doc_spine", "ledger-runtime"] {
        let hits = doc_spine::recall(&db, query, 3).unwrap();
        assert!(!hits.is_empty(), "no FTS hit for {query}");
        assert_eq!(hits[0].lane, "ledger_fts");
        assert_eq!(hits[0].ledger_generation, Some(first.index_generation));
        assert!(hits[0].node_id.is_some());
    }
    // User operators are quoted data, never executable FTS syntax.
    assert!(doc_spine::recall(&db, "title: OR NEAR(\"", 3).is_ok());

    std::fs::write(root.path().join("ledger-runtime.md"), "# Changed\n").unwrap();
    assert!(
        doc_spine::recall(&db, "LedgerDb", 3).unwrap().is_empty(),
        "hash mismatch must fail stale instead of serving indexed bytes"
    );
}

#[test]
fn unchanged_sync_advances_nodes_with_the_published_artifact_generation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("doc.md"), "# Stable\nbody\n").unwrap();
    let db = LedgerDb::open_in_memory();
    let first = doc_spine::sync(&db, root.path()).unwrap();
    let second = doc_spine::sync(&db, root.path()).unwrap();
    assert!(second.index_generation > first.index_generation);
    let (artifact, node): (i64, i64) = db
        .lock()
        .query_row(
            "SELECT artifact.index_generation, node.ledger_generation
             FROM ledger_doc_artifacts artifact JOIN ledger_nodes node ON node.doc_id=artifact.doc_id
             LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(artifact, second.index_generation);
    assert_eq!(node, second.index_generation);
}
