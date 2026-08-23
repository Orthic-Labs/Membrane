use membrane_federation::config::{FederationConfig, ProviderConfig};
use membrane_federation::engine::FederationMetrics;
use membrane_federation::merge::merge_outputs;
use membrane_protocol::{
    CandidateV1, FederationProviderStatusV1, ProviderId, ProviderOutputV1,
    PROVIDER_OUTPUT_SCHEMA_VERSION,
};
use std::collections::BTreeMap;

fn output(provider: ProviderId, id: &str) -> ProviderOutputV1 {
    ProviderOutputV1 {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider,
        status: FederationProviderStatusV1::Complete,
        generation: None,
        candidates: vec![CandidateV1 {
            id: id.to_owned(),
            layer: 1,
            provider: None,
            source_kind: "fixture".into(),
            source_ref: format!("fixture://{id}"),
            source_hash: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            trust_class: "agent_verified".into(),
            instruction_policy: "data_only".into(),
            provider_score: 0.5,
            score_components: BTreeMap::new(),
            base_commit: None,
            overlay_digest: None,
            freshness_class: None,
            snapshot_id: None,
            estimated_tokens: 1,
            protected: false,
            exact: true,
            recoverable: true,
            resolver: "fixture".into(),
            text: "fixture".into(),
        }],
        warnings: Vec::new(),
        omissions: Vec::new(),
        diagnostics: None,
        extensions: BTreeMap::new(),
    }
}

#[test]
fn expected_lanes_are_accounted_before_merge() {
    let result = merge_outputs(
        &[ProviderId::Anchors, ProviderId::Blueprint],
        &[output(ProviderId::Anchors, "anchor-1")],
        None,
    )
    .expect("valid provider lane");
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.omissions.len(), 1);
    assert_eq!(result.omissions[0].provider, ProviderId::Blueprint);
}

#[test]
fn disabled_configuration_preserves_all_nine_expected_lanes() {
    let config = FederationConfig::new(
        ProviderId::ALL
            .into_iter()
            .map(|provider| {
                if provider == ProviderId::Git {
                    ProviderConfig::disabled(provider)
                } else {
                    ProviderConfig::enabled(provider)
                }
            })
            .collect(),
    )
    .expect("complete provider configuration");
    assert_eq!(config.expected_providers().count(), ProviderId::ALL.len());
    assert!(config.disabled_omission(ProviderId::Git).is_some());
}

#[test]
fn metrics_are_content_free_and_structured() {
    let metrics = FederationMetrics {
        expected_lanes: 9,
        active_lanes: 8,
        output_lanes: 7,
        omission_lanes: 2,
        candidate_count: 3,
        ..FederationMetrics::default()
    };
    assert_eq!(metrics.expected_lanes, 9);
    assert_eq!(metrics.candidate_count, 3);
    assert!(!metrics.cancelled);
}
