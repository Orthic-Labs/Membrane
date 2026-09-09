//! Behavioral qualification for committed Pull residuals.
//!
//! These tests deliberately exercise public federation, admission, planner,
//! placement, and publication seams.  Missing source/authority/resolver
//! conditions remain typed outcomes; this file does not replace owners of
//! provider implementations or publication policy.

use membrane_federation::corrective::{
    alternate_provider_for_requirement, CorrectiveRetrievalReceiptV1,
    RequirementCoverageStateV1, SufficiencyAssessmentV1, SufficiencyContractV1,
    SufficiencyReasonV1, SufficiencyRequirementV1, SufficiencyStateV1,
};
use membrane_federation::deadline::{Deadline, MonotonicClock};
use membrane_federation::merge::{merge_outputs, merge_outputs_with_strategy, FusionStrategy};
use membrane_federation::normalize::{
    generation_admission, normalize_provider_output, CandidateNormalizationError,
};
use membrane_federation::scheduler::{schedule_providers, ProviderTask, SchedulerPolicy};
use membrane_protocol::{
    CandidateV1, DeadlineBudget, FederationProviderStatusV1, FreshnessSnapshotV1,
    InsufficientConfidenceLaneSearchV1, InsufficientConfidenceReasonV1,
    InsufficientConfidenceStatusV1, InsufficientConfidenceV1, ProviderId, ProviderOutputV1,
    PublicationFenceChangeV1, PublicationFenceStatusV1, PublicationFenceV1, ReasonCode,
    PROVIDER_OUTPUT_SCHEMA_VERSION,
};
use membrane_provider_sdk::{Provider, ProviderContext, SourceSet};
use membrane_runtime::pull::admission::{
    admit, AdmissionError, AdmissionRequest, Authority, InstructionPolicy, Origin,
    QuarantineStatus,
};
use membrane_runtime::pull::federation::{envelope_from_ccs, EnvelopeInput};
use membrane_runtime::pull::placement::place;
use std::collections::BTreeMap;
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

fn candidate(id: &str, provider: Option<&str>, source_hash: &str) -> CandidateV1 {
    CandidateV1 {
        id: id.to_owned(),
        layer: 3,
        provider: provider.map(str::to_owned),
        source_kind: "file".to_owned(),
        source_ref: format!("src/{id}.rs"),
        source_hash: source_hash.to_owned(),
        trust_class: "workspace_tracked".to_owned(),
        instruction_policy: "data_only".to_owned(),
        provider_score: 0.8,
        score_components: BTreeMap::from([(String::from("relevance"), 0.8)]),
        base_commit: None,
        overlay_digest: None,
        freshness_class: None,
        snapshot_id: None,
        estimated_tokens: 8,
        protected: false,
        exact: true,
        recoverable: true,
        resolver: "membrane_source_read:src".to_owned(),
        text: format!("evidence for {id}"),
    }
}

fn output(provider: ProviderId, candidates: Vec<CandidateV1>) -> ProviderOutputV1 {
    ProviderOutputV1 {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider,
        status: FederationProviderStatusV1::Complete,
        generation: Some(format!("sha256:{}", "a".repeat(64))),
        candidates,
        warnings: Vec::new(),
        omissions: Vec::new(),
        diagnostics: None,
        extensions: BTreeMap::new(),
    }
}

fn freshness() -> FreshnessSnapshotV1 {
    FreshnessSnapshotV1 {
        graph_state: "clean".to_owned(),
        generation: Some(format!("sha256:{}", "a".repeat(64))),
        snapshot_id: Some("snapshot-1".to_owned()),
        base_commit: Some("b".repeat(40)),
        overlay_digest: None,
        stale: false,
    }
}

