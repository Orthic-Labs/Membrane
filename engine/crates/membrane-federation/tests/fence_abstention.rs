//! §17.1 / §17.2 — typed abstention and the publication fence on the
//! production `FederationEngine::federate` path.
//!
//! Fixture pattern follows `corrective_retrieval_qualification.rs`: synthetic
//! providers prove path mechanics. (a) A no-answer merge publishes the typed
//! `insufficientConfidence` extension instead of below-floor hits. (b) A
//! `policy_changed` publication fence refuses emission with a typed engine
//! error, and a held fence survives to the response. (c) A request carrying a
//! `sufficiencyContract` evaluates the same `SufficiencyContractV1` evaluation
//! used by the first-party caller path — one alternate-provider-lane corrective
//! action only, never a repeat against the trigger provider.

use async_trait::async_trait;
use membrane_federation::config::{FederationConfig, ProviderConfig};
use membrane_federation::release::{ReleaseIdentity, ReleaseSource};
use membrane_federation::{FederationEngine, ProviderRegistry};
use membrane_protocol::{
    CandidateV1, FederationProviderStatusV1, FederationRequestV1, FreshnessSnapshotV1, ProviderId,
    ProviderOutputV1, ReasonCode, FEDERATION_REQUEST_SCHEMA_VERSION,
    INSUFFICIENT_CONFIDENCE_POLICY, INSUFFICIENT_CONFIDENCE_SCHEMA_VERSION,
    PUBLICATION_FENCE_POLICY, PUBLICATION_FENCE_SCHEMA_VERSION,
};
use membrane_provider_sdk::{
    empty_output, FreshnessSource, Provider, ProviderContext, ProviderError, ProviderRegistration,
    SourceResponse, SourceResult, SourceSet,
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

const GENERATION: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Clone, Default)]
struct Calls(Arc<Mutex<Vec<ProviderId>>>);

struct FixtureProvider {
    id: ProviderId,
    calls: Calls,
    /// When true, the provider yields one candidate; otherwise the lane is
    /// complete but empty (a no-answer lane).
    with_candidate: bool,
}

#[async_trait]
impl Provider for FixtureProvider {
    async fn provide(&self, _context: &ProviderContext) -> Result<ProviderOutputV1, ProviderError> {
        self.calls.0.lock().unwrap().push(self.id);
        let mut output = empty_output(self.id, FederationProviderStatusV1::Complete);
        output.generation = Some(GENERATION.to_owned());
        if self.with_candidate {
            output.candidates.push(CandidateV1 {
                id: format!("candidate-{}", self.id.as_str()),
                layer: 1,
                provider: Some(self.id.as_str().to_owned()),
                source_kind: "fixture".to_owned(),
                source_ref: format!("fixture://{}", self.id.as_str()),
                source_hash: GENERATION.to_owned(),
                trust_class: "agent_verified".to_owned(),
                instruction_policy: "data_only".to_owned(),
                provider_score: 1.0,
                score_components: Default::default(),
                base_commit: None,
                overlay_digest: None,
                freshness_class: None,
                snapshot_id: None,
                estimated_tokens: 1,
                protected: false,
                exact: true,
                recoverable: true,
                resolver: "fixture".to_owned(),
                text: "fixture evidence".to_owned(),
            });
        } else {
            output
                .omissions
                .push(membrane_protocol::ProviderOmissionV1 {
                    provider: self.id,
                    reason: ReasonCode::ProviderUnavailable,
                    candidate_id: None,
                    detail_id: Some("no_authorized_candidate".to_owned()),
                    stage: None,
                });
        }
        Ok(output)
    }
}

#[derive(Clone)]
struct FixtureRelease;
impl ReleaseSource for FixtureRelease {
    fn current_release(
        &self,
    ) -> Result<ReleaseIdentity, membrane_federation::release::ReleaseError> {
        Ok(ReleaseIdentity::new(GENERATION, "fence-abstention", None).unwrap())
    }
}

#[derive(Clone)]
struct FixtureFreshness;
#[async_trait]
impl FreshnessSource for FixtureFreshness {
    async fn freshness(
        &self,
        _query: &membrane_provider_sdk::SourceQuery,
    ) -> SourceResult<FreshnessSnapshotV1> {
        Ok(SourceResponse {
            value: FreshnessSnapshotV1 {
                graph_state: "current".to_owned(),
                generation: Some(GENERATION.to_owned()),
                snapshot_id: Some("fence-abstention-snapshot".to_owned()),
                base_commit: None,
                overlay_digest: None,
                stale: false,
            },
            generation: Some(GENERATION.to_owned()),
            complete: true,
            warnings: Vec::new(),
        })
    }
}

