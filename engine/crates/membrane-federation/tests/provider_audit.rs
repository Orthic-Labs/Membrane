use membrane_federation::providers::audit::normalize_audit_finding;
use membrane_protocol::CandidateV1;
use membrane_provider_sdk::AuditFinding;
use std::collections::BTreeMap;

fn finding() -> AuditFinding {
    let hash = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    AuditFinding {
        id: "audit:one".into(),
        repository_id: "repo-a".into(),
        generation: "audit-gen-1".into(),
        source_hash: hash.into(),
        candidate: CandidateV1 {
            id: "audit:one".into(),
            layer: 4,
            provider: None,
            source_kind: "audit_finding".into(),
            source_ref: "audit:one".into(),
            source_hash: hash.into(),
            trust_class: "agent_verified".into(),
            instruction_policy: "data_only".into(),
            provider_score: 0.8,
            score_components: BTreeMap::new(),
            base_commit: None,
            overlay_digest: None,
            freshness_class: None,
            snapshot_id: None,
            estimated_tokens: 20,
            protected: false,
            exact: true,
            recoverable: true,
            resolver: "audit finding".into(),
            text: "finding text".into(),
        },
    }
}

#[test]
fn preserves_identity_hash_trust_and_exactness() {
    let candidate =
        normalize_audit_finding(&finding(), "repo-a", Some("audit-gen-1")).expect("valid finding");
    assert_eq!(candidate.id, "audit:one");
    assert_eq!(
        candidate.source_hash,
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );
    assert_eq!(candidate.trust_class, "agent_verified");
    assert!(candidate.exact);
    assert_eq!(candidate.provider.as_deref(), Some("audit"));
}

#[test]
fn rejects_cross_repository_generation_and_hash_drift() {
    assert!(normalize_audit_finding(&finding(), "other-repo", Some("audit-gen-1")).is_err());
    assert!(normalize_audit_finding(&finding(), "repo-a", Some("audit-gen-2")).is_err());
    let mut invalid = finding();
    invalid.candidate.source_hash =
        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into();
    assert!(normalize_audit_finding(&invalid, "repo-a", Some("audit-gen-1")).is_err());
}