fn provider_context(deadline: Instant, cancellation: CancellationToken) -> ProviderContext {
    ProviderContext::new(
        "request-1",
        "D:/workspace/repo",
        "repo-1",
        "qualify Pull",
        "session-1",
        "test",
        Vec::new(),
        None,
        Some(format!("sha256:{}", "a".repeat(64))),
        freshness(),
        deadline,
        cancellation,
        "trace-1",
        SourceSet::default(),
    )
}

/// PUL-004, PUL-005, PUL-006, PUL-007, PUL-008, PUL-009, PUL-010,
/// PUL-011, PUL-012, PUL-013, PUL-014, PUL-015, PUL-025, PUL-026,
/// PUL-027, PUL-028:
/// request-owned budgets and bounded provider lanes retain typed omissions
/// when a source is unavailable, with no provider able to extend the budget.
#[tokio::test(flavor = "current_thread")]
async fn acquisition_is_bounded_and_unavailable_lanes_are_typed() {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let make_task = |provider: ProviderId, active: Arc<AtomicUsize>, peak: Arc<AtomicUsize>| {
        ProviderTask::new(provider, move |_context| {
            let active = Arc::clone(&active);
            let peak = Arc::clone(&peak);
            async move {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                tokio::task::yield_now().await;
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(output(provider, Vec::new()))
            }
        })
    };
    let tasks = vec![
        make_task(ProviderId::Anchors, Arc::clone(&active), Arc::clone(&peak)),
        make_task(ProviderId::Blueprint, Arc::clone(&active), Arc::clone(&peak))
            .with_prerequisites([ProviderId::Anchors]),
    ];
    let result = schedule_providers(
        provider_context(
            Instant::now() + Duration::from_secs(1),
            CancellationToken::new(),
        ),
        Deadline::at(Instant::now() + Duration::from_secs(1)),
        tasks,
        SchedulerPolicy::bounded(1, Duration::from_millis(5)),
    )
    .await;
    assert_eq!(result.outputs.len(), 2);
    assert_eq!(result.omissions.len(), 0);
    assert_eq!(peak.load(Ordering::SeqCst), 1);
    assert!(result.timings.windows(2).all(|pair| pair[0].provider.rank() < pair[1].provider.rank()));

    let missing = merge_outputs(
        &[ProviderId::Anchors, ProviderId::Blueprint],
        &[output(ProviderId::Anchors, Vec::new())],
        None,
    )
    .expect("missing lanes are accounted, not a merge error");
    assert!(missing.omissions.iter().any(|omission| {
        omission.provider == ProviderId::Blueprint
            && omission.reason == ReasonCode::ProviderUnavailable
    }));
}

/// PUL-006..PUL-015:
/// provider adapters consume request-bound inputs and report missing source or
/// grant state as typed lane outcomes instead of manufacturing evidence.
#[tokio::test(flavor = "current_thread")]
async fn provider_classes_report_typed_gaps_without_authority() {
    let context = provider_context(
        Instant::now() + Duration::from_secs(1),
        CancellationToken::new(),
    );
    let anchors = membrane_federation::providers::anchors::AnchorsProvider::default()
        .provide(&context)
        .await
        .unwrap();
    assert!(anchors.omissions.iter().any(|omission| {
        omission.provider == ProviderId::Anchors && omission.reason == ReasonCode::ProviderUnavailable
    }));

    let live_files = membrane_federation::providers::live_files::LiveFilesProvider::default()
        .provide(&context)
        .await
        .unwrap();
    assert!(live_files.omissions.iter().any(|omission| {
        omission.provider == ProviderId::LiveFiles && omission.reason == ReasonCode::ScopeGrantMissing
    }));

    for result in [
        membrane_federation::providers::cortex::CortexProvider::default()
            .provide(&context)
            .await
            .unwrap(),
        membrane_federation::providers::audit::AuditProvider::default()
            .provide(&context)
            .await
            .unwrap(),
        membrane_federation::providers::architect::ArchitectProvider::default()
            .provide(&context)
            .await
            .unwrap(),
    ] {
        assert!(result.omissions.iter().any(|omission| {
            omission.reason == ReasonCode::ProviderUnavailable
        }));
    }
}

