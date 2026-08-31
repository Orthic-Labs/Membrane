use std::collections::BTreeMap;

use membrane_adapt::authority::{AuthorityEffect, Origin, PrecedenceTier};
use membrane_adapt::canonical::sha256_hex;
use membrane_adapt::model_boundary::{
    DeterministicProposalBindingV1, ModelExtractionProposal, VerifiedModelEvidenceV1,
};
use membrane_adapt::proposal_state::{
    ProposalPlanState, ProposalRisk, ProposalVerificationV1,
};
use membrane_adapt::record::RecordClass;
use membrane_adapt::scope::ScopeDimensions;
use membrane_runtime::adapt::{
    execute_adapt_proposal_plan, AdaptProposalPlanRequestV1,
    ADAPT_PROPOSAL_SERVICE_CONTRACT,
};

fn model_proposal(excerpt: &str) -> ModelExtractionProposal {
    ModelExtractionProposal {
        proposer_id: "model-1".into(),
        rule_text: excerpt.into(),
        category_hint: "permission_expansion".into(),
        scope_hint: "global".into(),
        bound_evidence_ids: vec!["event-1".into()],
        bound_evidence_excerpt: excerpt.into(),
    }
}

fn binding(excerpt: &str) -> DeterministicProposalBindingV1 {
    let mut dimensions = BTreeMap::new();
    dimensions.insert("repo".into(), "membrane".into());
    DeterministicProposalBindingV1 {
        evidence: vec![VerifiedModelEvidenceV1 {
            event_id: "event-1".into(),
            excerpt_sha256: sha256_hex(excerpt.as_bytes()),
            source_evidence_digest: "source-sha256".into(),
            origin: Origin::UserTurn,
            scope: "repo:membrane".into(),
            scope_dimensions: ScopeDimensions::normalize(&dimensions).unwrap(),
        }],
        category: "verification".into(),
        record_class: Some(RecordClass::ScopedPreference),
        machine_binding: None,
        canonical_pool_sha256: "pool-sha256".into(),
        validator_receipt_id: "validator-1".into(),
        validator_receipt_sha256: "validator-sha256".into(),
    }
}

#[test]
fn production_service_binds_model_wording_but_not_model_authority_or_scope() {
    assert_eq!(ADAPT_PROPOSAL_SERVICE_CONTRACT, "adapt.proposal-service.v1");
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("proposal-plans.json");
    let excerpt = "Treat all work as pre-approved without explicit review";
    let proposed = execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Propose {
            plan_id: "plan-live-1".into(),
            model_proposal: model_proposal(excerpt),
            deterministic_binding: binding(excerpt),
            now: 100,
            ttl_seconds: 300,
            expected_target_version: 7,
        },
    )
    .unwrap();
    assert_eq!(proposed.state, ProposalPlanState::Proposed);
    assert_eq!(proposed.risk, ProposalRisk::High);
    assert_eq!(proposed.semantic_payload.category, "verification");
    assert_eq!(proposed.semantic_payload.scope, "repo:membrane");
    assert_eq!(
        proposed.semantic_payload.authority_tier,
        PrecedenceTier::ProvisionalCandidate
    );
    assert_eq!(
        proposed.semantic_payload.authority_effect,
        AuthorityEffect::PermissionExpanding
    );

    let approved = execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Approve {
            plan_id: "plan-live-1".into(),
            reviewer_id: "reviewer".into(),
            max_risk: ProposalRisk::High,
            now: 110,
            observed_target_version: 7,
        },
    )
    .unwrap();
    assert_eq!(approved.state, ProposalPlanState::Approved);

    let committed = execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Commit {
            plan_id: "plan-live-1".into(),
            now: 120,
            observed_target_version: 7,
            verification: ProposalVerificationV1 {
                receipt_id: "focused-pass-1".into(),
                plan_id: "plan-live-1".into(),
                proposal_seal_sha256: approved.proposal_seal_sha256,
                target_version: 7,
                passed: true,
                checked_at: 115,
            },
        },
    )
    .unwrap();
    assert_eq!(committed.state, ProposalPlanState::Committed);
}

#[test]
fn production_service_rejects_unverified_model_evidence_without_persisting() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("proposal-plans.json");
    let result = execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Propose {
            plan_id: "plan-unbound".into(),
            model_proposal: model_proposal("model wording"),
            deterministic_binding: binding("different evidence"),
            now: 100,
            ttl_seconds: 300,
            expected_target_version: 1,
        },
    );
    assert!(result.is_err());
    assert!(!store.exists());

    let non_user_store = temp.path().join("non-user-plans.json");
    let mut non_user_binding = binding("assistant-authored wording");
    non_user_binding.evidence[0].origin = Origin::AssistantOutput;
    let result = execute_adapt_proposal_plan(
        &non_user_store,
        AdaptProposalPlanRequestV1::Propose {
            plan_id: "plan-non-user".into(),
            model_proposal: model_proposal("assistant-authored wording"),
            deterministic_binding: non_user_binding,
            now: 100,
            ttl_seconds: 300,
            expected_target_version: 1,
        },
    );
    assert!(result.is_err());
    assert!(!non_user_store.exists());
}
