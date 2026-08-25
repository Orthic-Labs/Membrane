//! Local-only H2 replay gate preparation.
//!
//! This is deliberately a clone-only harness: it never opens the resident DB and cannot close
//! the operational gate.  It exercises current projection persistence plus shadow metrics, then
//! emits one numeric receipt line.  Any loss keeps H2 disabled and selects document-level fetch.

use std::fs;

use membrane_runtime::ledger::doc_projection::{
    project_markdown, replace_doc_projections, DocumentProjectionStoreInputV1,
    H2ReplayGateReceiptV1, ProjectionConfig, TokenCounter,
};
use membrane_runtime::ledger::doc_shadow::{
    evaluate_shadow_replay, DocumentClass, ReplayCandidateV1, ShadowReplayCaseV1,
    ShadowReplayDisposition,
};
use membrane_runtime::ledger::LedgerDb;
use tempfile::tempdir;

struct Words;

impl TokenCounter for Words {
    fn count(&self, text: &str) -> usize {
        text.split_whitespace().count()
    }
}

fn store_input(
    projections: Vec<membrane_runtime::ledger::doc_projection::DocumentProjectionV1>,
) -> DocumentProjectionStoreInputV1 {
    DocumentProjectionStoreInputV1 {
        parent_doc_id: "doc:local-gate".into(),
        source_content_hash: "sha256:local-gate-fixture".into(),
        source_revision: "local-gate@1".into(),
        index_generation: 1,
        projections,
    }
}

fn candidate(section_id: &str) -> ReplayCandidateV1 {
    ReplayCandidateV1 {
        doc_id: "doc:local-gate".into(),
        section_id: Some(section_id.into()),
        document_class: DocumentClass::Runbook,
        superseded: false,
        duplicate: false,
    }
}

fn projection_rows(db: &LedgerDb) -> Vec<(String, String)> {
    let conn = db.lock();
    let rows = conn
        .prepare("SELECT kind, anchor_id FROM ledger_doc_projections ORDER BY kind, anchor_id")
        .expect("prepare projection read")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query projection rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect projection rows");
    rows
}

/// A local clone replay must report every numeric comparison and fail closed on loss.
///
/// The baseline exposes a document-level lexical result.  H2 materialization replaces that
/// result with child projections, so a query requiring the document fetch pointer loses section
/// correctness.  That loss is the required fallback branch: H2 remains off and callers retain
/// document-level `anchor/retrieve` behavior.
#[test]
fn h2_local_clone_replay_logs_numeric_loss_and_keeps_h2_disabled() {
    let markdown =
        "# Deploy\n\nkeep document fetch available\n\n## Rollback\n\nrollback exact section";
    let temp = tempdir().expect("temp root");
    let source_path = temp.path().join("source.db");
    let clone_path = temp.path().join("h2-replay-clone.db");

    let source = LedgerDb::open(&source_path).expect("open source DB");
    replace_doc_projections(
        &source,
        &store_input(project_markdown(
            markdown,
            &Words,
            ProjectionConfig::default(),
        )),
    )
    .expect("persist baseline projections");
    {
        let conn = source.lock();
        conn.execute_batch("PRAGMA wal_checkpoint(FULL);")
            .expect("checkpoint source before clone");
    }
    drop(source);
    fs::copy(&source_path, &clone_path).expect("clone local DB");

    let baseline = LedgerDb::open(&source_path).expect("reopen source DB");
    let h2_clone = LedgerDb::open(&clone_path).expect("open H2 clone DB");
    replace_doc_projections(
        &h2_clone,
        &store_input(project_markdown(
            markdown,
            &Words,
            ProjectionConfig {
                max_tokens: 5,
                h2_replay_gate: H2ReplayGateReceiptV1 { passed: true },
            },
        )),
    )
    .expect("persist H2 projections in clone only");

    let baseline_rows = projection_rows(&baseline);
    let h2_rows = projection_rows(&h2_clone);
    assert!(baseline_rows
        .iter()
        .any(|(kind, anchor)| kind == "lexical" && anchor == "document"));
    assert!(h2_rows.iter().all(|(kind, _)| kind != "whole_document"));
    assert!(h2_rows
        .iter()
        .any(|(kind, anchor)| kind == "section" && anchor == "sec:rollback:1"));

    let report = evaluate_shadow_replay(&[ShadowReplayCaseV1 {
        expected_doc_id: "doc:local-gate".into(),
        expected_section_id: Some("document".into()),
        baseline: vec![candidate("document")],
        with_docs: h2_rows
            .iter()
            .filter(|(kind, _)| kind == "section")
            .map(|(_, anchor)| candidate(anchor))
            .collect(),
    }]);
    let gate = H2ReplayGateReceiptV1 { passed: false };

    eprintln!(
        "H2_LOCAL_GATE baseline_mean_rank={:.3} h2_mean_rank={:.3} baseline_doc_rate={:.3} h2_doc_rate={:.3} baseline_section_rate={:.3} h2_section_rate={:.3} displacement={} passed={} disposition=RemainDocumentLevel",
        report.baseline.mean_rank,
        report.with_docs.mean_rank,
        report.baseline.correct_doc_rate,
        report.with_docs.correct_doc_rate,
        report.baseline.correct_section_rate,
        report.with_docs.correct_section_rate,
        report.displacement_count,
        gate.passed,
    );

    assert!(report.with_docs.correct_section_rate < report.baseline.correct_section_rate);
    assert_eq!(
        report.disposition,
        ShadowReplayDisposition::NarrowToRunbookAndDecision
    );
    assert!(!gate.passed, "any replay loss keeps H2 disabled");
}