/// PUL-016, PUL-017, PUL-018, PUL-023:
/// normalization owns source/generation/authority metadata and refuses
/// malformed or mismatched lanes before fusion.
#[test]
fn normalization_preserves_independent_generation_and_authority_axes() {
    let good = output(ProviderId::Git, vec![candidate("git-head", Some("spoof"), "sha256:111")]);
    let normalized = normalize_provider_output(&good, ProviderId::Git).expect("valid lane");
    assert_eq!(normalized.candidates[0].provider, ProviderId::Git);
    assert_eq!(normalized.generation, good.generation);
    assert_eq!(normalized.candidates[0].candidate.provider.as_deref(), Some("git"));

    let wrong_provider = normalize_provider_output(&good, ProviderId::Blueprint).unwrap_err();
    assert_eq!(wrong_provider, CandidateNormalizationError::ProviderMismatch);
    assert_eq!(
        generation_admission(ProviderId::Git, Some("sha256:bb"), good.generation.as_deref())
            .unwrap_err()
            .reason,
        ReasonCode::GenerationIncoherent
    );

    let malformed = output(ProviderId::Git, vec![CandidateV1 {
        instruction_policy: "execute".to_owned(),
        ..candidate("bad", None, "sha256:222")
    }]);
    assert_eq!(
        normalize_provider_output(&malformed, ProviderId::Git).unwrap_err(),
        CandidateNormalizationError::InvalidInstructionPolicy
    );
}

/// PUL-021, PUL-022, PUL-023:
/// fixed-order fusion is named/versioned, deterministic, and records
/// duplicate/conflict outcomes without mixing provider-local scores.
#[test]
fn fusion_is_deterministic_and_conflicts_remain_content_free() {
    let first = output(ProviderId::Blueprint, vec![candidate(
        "blueprint-one",
        None,
        "sha256:111",
    )]);
    let second = output(ProviderId::Anchors, vec![candidate(
        "anchors-one",
        None,
        "sha256:222",
    )]);
    let fixed = merge_outputs_with_strategy(
        &[ProviderId::Blueprint, ProviderId::Anchors],
        &[first.clone(), second.clone()],
        None,
        FusionStrategy::FixedOrder,
    )
    .expect("fixed fusion");
    assert_eq!(fixed.candidates[0].provider, ProviderId::Anchors);
    assert_eq!(fixed.fusion_receipt.policy, "membrane-fusion-fixed-v1");
    assert_eq!(fixed.fusion_receipt.candidates_selected, 2);

    let duplicate = output(
        ProviderId::Git,
        vec![candidate("same", None, "sha256:333"), candidate("same", None, "sha256:333")],
    );
    let collapsed = merge_outputs(&[ProviderId::Git], &[duplicate], None).unwrap();
    assert_eq!(collapsed.candidates.len(), 1);
    assert!(collapsed.omissions.is_empty());

    let conflict = output(
        ProviderId::Git,
        vec![candidate("same", None, "sha256:444"), candidate("same", None, "sha256:555")],
    );
    let rejected = merge_outputs(&[ProviderId::Git], &[conflict], None).unwrap();
    assert!(rejected.candidates.is_empty());
    assert!(rejected.omissions.iter().any(|omission| {
        omission.reason == ReasonCode::CandidateIdentityConflict
            && omission.candidate_id.as_deref() == Some("same")
    }));
}

