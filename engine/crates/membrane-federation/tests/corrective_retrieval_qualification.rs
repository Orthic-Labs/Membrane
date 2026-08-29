use async_trait::async_trait;
use membrane_federation::config::{FederationConfig, ProviderConfig};
use membrane_federation::release::{ReleaseIdentity, ReleaseSource};
use membrane_federation::{FederationEngine, ProviderRegistry};
use membrane_protocol::{
    CandidateV1, FederationProviderStatusV1, FederationRequestV1, FreshnessSnapshotV1, ProviderId,
    ProviderOutputV1, ReasonCode, FEDERATION_REQUEST_SCHEMA_VERSION,
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
struct Calls(Arc<Mutex<Vec<(ProviderId, String, String, std::time::Instant, u32)>>>);

struct FixtureProvider {
    id: ProviderId,
    calls: Calls,
    corrective_candidate: bool,
}

#[async_trait]
impl Provider for FixtureProvider {
    async fn provide(&self, context: &ProviderContext) -> Result<ProviderOutputV1, ProviderError> {
        let mut calls = self.calls.0.lock().unwrap();
        calls.push((
            self.id,
            context.request_id.clone(),
            context.trace_id.clone(),
            context.deadline,
            context.freshness.graph_state.len() as u32,
        ));
        let invocation = calls.iter().filter(|call| call.0 == self.id).count();
        drop(calls);
        if self.id == ProviderId::Cortex && self.corrective_candidate && invocation > 1 {
            return Ok(candidate_output(self.id, "held-out-cortex"));
        }
        let mut output = empty_output(self.id, FederationProviderStatusV1::Complete);
        output.generation = Some(GENERATION.to_owned());
        output
            .omissions
            .push(membrane_protocol::ProviderOmissionV1 {
                provider: self.id,
                reason: ReasonCode::ProviderUnavailable,
                candidate_id: None,
                detail_id: Some("fixture_empty_initial_lane".to_owned()),
                stage: None,
            });
        Ok(output)
    }
}

fn candidate_output(provider: ProviderId, id: &str) -> ProviderOutputV1 {
    let mut output = empty_output(provider, FederationProviderStatusV1::Complete);
    output.generation = Some(GENERATION.to_owned());
    output.candidates.push(CandidateV1 {
        id: id.to_owned(),
        layer: 1,
        provider: Some(provider.as_str().to_owned()),
        source_kind: "fixture".to_owned(),
        source_ref: format!("fixture://{id}"),
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
        text: "held-out".to_owned(),
    });
    output
}

#[derive(Clone)]
struct FixtureRelease;
impl ReleaseSource for FixtureRelease {
    fn current_release(
        &self,
    ) -> Result<ReleaseIdentity, membrane_federation::release::ReleaseError> {
        Ok(ReleaseIdentity::new(GENERATION, "qualification", None).unwrap())
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
                snapshot_id: Some("qualification-snapshot".to_owned()),
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

fn engine(calls: Calls, corrective_candidate: bool) -> FederationEngine {
    let registrations = ProviderId::ALL
        .into_iter()
        .map(|id| {
            ProviderRegistration::new(
                id,
                format!("qualification.{}", id.as_str()),
                Vec::new(),
                Arc::new(FixtureProvider {
                    id,
                    calls: calls.clone(),
                    corrective_candidate: id == ProviderId::Cortex && corrective_candidate,
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
        request_id: "qualification-request".to_owned(),
        trace_id: "qualification-trace".to_owned(),
        task: "qualify corrective retrieval".to_owned(),
        repository_root: env!("CARGO_MANIFEST_DIR").to_owned(),
        client: "qualification".to_owned(),
        session_id: "qualification-session".to_owned(),
        deadline_ms: 5_000,
        max_tokens: 100,
        anchors: Vec::new(),
        scope_grant_id: None,
        manifest_digest: None,
        release_generation: Some(GENERATION.to_owned()),
        blueprint_generation: None,
        skills_generation: None,
        extensions: [(
            "sufficiencyContract".to_owned(),
            json!({
                "schemaVersion": 1,
                "policy": "membrane-sufficiency-v1",
                "requirements": [{
                    "id": "held-out-fixture",
                    "evidenceClass": "fixture",
                    "acceptableProviders": ["blueprint", "cortex"],
                    "acceptableSourceRefs": [],
                    "minimumCandidates": 1
                }],
                "maxCorrectiveStages": 1
            }),
        )]
        .into_iter()
        .collect(),
    }
}

#[tokio::test]
async fn dev_corrective_path_runs_exactly_one_alternate_and_remerges() {
    let calls = Calls::default();
    let response = engine(calls.clone(), true)
        .federate(&request(), CancellationToken::new())
        .await
        .unwrap();
    let receipt = &response.extensions["correctiveRetrieval"];
    assert_eq!(receipt["triggered"], true);
    assert_eq!(receipt["attempted"], true);
    assert_eq!(receipt["triggerProvider"], "blueprint");
    assert_eq!(receipt["targetProvider"], "cortex");
    assert_eq!(receipt["outcome"], "corrective_stage_sufficient");
    assert!(response
        .candidates
        .iter()
        .any(|candidate| candidate.id == "held-out-cortex"));

    let calls = calls.0.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.0 == ProviderId::Blueprint)
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.0 == ProviderId::Cortex)
            .count(),
        2
    );
    assert!(calls.iter().all(|call| call.1 == "qualification-request"));
    assert!(calls.iter().all(|call| call.2 == "qualification-trace"));
    assert_eq!(calls[1].3, calls[2].3);
}

/// (d) No contract on the request means sufficiency stays `not_evaluated` and
/// no corrective stage is planned, attempted, or executed — the unchanged
/// default path.
#[tokio::test]
async fn no_contract_leaves_sufficiency_not_evaluated() {
    let calls = Calls::default();
    let mut req = request();
    req.extensions.remove("sufficiencyContract");
    let response = engine(calls.clone(), true)
        .federate(&req, CancellationToken::new())
        .await
        .unwrap();
    let receipt = &response.extensions["correctiveRetrieval"];
    assert_eq!(receipt["triggered"], false);
    assert_eq!(receipt["attempted"], false);
    assert_eq!(receipt["outcome"], "not_evaluated_missing_sufficiency_contract");
    assert!(receipt.get("sufficiency").is_none() || receipt["sufficiency"].is_null());

    // Only the two enabled providers run once each — no corrective stage.
    let calls = calls.0.lock().unwrap();
    assert_eq!(calls.len(), 2);
}

/// (a) A contract already satisfied by the initial merge publishes without
/// any corrective action: no second call to any provider, and the receipt
/// records `sufficient` with `triggered: false`.
#[tokio::test]
async fn sufficient_initial_merge_publishes_without_corrective_action() {
    struct AlwaysMatchingProvider {
        id: ProviderId,
        calls: Calls,
    }
    #[async_trait]
    impl Provider for AlwaysMatchingProvider {
        async fn provide(
            &self,
            context: &ProviderContext,
        ) -> Result<ProviderOutputV1, ProviderError> {
            self.calls.0.lock().unwrap().push((
                self.id,
                context.request_id.clone(),
                context.trace_id.clone(),
                context.deadline,
                0,
            ));
            Ok(candidate_output(self.id, "already-sufficient"))
        }
    }

    let calls = Calls::default();
    let registrations = ProviderId::ALL
        .into_iter()
        .map(|id| {
            ProviderRegistration::new(
                id,
                format!("qualification.{}", id.as_str()),
                Vec::new(),
                Arc::new(AlwaysMatchingProvider {
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
                "id": "held-out-fixture",
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
    assert_eq!(receipt["triggered"], false);
    assert_eq!(receipt["attempted"], false);
    assert_eq!(receipt["outcome"], "sufficient");
    assert_eq!(receipt["sufficiency"]["state"], "sufficient");

    // Publishing on an already-sufficient merge must never call any provider
    // a second time.
    let calls = calls.0.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.0 == ProviderId::Blueprint)
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.0 == ProviderId::Cortex)
            .count(),
        1
    );
}

#[tokio::test]
async fn held_out_terminal_case_attempts_once_then_types_second_insufficiency() {
    let calls = Calls::default();
    let response = engine(calls.clone(), false)
        .federate(&request(), CancellationToken::new())
        .await
        .unwrap();
    let receipt = &response.extensions["correctiveRetrieval"];
    assert_eq!(receipt["triggered"], true);
    assert_eq!(receipt["attempted"], true);
    assert_eq!(
        receipt["outcome"],
        "terminal_insufficient_second_assessment"
    );
    assert!(response.candidates.is_empty());
    let calls = calls.0.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[1].3, calls[2].3);
}