fn engine(calls: Calls, with_candidate: bool) -> FederationEngine {
    let registrations = ProviderId::ALL
        .into_iter()
        .map(|id| {
            ProviderRegistration::new(
                id,
                format!("fence-abstention.{}", id.as_str()),
                Vec::new(),
                Arc::new(FixtureProvider {
                    id,
                    calls: calls.clone(),
                    with_candidate,
                }),
            )
        })
        .collect();
    let registry = ProviderRegistry::new(registrations).unwrap();
    let config = FederationConfig::new(
        ProviderId::ALL
            .into_iter()
            .map(|id| {
                if matches!(id, ProviderId::Blueprint | ProviderId::Cortex) {
                    ProviderConfig::enabled(id)
                } else {
                    ProviderConfig::disabled(id)
                }
            })
            .collect(),
    )
    .unwrap();
    let sources = SourceSet {
        freshness: Some(Arc::new(FixtureFreshness)),
        ..SourceSet::default()
    };
    FederationEngine::with_release_source(registry, config, sources, FixtureRelease).unwrap()
}

fn request() -> FederationRequestV1 {
    FederationRequestV1 {
        schema_version: FEDERATION_REQUEST_SCHEMA_VERSION,
        request_id: "fence-abstention-request".to_owned(),
        trace_id: "fence-abstention-trace".to_owned(),
        task: "no-answer fence probe".to_owned(),
        repository_root: env!("CARGO_MANIFEST_DIR").to_owned(),
        client: "qualification".to_owned(),
        session_id: "fence-abstention-session".to_owned(),
        deadline_ms: 5_000,
        max_tokens: 100,
        anchors: Vec::new(),
        scope_grant_id: None,
        manifest_digest: None,
        release_generation: Some(GENERATION.to_owned()),
        blueprint_generation: None,
        skills_generation: None,
        extensions: Default::default(),
    }
}

/// §17.1 — a no-answer merge publishes the versioned typed abstention, never
/// below-floor hits.
#[tokio::test]
async fn no_answer_query_publishes_typed_insufficient_confidence() {
    let calls = Calls::default();
    let response = engine(calls, false)
        .federate(&request(), CancellationToken::new())
        .await
        .unwrap();
    assert!(
        response.candidates.is_empty(),
        "a no-answer merge must not emit below-floor hits"
    );
    let abstention = &response.extensions["insufficientConfidence"];
    assert_eq!(abstention["status"], "insufficient_confidence");
    assert_eq!(
        abstention["schemaVersion"],
        INSUFFICIENT_CONFIDENCE_SCHEMA_VERSION
    );
    assert_eq!(abstention["policy"], INSUFFICIENT_CONFIDENCE_POLICY);
    assert_eq!(abstention["reason"], "evidence_floor");
    assert_eq!(abstention["suggestedAction"], "add_authoritative_evidence");
    let searched = abstention["searched"]
        .as_array()
        .expect("per-lane searched");
    assert_eq!(searched.len(), 2, "one entry per active lane");
    assert!(searched
        .iter()
        .all(|lane| lane["lane"].is_string() && lane["searched"].as_u64().is_some()));
    // The envelope round-trips through the versioned protocol type.
    let parsed: membrane_protocol::InsufficientConfidenceV1 =
        serde_json::from_value(abstention.clone()).expect("typed abstention parses");
    assert_eq!(
        parsed.reason.as_str(),
        "evidence_floor",
        "reason vocabulary is the §17.1 contract"
    );
}

/// §17.1 — the same engine with a real candidate publishes evidence and never
/// a spurious abstention.
#[tokio::test]
async fn answered_query_emits_candidates_and_no_abstention() {
    let calls = Calls::default();
    let response = engine(calls, true)
        .federate(&request(), CancellationToken::new())
        .await
        .unwrap();
    assert!(!response.candidates.is_empty());
    assert!(
        response.extensions.get("insufficientConfidence").is_none(),
        "an answered merge must not carry an abstention extension"
    );
}

/// §17.2 — a `policy_changed` publication fence refuses emission with a typed
/// engine error; the stale-authorized packet is never emitted.
#[tokio::test]
async fn policy_changed_fence_refuses_packet_emission() {
    let calls = Calls::default();
    let mut req = request();
    req.extensions.insert(
        "publicationFence".to_owned(),
        json!({
            "schemaVersion": PUBLICATION_FENCE_SCHEMA_VERSION,
            "policy": PUBLICATION_FENCE_POLICY,
            "status": "policy_changed",
            "change": "policy_epoch"
        }),
    );
    let error = engine(calls, true)
        .federate(&req, CancellationToken::new())
        .await
        .expect_err("a tripped fence must refuse emission");
    let message = error.to_string();
    assert!(
        message.contains("policy_changed"),
        "typed fence refusal names the change: {message}"
    );
}

/// §17.2 — a held fence is validated and stamped into the response for the
/// downstream publication seam to preserve.
#[tokio::test]
async fn held_fence_is_validated_and_stamped_into_the_response() {
    let calls = Calls::default();
    let mut req = request();
    req.extensions.insert(
        "publicationFence".to_owned(),
        json!({
            "schemaVersion": PUBLICATION_FENCE_SCHEMA_VERSION,
            "policy": PUBLICATION_FENCE_POLICY,
            "status": "held"
        }),
    );
    let response = engine(calls, true)
        .federate(&req, CancellationToken::new())
        .await
        .unwrap();
    let fence = &response.extensions["publicationFence"];
    assert_eq!(fence["status"], "held");
    assert_eq!(fence["policy"], PUBLICATION_FENCE_POLICY);
}