/// PUL-020, PUL-025, PUL-026:
/// insufficiency can select one acceptable alternate lane, while the receipt
/// stays typed and bounded at one corrective stage.
#[test]
fn corrective_retrieval_selects_one_alternate_and_stays_bounded() {
    let contract = SufficiencyContractV1 {
        schema_version: 1,
        policy: "membrane-sufficiency-v1".to_owned(),
        requirements: vec![SufficiencyRequirementV1 {
            id: "repo".to_owned(),
            evidence_class: "repository_file".to_owned(),
            acceptable_providers: vec![ProviderId::Blueprint, ProviderId::Cortex],
            acceptable_source_refs: Vec::new(),
            minimum_candidates: 1,
        }],
        max_corrective_stages: 1,
    };
    contract.validate().unwrap();
    assert_eq!(
        alternate_provider_for_requirement(
            &[ProviderId::Anchors, ProviderId::Blueprint, ProviderId::Cortex],
            ProviderId::Blueprint,
            &[ProviderId::Blueprint, ProviderId::Cortex],
        ),
        Some(ProviderId::Cortex)
    );
    assert_eq!(
        contract.alternate_target(ProviderId::Blueprint, "repo", &[ProviderId::Blueprint, ProviderId::Cortex]),
        Some(ProviderId::Cortex)
    );

    let assessment = SufficiencyAssessmentV1 {
        schema_version: 1,
        policy: "membrane-sufficiency-v1".to_owned(),
        state: SufficiencyStateV1::Insufficient,
        requirements: vec![membrane_federation::corrective::RequirementCoverageV1 {
            requirement_id: "repo".to_owned(),
            state: RequirementCoverageStateV1::Missing,
            matching_candidates: 0,
            required_candidates: 1,
            reason: SufficiencyReasonV1::NoMatchingCandidate,
        }],
    };
    let receipt = CorrectiveRetrievalReceiptV1::from_assessment(
        assessment,
        Some(ProviderId::Cortex),
        Some("repo".to_owned()),
        99,
    );
    assert!(receipt.triggered);
    assert_eq!(receipt.stage_limit, 1);
    assert_eq!(receipt.outcome, "corrective_stage_planned");
}

/// PUL-033:
/// no surviving evidence is represented by the versioned insufficient-
/// confidence shape with searched-lane accounting, not an empty success.
#[test]
fn no_evidence_returns_versioned_typed_abstention() {
    let abstention = InsufficientConfidenceV1 {
        status: InsufficientConfidenceStatusV1::InsufficientConfidence,
        schema_version: 1,
        policy: membrane_protocol::INSUFFICIENT_CONFIDENCE_POLICY.to_owned(),
        searched: vec![InsufficientConfidenceLaneSearchV1 {
            lane: "blueprint".to_owned(),
            searched: 3,
        }],
        reason: InsufficientConfidenceReasonV1::NoAuthorizedCandidateAboveThreshold,
        suggested_action: InsufficientConfidenceV1::suggested_action_for(
            InsufficientConfidenceReasonV1::NoAuthorizedCandidateAboveThreshold,
        )
        .map(str::to_owned),
    };
    let wire = serde_json::to_value(&abstention).unwrap();
    assert_eq!(wire["status"], "insufficient_confidence");
    assert_eq!(wire["searched"][0]["searched"], 3);
    assert_eq!(wire["suggestedAction"], "broaden_scope_or_request_access");
}

/// PUL-035, PUL-036:
/// final resolver and publication authorization observations refuse stale
/// packet emission with typed policy-change details.
#[test]
fn publication_fence_refuses_changed_authorization_before_emission() {
    let changed = PublicationFenceV1::policy_changed(PublicationFenceChangeV1::Revocation);
    assert_eq!(changed.status, PublicationFenceStatusV1::PolicyChanged);
    let error = membrane_runtime::pull::federation::fence_packet_emission(Some(changed)).unwrap_err();
    assert!(error.contains("policy_changed"));
    assert!(error.contains("stale-authorized packet is not emitted"));
    let held = membrane_runtime::pull::federation::fence_packet_emission(Some(PublicationFenceV1::held()))
        .unwrap()
        .unwrap();
    assert_eq!(held.status, PublicationFenceStatusV1::Held);
}

