use membrane_federation::providers::cortex::normalize_memory_candidate;
use membrane_protocol::CandidateV1;
use membrane_provider_sdk::MemoryCandidate;
use std::collections::BTreeMap;

fn record() -> MemoryCandidate {
    MemoryCandidate {
        id: "memory::global/rule".into(),
        repository_id: "repo-a".into(),
        generation: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .into(),
        source_hash: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .into(),
        candidate: CandidateV1 {
            id: "memory::global/rule".into(),
            layer: 7,
            provider: None,
            source_kind: "memory".into(),
            source_ref: "memory::global/rule".into(),
            source_hash: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .into(),
            trust_class: "agent_verified".into(),
            instruction_policy: "data_only".into(),
            provider_score: 0.8,
            score_components: BTreeMap::new(),
            base_commit: None,
            overlay_digest: None,
            freshness_class: None,
            snapshot_id: None,
            estimated_tokens: 12,
            protected: false,
            exact: false,
            recoverable: true,
            resolver: "cortex memory-candidates".into(),
            text: "durable rule".into(),
        },
    }
}
#[test]
fn stamps_cortex_without_rewriting_memory_semantics() {
    let normalized = normalize_memory_candidate(&record(), "repo-a").expect("valid source record");
    assert_eq!(normalized.provider.as_deref(), Some("cortex"));
    assert_eq!(normalized.layer, 7);
    assert_eq!(normalized.source_kind, "memory");
    assert_eq!(normalized.instruction_policy, "data_only");
    assert!(!normalized.protected);
    assert!(!normalized.exact);
    assert!(normalized.recoverable);
    assert_eq!(normalized.provider_score, 0.8);
}

#[test]
fn rejects_cross_repository_or_conflicting_source_identity() {
    assert!(normalize_memory_candidate(&record(), "other-repo").is_err());

    let mut mismatched_hash = record();
    mismatched_hash.candidate.source_hash =
        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into();
    assert!(normalize_memory_candidate(&mismatched_hash, "repo-a").is_err());

    let mut mismatched_id = record();
    mismatched_id.candidate.id = "memory::global/other".into();
    assert!(normalize_memory_candidate(&mismatched_id, "repo-a").is_err());
}