/// §17.2 — a malformed fence receipt fails typed, never bypasses the gate.
#[tokio::test]
async fn malformed_fence_receipt_fails_typed() {
    let calls = Calls::default();
    let mut req = request();
    req.extensions.insert(
        "publicationFence".to_owned(),
        json!({"schemaVersion": 99, "status": "nonsense"}),
    );
    let error = engine(calls, true)
        .federate(&req, CancellationToken::new())
        .await
        .expect_err("malformed fence is a typed failure");
    assert!(
        error.to_string().contains("fence"),
        "typed fence error: {error}"
    );
}

/// §13.1 — a sufficiency contract supplied on the request reaches the same
/// planner-owned evaluation, and the bounded corrective stage runs exactly one
/// alternate-provider lane (never a repeat against the trigger).
#[tokio::test]
async fn sufficiency_contract_runs_one_alternate_lane_never_the_trigger_twice() {
    struct CorrectiveProvider {
        id: ProviderId,
        calls: Calls,
    }
    #[async_trait]
    impl Provider for CorrectiveProvider {
        async fn provide(
            &self,
            _context: &ProviderContext,
        ) -> Result<ProviderOutputV1, ProviderError> {
            let mut calls = self.calls.0.lock().unwrap();
            calls.push(self.id);
            let invocation = calls.iter().filter(|call| **call == self.id).count();
            drop(calls);
            let mut output = empty_output(self.id, FederationProviderStatusV1::Complete);
            output.generation = Some(GENERATION.to_owned());
            if self.id == ProviderId::Cortex && invocation > 1 {
                output.candidates.push(CandidateV1 {
                    id: "corrective-candidate".to_owned(),
                    layer: 1,
                    provider: Some(self.id.as_str().to_owned()),
                    source_kind: "fixture".to_owned(),
                    source_ref: "fixture://corrective".to_owned(),
                    source_hash: GENERATION.to_owned(),
                    trust_class: "agent_verified".to_owned(),
                    instruction_policy: "data_only".to_owned(),
                    provider_score: 1.0,
                    score_components: Default::default(),
                    base_commit: None,
                    overlay_digest: None,
                    freshness_class: None,
                    snapshot_id: None,
                    estimated_tokens: 1,
                    protected: false,
                    exact: true,
                    recoverable: true,
                    resolver: "fixture".to_owned(),
                    text: "corrective evidence".to_owned(),
                });
            }
            Ok(output)
        }
    }

    let calls = Calls::default();
    let registrations = ProviderId::ALL
        .into_iter()
        .map(|id| {
            ProviderRegistration::new(
                id,
                format!("fence-abstention.{}", id.as_str()),
                Vec::new(),
                Arc::new(CorrectiveProvider {
                    id,
                    calls: calls.clone(),
                }),
            )
        })
        .collect();
    let registry = ProviderRegistry::new(registrations).unwrap();
    let config = FederationConfig::new(
        ProviderId::ALL
            .into_iter()
            .map(|id| {
                if matches!(id, ProviderId::Blueprint | ProviderId::Cortex) {
                    ProviderConfig::enabled(id)
                } else {
                    ProviderConfig::disabled(id)
                }
            })
            .collect(),
    )
    .unwrap();
    let sources = SourceSet {
        freshness: Some(Arc::new(FixtureFreshness)),
        ..SourceSet::default()
    };
    let engine =
        FederationEngine::with_release_source(registry, config, sources, FixtureRelease).unwrap();

    let mut req = request();
    req.extensions.insert(
        "sufficiencyContract".to_owned(),
        json!({
            "schemaVersion": 1,
            "policy": "membrane-sufficiency-v1",
            "requirements": [{
                "id": "required-fixture-evidence",
                "evidenceClass": "fixture",
                "acceptableProviders": ["blueprint", "cortex"],
                "acceptableSourceRefs": [],
                "minimumCandidates": 1
            }],
            "maxCorrectiveStages": 1
        }),
    );
    let response = engine
        .federate(&req, CancellationToken::new())
        .await
        .unwrap();
    let receipt = &response.extensions["correctiveRetrieval"];
    assert_eq!(receipt["triggered"], true);
    assert_eq!(receipt["attempted"], true);
    assert_eq!(receipt["targetProvider"], "cortex");
    assert_eq!(receipt["outcome"], "corrective_stage_sufficient");
    assert!(response
        .candidates
        .iter()
        .any(|candidate| candidate.id == "corrective-candidate"));

    let calls = calls.0.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|call| **call == ProviderId::Blueprint)
            .count(),
        1,
        "the trigger lane is never repeated"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| **call == ProviderId::Cortex)
            .count(),
        2,
        "exactly one corrective stage on the alternate lane"
    );
}
