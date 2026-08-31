//! Qualification harness for bounded experimental RRF fusion.
//!
//! This harness intentionally measures mechanics only. Fixed order remains
//! qualified default; held-out quality claims require root-run
//! operational evidence and are represented as unavailable in the fixture.

use membrane_federation::merge::{merge_outputs, merge_outputs_with_strategy, FusionStrategy};
use membrane_protocol::{
    CandidateV1, FederationProviderStatusV1, ProviderId, ProviderOutputV1,
    PROVIDER_OUTPUT_SCHEMA_VERSION,
};
use serde::Deserialize;
use std::collections::BTreeMap;

const FIXTURE: &str = include_str!("fixtures/fusion-qualification.v1.json");
const FIXTURE_GENERATION: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Corpus {
    schema_version: u32,
    control_policy: String,
    candidate_strategy: String,
    corpora: Vec<CorpusSet>,
    operational_metrics: OperationalMetrics,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorpusSet {
    name: String,
    frozen: bool,
    cases: Vec<QualificationCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QualificationCase {
    id: String,
    required_providers: Vec<String>,
    providers: Vec<FixtureProvider>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureProvider {
    provider: String,
    #[serde(default)]
    candidates: Vec<FixtureCandidate>,
}

#[derive(Debug, Deserialize)]
struct FixtureCandidate {
    id: String,
    text: String,
    score: f64,
    #[serde(default)]
    source_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationalMetrics {
    latency_ms: UnavailableMetric,
    cost_units: UnavailableMetric,
}

#[derive(Debug, Deserialize)]
struct UnavailableMetric {
    status: String,
    reason: String,
}

fn provider_id(name: &str) -> ProviderId {
    ProviderId::parse(name).unwrap_or_else(|| panic!("unknown fixture provider: {name}"))
}

fn outputs(case: &QualificationCase) -> Vec<ProviderOutputV1> {
    case.providers
        .iter()
        .map(|provider| ProviderOutputV1 {
            schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
            provider: provider_id(&provider.provider),
            status: FederationProviderStatusV1::Complete,
            generation: Some(FIXTURE_GENERATION.to_owned()),
            candidates: provider
                .candidates
                .iter()
                .map(|candidate| CandidateV1 {
                    id: candidate.id.clone(),
                    layer: 1,
                    provider: None,
                    source_kind: "qualification_fixture".to_owned(),
                    source_ref: format!("fixture://{}", candidate.id),
                    source_hash: candidate.source_hash.clone().unwrap_or_else(|| {
                        format!(
                            "sha256:{:0<64}",
                            candidate
                                .id
                                .as_bytes()
                                .iter()
                                .map(|byte| format!("{byte:02x}"))
                                .collect::<String>()
                        )
                    }),
                    trust_class: "agent_verified".to_owned(),
                    instruction_policy: "data_only".to_owned(),
                    provider_score: candidate.score,
                    score_components: BTreeMap::new(),
                    base_commit: None,
                    overlay_digest: None,
                    freshness_class: None,
                    snapshot_id: None,
                    estimated_tokens: 8,
                    protected: false,
                    exact: true,
                    recoverable: true,
                    resolver: "fixture".to_owned(),
                    text: candidate.text.clone(),
                })
                .collect(),
            warnings: Vec::new(),
            omissions: Vec::new(),
            diagnostics: None,
            extensions: BTreeMap::new(),
        })
        .collect()
}

#[test]
fn qualification_fixture_has_development_and_frozen_held_out_corpora() {
    let fixture: Corpus = serde_json::from_str(FIXTURE).expect("valid qualification fixture");
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.control_policy, "membrane-fusion-fixed-v1");
    assert_eq!(fixture.candidate_strategy, "membrane-fusion-rrf-v1");
    assert!(fixture
        .corpora
        .iter()
        .any(|set| set.name == "development" && !set.frozen));
    assert!(fixture
        .corpora
        .iter()
        .any(|set| set.name == "held_out" && set.frozen));
    for metric in [
        &fixture.operational_metrics.latency_ms,
        &fixture.operational_metrics.cost_units,
    ] {
        assert_eq!(metric.status, "unavailable");
        assert!(!metric.reason.is_empty());
    }
}

#[test]
fn production_path_keeps_fixed_default_and_gates_rrf_explicitly() {
    let fixture: Corpus = serde_json::from_str(FIXTURE).expect("valid qualification fixture");
    for corpus in fixture.corpora {
        for case in corpus.cases {
            let expected = case
                .required_providers
                .iter()
                .map(|name| provider_id(name))
                .collect::<Vec<_>>();
            let lanes = outputs(&case);
            let control = merge_outputs_with_strategy(
                &expected,
                &lanes,
                Some(FIXTURE_GENERATION),
                FusionStrategy::FixedOrder,
            )
            .unwrap_or_else(|error| panic!("{} control merge: {error}", case.id));
            let rrf = merge_outputs_with_strategy(
                &expected,
                &lanes,
                Some(FIXTURE_GENERATION),
                FusionStrategy::Rrf,
            )
            .unwrap_or_else(|error| panic!("{} RRF merge: {error}", case.id));
            let active = merge_outputs(&expected, &lanes, Some(FIXTURE_GENERATION))
                .unwrap_or_else(|error| panic!("{} active merge: {error}", case.id));

            assert_eq!(control.fusion_receipt.policy, "membrane-fusion-fixed-v1");
            assert_eq!(rrf.fusion_receipt.policy, "membrane-fusion-rrf-v1");
            assert_eq!(active.fusion_receipt, control.fusion_receipt);
            assert_eq!(active.candidates, control.candidates);
            assert_eq!(control.fusion_receipt.schema_version, 1);
            assert_eq!(rrf.fusion_receipt.schema_version, 1);
            assert_eq!(control.fusion_receipt.provider_order.len(), lanes.len());
            let candidate_lane_count = lanes
                .iter()
                .filter(|lane| !lane.candidates.is_empty())
                .count();
            assert_eq!(
                rrf.fusion_receipt.provider_order.len(),
                candidate_lane_count
            );
            assert!(
                control.fusion_receipt.candidates_selected
                    <= control.fusion_receipt.candidates_received
            );
            assert!(
                rrf.fusion_receipt.candidates_selected <= rrf.fusion_receipt.candidates_received
            );

            for required in expected {
                if !lanes.iter().any(|lane| lane.provider == required) {
                    assert!(control
                        .omissions
                        .iter()
                        .any(|omission| omission.provider == required));
                    assert!(rrf
                        .omissions
                        .iter()
                        .any(|omission| omission.provider == required));
                }
            }

            let control_again = merge_outputs_with_strategy(
                &case
                    .required_providers
                    .iter()
                    .map(|name| provider_id(name))
                    .collect::<Vec<_>>(),
                &lanes,
                Some(FIXTURE_GENERATION),
                FusionStrategy::FixedOrder,
            )
            .unwrap();
            assert_eq!(
                serde_json::to_string(&control.fusion_receipt).unwrap(),
                serde_json::to_string(&control_again.fusion_receipt).unwrap(),
                "{} control receipt must be stable",
                case.id
            );
        }
    }
}

#[test]
fn fixture_exercises_duplicate_and_diverse_provider_candidates() {
    let fixture: Corpus = serde_json::from_str(FIXTURE).unwrap();
    let case = fixture.corpora[0]
        .cases
        .iter()
        .find(|case| case.id == "duplicate-and-diversity")
        .expect("duplicate fixture case");
    let expected = [ProviderId::Anchors, ProviderId::Blueprint];
    let result = merge_outputs_with_strategy(
        &expected,
        &outputs(case),
        Some(FIXTURE_GENERATION),
        FusionStrategy::Rrf,
    )
    .unwrap();
    assert!(result
        .candidates
        .iter()
        .any(|candidate| candidate.id() == "shared"));
    assert!(result
        .candidates
        .iter()
        .any(|candidate| candidate.id() == "unique-blueprint"));
    assert!(result
        .fusion_receipt
        .decisions
        .iter()
        .all(|decision| decision.fused_rank.is_some()));
}
