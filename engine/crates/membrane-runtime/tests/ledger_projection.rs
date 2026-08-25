use membrane_runtime::ledger::doc_projection::{
    project_markdown, replace_doc_projections, DocumentProjectionStoreInputV1,
    H2ReplayGateReceiptV1, ProjectionConfig, ProjectionKind, TokenCounter,
};
use membrane_runtime::ledger::LedgerDb;

struct Words;

impl TokenCounter for Words {
    fn count(&self, text: &str) -> usize {
        text.split_whitespace().count()
    }
}

#[test]
fn lexical_projection_is_always_emitted() {
    let projections = project_markdown("# Title\n\nbody", &Words, ProjectionConfig::default());

    assert_eq!(projections[0].kind, ProjectionKind::Lexical);
    assert_eq!(projections[0].content, "# Title\n\nbody");
}

#[test]
fn tokenizer_measured_whole_document_fit_is_emitted_before_sections() {
    let projections = project_markdown(
        "# Title\n\none two three",
        &Words,
        ProjectionConfig {
            max_tokens: 5,
            ..ProjectionConfig::default()
        },
    );

    assert_eq!(projections.len(), 2);
    assert_eq!(projections[1].kind, ProjectionKind::WholeDocument);
    assert_eq!(projections[1].token_count, 5);
}

#[test]
fn oversized_document_cascades_at_structural_boundaries_without_splitting_fences() {
    let markdown = "# First\n\none two\n\n```md\n# Not a section\n```\n\n# Second\n\nthree four";
    let projections = project_markdown(
        markdown,
        &Words,
        ProjectionConfig {
            max_tokens: 5,
            ..ProjectionConfig::default()
        },
    );

    let structural = &projections[1..];
    assert!(structural
        .iter()
        .all(|projection| projection.kind == ProjectionKind::Section));
    assert_eq!(
        structural
            .iter()
            .filter(|projection| projection.content.contains("# Not a section"))
            .count(),
        1
    );
    assert!(structural
        .iter()
        .find(|projection| projection.content.contains("# Not a section"))
        .expect("fence projection")
        .content
        .contains("```md\n# Not a section\n```"));
}

#[test]
fn h2_replay_gate_defaults_closed() {
    assert!(!H2ReplayGateReceiptV1::default().passed);
}

#[test]
fn h2_split_is_disabled_without_a_passing_replay_gate() {
    let markdown = "# Parent\n\nintro\n\n## Child\n\nbody";
    let projections = project_markdown(
        markdown,
        &Words,
        ProjectionConfig {
            max_tokens: 2,
            ..ProjectionConfig::default()
        },
    );

    assert!(projections[1..]
        .iter()
        .all(|projection| !projection.provenance.anchor_id.starts_with("sec:child:1")));
}

#[test]
fn h2_split_records_collapsed_parent_provenance_after_replay_gate_passes() {
    let markdown = "# Parent\n\nintro\n\n## Child\n\nbody";
    let projections = project_markdown(
        markdown,
        &Words,
        ProjectionConfig {
            max_tokens: 3,
            h2_replay_gate: H2ReplayGateReceiptV1 { passed: true },
        },
    );

    assert_eq!(projections.len(), 3);
    assert_eq!(projections[1].provenance.anchor_id, "sec:parent:1");
    assert_eq!(projections[2].provenance.anchor_id, "sec:child:1");
    assert_eq!(
        projections[2].provenance.collapsed_to_parent.as_deref(),
        Some("sec:parent:1")
    );
}

#[test]
fn oversized_h1_cascades_to_child_heading_then_paragraphs_without_splitting_fences() {
    let markdown = "# Parent\n\nintro one two three\n\n## Child\n\nchild one two three\n\n```rust\nlet example = \"keep this fence whole\";\n```\n\nclosing one two three";
    let projections = project_markdown(
        markdown,
        &Words,
        ProjectionConfig {
            max_tokens: 4,
            h2_replay_gate: H2ReplayGateReceiptV1 { passed: true },
        },
    );

    let structural = &projections[1..];
    assert!(structural.iter().any(|projection| {
        projection.provenance.anchor_id == "sec:child:1"
            || projection
                .provenance
                .anchor_id
                .starts_with("sec:child:1:part:")
    }));
    let fence = structural
        .iter()
        .find(|projection| projection.content.contains("keep this fence whole"))
        .expect("fence projection");
    assert!(fence
        .content
        .contains("```rust\nlet example = \"keep this fence whole\";\n```"));
    assert_eq!(
        structural
            .iter()
            .filter(|projection| projection.content.contains("keep this fence whole"))
            .count(),
        1
    );
}

