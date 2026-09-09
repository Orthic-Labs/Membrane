use membrane_blueprint::contracts::*;
use membrane_blueprint::phase2::{
    validate_verdict_metadata, DerivationMetadata, InvalidationMetadata, Phase2Verdict,
    SupersessionMetadata, VerificationMetadata,
};
use serde_json::json;

fn sealable_verdict(generation_id: &str) -> Phase2Verdict {
    Phase2Verdict {
        claim_id: "claim-1".into(),
        derivation: Some(DerivationMetadata {
            method: "static-analysis".into(),
            evidence_refs: vec![json!({"path": "src/lib.rs"})],
            fingerprint: "xxh128:0".into(),
            provider: "anthropic".into(),
            model: "claude-sonnet".into(),
            version: "1".into(),
        }),
        verification: Some(VerificationMetadata { status: "verified".into(), confidence: json!(0.9) }),
        invalidation: Some(InvalidationMetadata { generation_id: generation_id.into(), reason: "initial".into() }),
        supersession: Some(SupersessionMetadata { state: "current".into(), superseded_by: None }),
        ..Default::default()
    }
}

// BM05 / Z03: a DerivationMetadata record lacking provider/model/version must be rejected before
// it can be sealed as queryable derivation evidence. Metadata validity is not semantic
// verification -- this proves only that the record is well-formed and attributable, never that
// the underlying claim content is true.
#[test]
fn seal_rejects_derivation_metadata_missing_provider_model_version() {
    let mut verdict = sealable_verdict("gen-1");
    // Well-formed on every other axis, so this isolates the provider/model/version gap.
    assert!(validate_verdict_metadata(&verdict, "gen-1").is_ok());

    if let Some(d) = verdict.derivation.as_mut() {
        d.provider = String::new();
        d.model = String::new();
        d.version = String::new();
    }
    let err = validate_verdict_metadata(&verdict, "gen-1").expect_err("empty provider/model/version must fail sealing");
    assert!(err.to_string().contains("provider/model/version"), "unexpected error: {err}");
}

#[test]
fn v1_contracts_preserve_wire_names_and_required_status_fields() {
    let status = RepositoryStatusV1 { schema_version: 1, state: "fresh".into(), artifacts: Default::default(), stats: Default::default(), errors: vec![], warnings: vec![], reasons: vec![], capabilities: Default::default(), details: None, properties: None, extensions: None };
    let value = serde_json::to_value(status).unwrap();
    for field in ["schemaVersion","state","artifacts","stats","errors","warnings","reasons","capabilities"] { assert!(value.get(field).is_some(), "missing {field}"); }
    assert_eq!(value["state"], "fresh");
}

#[test]
fn scope_grant_rejects_bad_signature_and_round_trips() {
    let grant = ScopeGrantV1 { task_id:"task-01".into(), repo_root:"/repo".into(), generation_id:None, receipt_id:"r".into(), paths:vec!["src/**".into()], issued_ms:1, ttl_ms:1, signature:"a".repeat(64) };
    assert!(grant.validate().is_ok());
    assert!(serde_json::from_value::<ScopeGrantV1>(json!({"taskId":"x"})).is_err());

    let mut wire = serde_json::to_value(&grant).unwrap();
    wire.as_object_mut().unwrap().remove("generationId");
    assert!(serde_json::from_value::<ScopeGrantV1>(wire).is_err());
    assert_eq!(serde_json::from_value::<ScopeGrantV1>(serde_json::to_value(grant).unwrap()).unwrap().generation_id, None);
}
