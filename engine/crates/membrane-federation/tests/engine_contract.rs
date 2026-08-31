use membrane_federation::config::{FederationConfig, ProviderConfig};
use membrane_federation::corrective::{
    corrective_plan, corrective_trigger, evaluate_sufficiency, CorrectiveRetrievalReceiptV1,
    SufficiencyContractV1, SufficiencyRequirementV1, SufficiencyStateV1,
    SUFFICIENCY_CONTRACT_SCHEMA_VERSION, SUFFICIENCY_POLICY,
};
use membrane_federation::engine::FederationMetrics;
use membrane_federation::merge::{merge_outputs, merge_outputs_with_strategy, FusionStrategy};
use membrane_federation::normalize::normalize_provider_output;
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
            source_hash: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .into(),
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
fn active_merge_emits_versioned_fusion_receipt() {
    let result = merge_outputs(
        &[ProviderId::Anchors, ProviderId::Blueprint],
        &[output(ProviderId::Anchors, "anchor-1")],
        None,
    )
    .expect("valid provider lane");
    assert_eq!(
        result.fusion_receipt.policy,
        membrane_protocol::FusionReceiptV1::RRF_POLICY
    );
    assert!(result
        .fusion_receipt
        .decisions
        .iter()
        .any(|decision| decision.decision == "selected"));
    let response = result.response("request", "trace");
    assert_eq!(
        response.extensions["fusionReceipt"]["policy"].as_str(),
        Some(membrane_protocol::FusionReceiptV1::RRF_POLICY)
    );
}

#[test]
fn rrf_is_production_default_and_fixed_order_remains_explicit_control() {
    let outputs = [
        output(ProviderId::Anchors, "anchor-1"),
        output(ProviderId::Blueprint, "blueprint-1"),
    ];
    let active = merge_outputs(
        &[ProviderId::Anchors, ProviderId::Blueprint],
        &outputs,
        None,
    )
    .expect("active merge");
    assert_eq!(
        active.fusion_receipt.policy,
        membrane_protocol::FusionReceiptV1::RRF_POLICY
    );
    assert_eq!(active.candidates.len(), 1);
    let control = merge_outputs_with_strategy(
        &[ProviderId::Anchors, ProviderId::Blueprint],
        &outputs,
        None,
        FusionStrategy::FixedOrder,
    )
    .expect("explicit fixed-order merge");
    assert_eq!(
        control.fusion_receipt.policy,
        membrane_protocol::FusionReceiptV1::POLICY
    );
    assert_eq!(control.candidates.len(), 2);
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

#[test]
fn corrective_plan_runs_one_deterministic_acceptable_alternate() {
    let contract = SufficiencyContractV1 {
        schema_version: SUFFICIENCY_CONTRACT_SCHEMA_VERSION,
        policy: SUFFICIENCY_POLICY.to_owned(),
        requirements: vec![SufficiencyRequirementV1 {
            id: "repo-evidence".to_owned(),
            evidence_class: "fixture".to_owned(),
            acceptable_providers: vec![ProviderId::Blueprint, ProviderId::Cortex],
            acceptable_source_refs: Vec::new(),
            minimum_candidates: 1,
        }],
        max_corrective_stages: 1,
    };
    let blueprint = normalize_provider_output(
        &ProviderOutputV1 {
            provider: ProviderId::Blueprint,
            status: FederationProviderStatusV1::Complete,
            candidates: Vec::new(),
            ..output(ProviderId::Blueprint, "unused")
        },
        ProviderId::Blueprint,
    )
    .expect("blueprint lane normalizes");
    let cortex = normalize_provider_output(
        &ProviderOutputV1 {
            provider: ProviderId::Cortex,
            status: FederationProviderStatusV1::Complete,
            candidates: Vec::new(),
            ..output(ProviderId::Cortex, "unused")
        },
        ProviderId::Cortex,
    )
    .expect("cortex lane normalizes");
    let lanes = vec![blueprint, cortex];
    let assessment = evaluate_sufficiency(
        &contract,
        &lanes,
        &[ProviderId::Blueprint, ProviderId::Cortex],
    )
    .expect("contract evaluates");
    assert_eq!(assessment.state, SufficiencyStateV1::Insufficient);
    assert_eq!(
        corrective_trigger(
            &contract,
            &assessment,
            &lanes,
            &[ProviderId::Blueprint, ProviderId::Cortex],
        ),
        Some((ProviderId::Blueprint, "repo-evidence".to_owned()))
    );
    assert_eq!(
        corrective_plan(
            &contract,
            &assessment,
            &lanes,
            &[ProviderId::Blueprint, ProviderId::Cortex],
        ),
        Some((
            ProviderId::Blueprint,
            ProviderId::Cortex,
            "repo-evidence".to_owned(),
        ))
    );
}

#[test]
fn corrective_receipt_types_second_insufficiency_without_retry() {
    let assessment = membrane_federation::corrective::SufficiencyAssessmentV1 {
        schema_version: SUFFICIENCY_CONTRACT_SCHEMA_VERSION,
        policy: SUFFICIENCY_POLICY.to_owned(),
        state: SufficiencyStateV1::Insufficient,
        requirements: Vec::new(),
    };
    let receipt = CorrectiveRetrievalReceiptV1::after_stage(
        assessment,
        ProviderId::Blueprint,
        ProviderId::Cortex,
        "repo-evidence".to_owned(),
        true,
        "terminal_insufficient_second_assessment",
    );
    assert!(receipt.triggered);
    assert!(receipt.attempted);
    assert_eq!(receipt.stage_limit, 1);
    assert_eq!(receipt.outcome, "terminal_insufficient_second_assessment");
}
