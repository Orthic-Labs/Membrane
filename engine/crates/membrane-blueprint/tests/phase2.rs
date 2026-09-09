use membrane_blueprint::phase2::{fingerprint, seal_phase2_artifacts, validate_verdict_metadata, DerivationMetadata, DimensionMetadata, IncrementalUnderstanding, InvalidationMetadata, Phase2Queue, Phase2SealError, Phase2Verdict, SupersessionMetadata, Understanding, VerificationMetadata, VerdictEnvelope};
use membrane_blueprint::GraphGeneration;
use serde_json::json;

fn sealed_generation() -> GraphGeneration {
    GraphGeneration {
        schema_version: 1,
        provider: "test".into(),
        provider_version: "1".into(),
        generation_id: "generation-seal".into(),
        source_hash: "hash".into(),
        repo_root: "/repo".into(),
        complete: true,
        nodes: vec![],
        edges: vec![],
        files: vec![],
        truncation_reasons: vec![],
    }
}

fn understanding_with_dimension_metadata(meta: DimensionMetadata) -> Understanding {
    let mut dimensions = std::collections::BTreeMap::new();
    for dimension in ["architecture", "interfaces", "health", "contract", "security", "solid"] {
        let m = if dimension == "architecture" { meta.clone() } else { DimensionMetadata { input_files: vec!["src/a.rs".into()], ..Default::default() } };
        dimensions.insert(dimension.to_owned(), m);
    }
    Understanding {
        architecture: Some(json!({"ok": true})),
        interfaces: Some(json!({"ok": true})),
        health: Some(json!({"ok": true})),
        contract: Some(json!({"ok": true})),
        security: Some(json!({"ok": true})),
        solid: Some(json!({"ok": true})),
        source_generation_id: None,
        incremental: Some(IncrementalUnderstanding { schema_version: None, dimensions }),
    }
}

#[test]
fn fingerprints_are_stable_for_object_key_order() {
    let a = serde_json::json!({"b": 2, "a": 1});
    let b = serde_json::json!({"a": 1, "b": 2});
    assert_eq!(fingerprint(&a), fingerprint(&b));
}

#[test]
fn empty_dimension_dependencies_cannot_be_sealed() {
    let graph = sealed_generation();
    let queue = Phase2Queue::default();
    let envelope = VerdictEnvelope::default();
    // The "architecture" dimension declares no input files and no input verdict ids.
    let understanding = understanding_with_dimension_metadata(DimensionMetadata::default());

    let result = seal_phase2_artifacts(&graph, &queue, &envelope, &understanding);

    let err = result.expect_err("sealing must reject a dimension that declares no dependencies");
    let Phase2SealError::Invalid(message) = err;
    assert!(
        message.contains("architecture") && message.contains("no dependencies"),
        "expected a no-dependencies rejection for 'architecture', got: {message}"
    );
}

#[test]
fn non_empty_dimension_dependencies_are_not_rejected_for_missing_dependencies() {
    let graph = sealed_generation();
    let queue = Phase2Queue::default();
    let envelope = VerdictEnvelope::default();
    // Every dimension, including "architecture", declares at least one dependency, and the
    // declared file is present in the graph's file inventory (empty here, so declaring a file
    // that is absent from `graph_file_hashes` still exercises the "missing dependencies" path,
    // not the "no dependencies" path this test guards against regressing).
    let understanding = understanding_with_dimension_metadata(DimensionMetadata { input_files: vec!["src/a.rs".into()], ..Default::default() });

    let result = seal_phase2_artifacts(&graph, &queue, &envelope, &understanding);

    let err = result.expect_err("declared file is absent from the graph, so sealing must still fail");
    let Phase2SealError::Invalid(message) = err;
    assert!(
        !message.contains("no dependencies"),
        "a dimension with declared dependencies must not be rejected as having none, got: {message}"
    );
    assert!(message.contains("missing dependencies"), "expected a missing-dependencies rejection, got: {message}");
}

#[test]
fn seal_metadata_is_required_and_preserved_shape_is_typed() {
    let verdict = Phase2Verdict { claim_id: "claim".into(), ..Default::default() };
    assert!(validate_verdict_metadata(&verdict, "gen:1").is_err());
    let verdict = Phase2Verdict { claim_id: "claim".into(), derivation: Some(DerivationMetadata { method: "judgment".into(), evidence_refs: vec![serde_json::json!("src/a.ts:1")], fingerprint: "xxh128:f".into(), provider: "none".into(), model: "none".into(), version: "n/a".into() }), verification: Some(VerificationMetadata { status: "verified".into(), confidence: serde_json::json!(0.8) }), invalidation: Some(InvalidationMetadata { generation_id: "gen:1".into(), reason: "source-change".into() }), supersession: Some(SupersessionMetadata { state: "current".into(), superseded_by: None }), ..Default::default() };
    assert!(validate_verdict_metadata(&verdict, "gen:1").is_ok());
    assert_eq!(verdict.derivation.as_ref().unwrap().method, "judgment");
}