/// PUL-040, PUL-041:
/// semantic placement changes presentation only, while the public planner
/// path aggregates independent source identities under one token ceiling.
#[test]
fn placement_and_native_aggregate_preserve_identity_under_one_ceiling() {
    let source_a = candidate("repo-a", Some("blueprint"), "sha256:aaa");
    let source_b = candidate("repo-b", Some("git"), "sha256:bbb");
    let ccs = serde_json::json!({
        "schemaVersion": 1,
        "traceId": "trace-aggregate",
        "indexedAt": "2026-09-08T00:00:00Z",
        "task": "aggregate repositories",
        "mode": "verify",
        "provider": "native",
        "freshness": {"revision": "rev", "indexedAt": "2026-09-08T00:00:00Z", "stale": false},
        "providerCeiling": {"maxCandidates": 8, "maxEstimatedTokens": 64},
        "candidates": [source_a, source_b],
        "omissions": []
    });
    let payload = envelope_from_ccs(
        &ccs.to_string(),
        EnvelopeInput {
            max_tokens: 16,
            packet_char_budget_override: None,
            packet_char_budget_model: None,
            accepted_receipt_versions: vec![2],
            scope_grant_present: false,
            consumer_resolvers: Vec::new(),
            scope_grant_fence: None,
            gateway_process_ms: 0.0,
        },
    )
    .unwrap();
    assert_eq!(payload["packet"]["budget"]["maxTokens"], 16);
    let blocks = payload["packet"]["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 2);
    assert!(blocks.iter().any(|block| block["sourceHash"] == "sha256:aaa"));
    assert!(blocks.iter().any(|block| block["sourceHash"] == "sha256:bbb"));

    let mut packet = cortex_core::planner::ContextPacketV1 {
        schema_version: 1,
        trace_id: "trace-placement".to_owned(),
        task: "place".to_owned(),
        mode: "verify".to_owned(),
        budget: cortex_core::planner::BudgetV1 {
            max_tokens: 64,
            admitted_tokens: 16,
            packet_char_budget_default: None,
            packet_char_budget_override: None,
            packet_char_budget_model: None,
            configured_packet_char_budget: None,
            effective_packet_char_budget: None,
        },
        allocations: BTreeMap::new(),
        provider_accounting: BTreeMap::new(),
        blocks: vec![
            cortex_core::planner::BlockV1 {
                id: "doc".to_owned(), layer: 1, provider: "ledger".to_owned(), source_kind: "doc".to_owned(),
                source_ref: "doc.md".to_owned(), source_hash: "sha256:1".to_owned(), trust_class: "workspace".to_owned(),
                instruction_policy: "data_only".to_owned(), base_commit: None, overlay_digest: None, freshness_class: None,
                snapshot_id: None, priority: 1, estimated_tokens: 8, delivery_stage: None, delivery_class: None,
                selected_tokens: None, allotted_tokens: None, rendered_tokens: None, delivered_chars: None, drop_reason: None,
                protected: false, recoverable: true, resolver: "source_read".to_owned(), text: "doc".to_owned(),
            },
            cortex_core::planner::BlockV1 {
                id: "rule".to_owned(), layer: 1, provider: "rules".to_owned(), source_kind: "rule".to_owned(),
                source_ref: "rules.md".to_owned(), source_hash: "sha256:2".to_owned(), trust_class: "workspace".to_owned(),
                instruction_policy: "data_only".to_owned(), base_commit: None, overlay_digest: None, freshness_class: None,
                snapshot_id: None, priority: 1, estimated_tokens: 8, delivery_stage: None, delivery_class: None,
                selected_tokens: None, allotted_tokens: None, rendered_tokens: None, delivered_chars: None, drop_reason: None,
                protected: false, recoverable: true, resolver: "source_read".to_owned(), text: "rule".to_owned(),
            },
        ],
        omissions: Vec::new(),
    };
    let before = packet.blocks.iter().map(|block| block.id.clone()).collect::<Vec<_>>();
    let receipt = place(&mut packet);
    let after = packet.blocks.iter().map(|block| block.id.clone()).collect::<Vec<_>>();
    assert_eq!(before.into_iter().collect::<std::collections::BTreeSet<_>>(), after.iter().cloned().collect());
    assert_eq!(receipt.policy, "pull-semantic-placement-v1");
    assert_eq!(after, vec!["rule", "doc"]);
}

/// PUL-042:
/// a resolver-only candidate is unavailable without a negotiated consumer,
/// then becomes resolver-backed when that capability is explicitly present.
#[test]
fn resolver_selection_is_negotiated_or_typed_unavailable() {
    let mut only_handle = candidate("handle", Some("blueprint"), "sha256:ccc");
    only_handle.text.clear();
    let ccs = serde_json::json!({
        "schemaVersion": 1, "traceId": "trace-resolver", "indexedAt": "2026-09-08T00:00:00Z",
        "task": "resolve", "mode": "verify", "provider": "blueprint",
        "freshness": {"revision": "rev", "indexedAt": "2026-09-08T00:00:00Z", "stale": false},
        "providerCeiling": {"maxCandidates": 8, "maxEstimatedTokens": 64},
        "candidates": [only_handle], "omissions": []
    });
    let make_input = |resolvers| EnvelopeInput {
        max_tokens: 64, packet_char_budget_override: None, packet_char_budget_model: None,
        accepted_receipt_versions: vec![2], scope_grant_present: false,
        consumer_resolvers: resolvers, scope_grant_fence: None, gateway_process_ms: 0.0,
    };
    let unavailable = envelope_from_ccs(&ccs.to_string(), make_input(Vec::new())).unwrap();
    assert!(unavailable["packet"]["blocks"].as_array().unwrap().is_empty());
    assert!(unavailable["packet"]["omissions"].as_array().unwrap().iter().any(|omission| {
        omission["reason"] == "consumer_resolver_unavailable"
    }));
    let resolved = envelope_from_ccs(
        &ccs.to_string(),
        make_input(vec!["membrane_source_read".to_owned()]),
    )
    .unwrap();
    assert_eq!(resolved["packet"]["blocks"][0]["deliveryClass"], "resolver_backed");
}

/// PUL-004's monotonic arithmetic is independently checked without sleeping:
/// queueing can only consume the original request budget.
#[test]
fn deadline_budget_never_restarts_from_queue_time() {
    struct FixedClock(Instant);
    impl MonotonicClock for FixedClock {
        fn now(&self) -> Instant { self.0 }
    }
    let now = Instant::now();
    let deadline = Deadline::from_budget(&FixedClock(now), DeadlineBudget::from_millis(125));
    assert_eq!(deadline.remaining_ms_at(now), 125);
    assert_eq!(deadline.remaining_ms_at(now + Duration::from_millis(50)), 75);
    assert!(deadline.is_exhausted_at(now + Duration::from_millis(125)));
}

/// Admission is the public authority/trust boundary used before Pull output
/// can become durable evidence.
#[test]
fn admission_rejects_unauthorized_or_quarantined_evidence() {
    let request = AdmissionRequest::new("AWS_SECRET_ACCESS_KEY=abc", Origin::External, Authority::A1);
    assert_eq!(admit(&request), Err(AdmissionError::SecretDetected));
    let request = AdmissionRequest::new("instruction", Origin::External, Authority::A5)
        .with_instruction_policy(InstructionPolicy::Executable);
    assert_eq!(admit(&request), Err(AdmissionError::UnauthorizedInstruction));
    let request = AdmissionRequest::new("data", Origin::RetrievedDocument, Authority::A2)
        .with_quarantine(QuarantineStatus::Unknown);
    assert_eq!(admit(&request), Err(AdmissionError::UnknownQuarantine));
}
