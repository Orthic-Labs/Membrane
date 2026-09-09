use membrane_blueprint::doc_truth::{project_document_truth, DocumentClaim, TruthEdge};

#[test]
fn deterministic_observation_has_null_public_confidence() {
    let result = project_document_truth(&[DocumentClaim { id: "c".into(), edges: vec![TruthEdge { kind: "supports".into(), confidence_class: Some("EXTRACTED".into()), confidence: Some(1.0), ..Default::default() }], ..Default::default() }], &[], Some("gen:1".into()), "fresh");
    assert_eq!(result.claims[0].grounding, "direct");
    assert!(result.claims[0].observed[0].confidence.is_none());
    assert_eq!(result.claims[0].confidence, Some(serde_json::Value::Null));
}

#[test]
fn conflicting_observations_are_ambiguous() {
    let result = project_document_truth(&[DocumentClaim { id: "c".into(), edges: vec![TruthEdge { kind: "supports".into(), ..Default::default() }, TruthEdge { kind: "contradicts".into(), ..Default::default() }], ..Default::default() }], &[], None, "fresh");
    assert_eq!(result.claims[0].grounding, "ambiguous");
    assert!(result.claims[0].mismatch.present);
}
