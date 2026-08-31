use std::collections::BTreeMap;

use membrane_adapt::authority::{AuthorityEffect, PrecedenceTier};
use membrane_adapt::proposal_state::{
    ProposalPlanState, ProposalPlanStore, ProposalRisk, ProposalStateError,
    ProposalVerificationV1,
};
use membrane_adapt::record::{InfluenceClass, RecordClass};
use membrane_adapt::scope::ScopeDimensions;
use membrane_adapt::seal::{SemanticPayloadV1, SEAL_CONTRACT_VERSION};

fn payload(text: &str) -> SemanticPayloadV1 {
    SemanticPayloadV1 {
        seal_contract_version: SEAL_CONTRACT_VERSION.into(),
        record_kind: "preference".into(),
        category: "verification".into(),
        canonical_text: text.into(),
        scope: "repo-x".into(),
        scope_dimensions: ScopeDimensions::normalize(&BTreeMap::new()).unwrap(),
        authority_tier: PrecedenceTier::ExplicitScopedUserPreference,
        authority_effect: AuthorityEffect::Neutral,
        influence_class: InfluenceClass::Provisional,
        record_class: Some(RecordClass::StandingPreference),
        machine_binding: None,
        source_evidence_digests: vec!["evidence-sha".into()],
        canonical_pool_sha256: "pool-sha".into(),
        admission_policy_version: "adapt.admission.v1".into(),
        validator_receipt_id: "validator-1".into(),
        validator_receipt_sha256: "validator-sha".into(),
        redaction_contract_version: "membrane.redaction.v1".into(),
        provenance_contract_version: "adapt.provenance.v2".into(),
    }
}

#[test]
fn sealed_plan_persists_propose_approve_commit_with_guards() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("plans.json");
    let mut store = ProposalPlanStore::open(&path).unwrap();
    let plan = store
        .propose(
            "plan-1",
            payload("run focused verification"),
            ProposalRisk::High,
            100,
            300,
            7,
        )
        .unwrap();
    let proposal_seal = plan.proposal_seal_sha256.clone();
    assert_eq!(plan.state, ProposalPlanState::Proposed);
    assert_eq!(store.revision(), 1);

    let mut store = ProposalPlanStore::open(&path).unwrap();
    assert_eq!(
        store.approve("plan-1", "reviewer", ProposalRisk::Medium, 110, 7),
        Err(ProposalStateError::RiskApprovalInsufficient)
    );
    assert!(matches!(
        store.approve("plan-1", "reviewer", ProposalRisk::High, 110, 8),
        Err(ProposalStateError::VersionConflict { .. })
    ));
    store
        .approve("plan-1", "reviewer", ProposalRisk::High, 110, 7)
        .unwrap();

    let rejected = ProposalVerificationV1 {
        receipt_id: "verify-1".into(),
        plan_id: "other-plan".into(),
        proposal_seal_sha256: proposal_seal.clone(),
        target_version: 7,
        passed: true,
        checked_at: 115,
    };
    assert_eq!(
        store.commit("plan-1", 120, 7, rejected),
        Err(ProposalStateError::VerificationRejected)
    );
    let accepted = ProposalVerificationV1 {
        receipt_id: "verify-1".into(),
        plan_id: "plan-1".into(),
        proposal_seal_sha256: proposal_seal,
        target_version: 7,
        passed: true,
        checked_at: 115,
    };
    store.commit("plan-1", 120, 7, accepted).unwrap();

    let reopened = ProposalPlanStore::open(&path).unwrap();
    let committed = reopened.get("plan-1").unwrap();
    assert_eq!(committed.state, ProposalPlanState::Committed);
    assert_eq!(committed.committed_at, Some(120));
    assert!(committed.approval.is_some() && committed.verification.is_some());
}

#[test]
fn expiry_is_persisted_and_stale_writers_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("plans.json");
    let mut first = ProposalPlanStore::open(&path).unwrap();
    let mut stale = ProposalPlanStore::open(&path).unwrap();
    first
        .propose("expiring", payload("one"), ProposalRisk::Low, 10, 5, 1)
        .unwrap();
    assert!(matches!(
        stale.propose("stale", payload("two"), ProposalRisk::Low, 10, 5, 1),
        Err(ProposalStateError::ConcurrentModification { .. })
    ));
    assert_eq!(
        first.approve("expiring", "reviewer", ProposalRisk::Low, 15, 1),
        Err(ProposalStateError::Expired("expiring".into()))
    );
    let reopened = ProposalPlanStore::open(&path).unwrap();
    assert_eq!(
        reopened.get("expiring").unwrap().state,
        ProposalPlanState::Expired
    );
}

#[test]
fn tampered_sealed_payload_is_rejected_on_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("plans.json");
    let mut store = ProposalPlanStore::open(&path).unwrap();
    store
        .propose("sealed", payload("original"), ProposalRisk::Low, 1, 60, 1)
        .unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["plans"]["sealed"]["semantic_payload"]["canonical_text"] =
        serde_json::Value::String("tampered".into());
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(
        ProposalPlanStore::open(&path),
        Err(ProposalStateError::Corrupt(_))
    ));
}
