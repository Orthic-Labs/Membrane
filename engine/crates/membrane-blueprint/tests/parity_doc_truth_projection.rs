//! Parity tests proving `src/doc_truth.rs` (an existing native port) matches
//! `blueprint/src/graph/doc-truth-projection.mjs`, porting the pure-logic
//! cases from `blueprint/tests/doc-truth-projection.test.mjs`. The legacy
//! file's fourth case (`schema 20 makes claim_code_edges confidence
//! nullable...`) exercises SQLite schema migration via
//! `store-sqlite.mjs`/`confidence-migration.mjs` and has no bearing on the
//! `projectDocumentTruth` logic itself, so it is intentionally not ported
//! here.

use membrane_blueprint::doc_truth::{project_document_truth, DocumentClaim, TruthEdge};
use serde_json::Value;

fn edge(kind: &str, confidence_class: &str, confidence: Option<f64>) -> TruthEdge {
    TruthEdge {
        kind: kind.to_string(),
        source: Some("doc:a".to_string()),
        target: Some("file:src/a.ts".to_string()),
        reason: Some(format!("{kind} reason")),
        confidence_class: Some(confidence_class.to_string()),
        confidence,
        evidence_doc_path: Some("docs/a.md".to_string()),
        evidence_doc_line: Some(4),
        evidence_doc_sha1: Some("doc-hash".to_string()),
        evidence_code_path: Some("src/a.ts".to_string()),
        evidence_code_node_id: Some("file:src/a.ts".to_string()),
        evidence_code_content_hash: Some("code-hash".to_string()),
    }
}

fn deterministic_edge(kind: &str) -> TruthEdge {
    edge(kind, "DETERMINISTIC_EXTRACTION", None)
}

fn claim(id: &str, edges: Vec<TruthEdge>) -> DocumentClaim {
    DocumentClaim {
        id: id.to_string(),
        document_id: Some("doc:a".to_string()),
        source: Some("docs/a.md".to_string()),
        line: Some(4),
        status: Some("implemented".to_string()),
        sha1: Some("doc-hash".to_string()),
        edges,
    }
}

#[test]
fn doc_truth_preserves_declaration_and_observed_evidence_with_deterministic_null_confidence() {
    let projection = project_document_truth(&[claim("c1", vec![deterministic_edge("supports")])], &[], Some("g1".to_string()), "fresh");
    let c = &projection.claims[0];
    assert_eq!(c.grounding, "direct");
    assert_eq!(c.declared.status, "implemented");
    assert_eq!(c.observed[0].confidence, None);
    assert!(!c.mismatch.present);
    let kinds: Vec<&str> = c.citations.iter().map(|cit| cit.kind.as_str()).collect();
    assert_eq!(kinds, vec!["document", "code"]);
}

#[test]
fn doc_truth_emits_contradicted_ambiguous_unsupported_and_stale() {
    assert_eq!(
        project_document_truth(&[claim("c", vec![deterministic_edge("contradicts")])], &[], None, "fresh").claims[0].grounding,
        "contradicted"
    );
    assert_eq!(
        project_document_truth(&[claim("c", vec![deterministic_edge("supports"), deterministic_edge("contradicts")])], &[], None, "fresh").claims[0]
            .grounding,
        "ambiguous"
    );
    assert_eq!(project_document_truth(&[claim("c", vec![])], &[], None, "fresh").claims[0].grounding, "unsupported");
    assert_eq!(
        project_document_truth(&[claim("c", vec![deterministic_edge("supports")])], &[], None, "changed_since_generation").claims[0].grounding,
        "stale"
    );
}

#[test]
fn heuristic_doc_relation_may_retain_inferential_confidence() {
    let projected = project_document_truth(
        &[claim("c", vec![edge("supersedes", "HEURISTIC_BRIDGE", Some(0.9))])],
        &[],
        None,
        "fresh",
    );
    assert_eq!(projected.claims[0].grounding, "indirect");
    assert_eq!(projected.claims[0].observed[0].confidence, Some(0.9));
}

#[test]
fn empty_supersedes_and_counts_shape_match_schema() {
    let projection = project_document_truth(&[claim("c1", vec![deterministic_edge("supports")])], &[], Some("g1".to_string()), "fresh");
    assert_eq!(projection.schema_version, 1);
    assert_eq!(projection.kind, "document-truth-grounding");
    assert_eq!(projection.generation_id.as_deref(), Some("g1"));
    assert_eq!(projection.freshness, "fresh");
    assert_eq!(projection.supersedes, Vec::<Value>::new());
    assert_eq!(*projection.counts.get("direct").unwrap(), 1);
}