#[test]
fn projection_store_persists_lexical_whole_document_and_provenance() {
    let db = LedgerDb::open_in_memory();
    let input = DocumentProjectionStoreInputV1 {
        parent_doc_id: "doc:runbook".into(),
        source_content_hash: "sha256:content-v1".into(),
        source_revision: "revision-v1".into(),
        index_generation: 7,
        projections: project_markdown(
            "# Runbook\n\nrestart service",
            &Words,
            ProjectionConfig {
                max_tokens: 10,
                ..ProjectionConfig::default()
            },
        ),
    };

    replace_doc_projections(&db, &input).unwrap();

    let conn = db.lock();
    let mut statement = conn
        .prepare("SELECT kind, parent_doc_id, content, token_count, anchor_id, collapsed_to_parent, source_content_hash, source_revision, index_generation FROM ledger_doc_projections ORDER BY kind")
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "lexical");
    assert_eq!(rows[0].1, "doc:runbook");
    assert_eq!(rows[0].4, "document");
    assert_eq!(rows[0].6, "sha256:content-v1");
    assert_eq!(rows[0].7, "revision-v1");
    assert_eq!(rows[0].8, 7);
    assert_eq!(rows[1].0, "whole_document");
}

#[test]
fn projection_store_replaces_stale_parent_rows_atomically() {
    let db = LedgerDb::open_in_memory();
    let initial = DocumentProjectionStoreInputV1 {
        parent_doc_id: "doc:guide".into(),
        source_content_hash: "sha256:old".into(),
        source_revision: "old-revision".into(),
        index_generation: 1,
        projections: project_markdown(
            "# Parent\n\nintro\n\n## Child\n\nbody",
            &Words,
            ProjectionConfig {
                max_tokens: 3,
                h2_replay_gate: H2ReplayGateReceiptV1 { passed: true },
            },
        ),
    };
    replace_doc_projections(&db, &initial).unwrap();

    let replacement = DocumentProjectionStoreInputV1 {
        parent_doc_id: "doc:guide".into(),
        source_content_hash: "sha256:new".into(),
        source_revision: "new-revision".into(),
        index_generation: 2,
        projections: project_markdown(
            "# Replacement\n\nsmall",
            &Words,
            ProjectionConfig::default(),
        ),
    };
    replace_doc_projections(&db, &replacement).unwrap();

    let conn = db.lock();
    let rows = conn
        .prepare("SELECT kind, anchor_id, source_content_hash, source_revision, index_generation FROM ledger_doc_projections WHERE parent_doc_id=?1 ORDER BY kind")
        .unwrap()
        .query_map(["doc:guide"], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(
        rows,
        vec![
            (
                "lexical".into(),
                "document".into(),
                "sha256:new".into(),
                "new-revision".into(),
                2
            ),
            (
                "whole_document".into(),
                "document".into(),
                "sha256:new".into(),
                "new-revision".into(),
                2
            ),
        ]
    );
}

#[test]
fn projection_store_rolls_back_stale_removal_when_replacement_is_invalid() {
    let db = LedgerDb::open_in_memory();
    let initial = DocumentProjectionStoreInputV1 {
        parent_doc_id: "doc:atomic".into(),
        source_content_hash: "sha256:old".into(),
        source_revision: "old-revision".into(),
        index_generation: 1,
        projections: project_markdown("# Old\n\nbody", &Words, ProjectionConfig::default()),
    };
    replace_doc_projections(&db, &initial).unwrap();

    let mut duplicate_projections =
        project_markdown("# New\n\nbody", &Words, ProjectionConfig::default());
    duplicate_projections.push(duplicate_projections[0].clone());
    let invalid = DocumentProjectionStoreInputV1 {
        parent_doc_id: "doc:atomic".into(),
        source_content_hash: "sha256:new".into(),
        source_revision: "new-revision".into(),
        index_generation: 2,
        projections: duplicate_projections,
    };

    assert!(replace_doc_projections(&db, &invalid).is_err());

    let conn = db.lock();
    let rows = conn
        .prepare("SELECT kind, source_content_hash, source_revision, index_generation FROM ledger_doc_projections WHERE parent_doc_id=?1 ORDER BY kind")
        .unwrap()
        .query_map(["doc:atomic"], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            (
                "lexical".into(),
                "sha256:old".into(),
                "old-revision".into(),
                1
            ),
            (
                "whole_document".into(),
                "sha256:old".into(),
                "old-revision".into(),
                1
            ),
        ]
    );
}

#[test]
fn projection_store_preserves_section_parent_provenance() {
    let db = LedgerDb::open_in_memory();
    let input = DocumentProjectionStoreInputV1 {
        parent_doc_id: "doc:parent".into(),
        source_content_hash: "sha256:source".into(),
        source_revision: "revision".into(),
        index_generation: 4,
        projections: project_markdown(
            "# Parent\n\nintro\n\n## Child\n\nbody",
            &Words,
            ProjectionConfig {
                max_tokens: 3,
                h2_replay_gate: H2ReplayGateReceiptV1 { passed: true },
            },
        ),
    };

    replace_doc_projections(&db, &input).unwrap();

    let conn = db.lock();
    let (parent_doc_id, anchor_id, collapsed_to_parent): (String, String, Option<String>) = conn
        .query_row(
            "SELECT parent_doc_id, anchor_id, collapsed_to_parent FROM ledger_doc_projections WHERE kind='section' AND anchor_id='sec:child:1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(parent_doc_id, "doc:parent");
    assert_eq!(anchor_id, "sec:child:1");
    assert_eq!(collapsed_to_parent.as_deref(), Some("sec:parent:1"));
}
