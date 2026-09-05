use std::collections::BTreeMap;

use membrane_adapt::authority::{AuthorityEffect, PrecedenceTier};
use membrane_adapt::proposal_state::{
    semantic_target_sha256, ProposalPlanStore, ProposalRisk, ProposalStateError,
};
use membrane_adapt::record::{InfluenceClass, RecordClass};
use membrane_adapt::scope::ScopeDimensions;
use membrane_adapt::seal::{SemanticPayloadV1, SEAL_CONTRACT_VERSION};

fn payload(text: &str, scope: &str, category: &str) -> SemanticPayloadV1 {
    SemanticPayloadV1 {
        seal_contract_version: SEAL_CONTRACT_VERSION.into(),
        record_kind: "preference".into(),
        category: category.into(),
        canonical_text: text.into(),
        scope: scope.into(),
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
fn exact_semantic_replay_converges_without_second_pending_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("plans.json");
    let mut store = ProposalPlanStore::open(&path).unwrap();
    store
        .propose(
            "plan-1",
            payload("run focused verification", "repo-x", "verification"),
            ProposalRisk::Low,
            100,
            300,
            7,
        )
        .unwrap();
    assert_eq!(store.revision(), 1);

    let replay = store
        .propose(
            "plan-2",
            payload("run focused verification", "repo-x", "verification"),
            ProposalRisk::Low,
            101,
            300,
            7,
        )
        .unwrap();
    assert_eq!(replay.plan_id, "plan-1");
    assert_eq!(store.revision(), 1);
    assert!(store.get("plan-2").is_none());
}

#[test]
fn competing_variant_for_same_target_and_version_surfaces_typed_conflict() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("plans.json");
    let mut store = ProposalPlanStore::open(&path).unwrap();
    let first_payload = payload("run focused verification", "repo-x", "verification");
    let target = semantic_target_sha256(&first_payload);
    store
        .propose("plan-1", first_payload, ProposalRisk::Low, 100, 300, 7)
        .unwrap();

    let error = store
        .propose(
            "plan-2",
            payload("run all verification", "repo-x", "verification"),
            ProposalRisk::Low,
            101,
            300,
            7,
        )
        .unwrap_err();
    assert_eq!(
        error,
        ProposalStateError::TargetConflict {
            target_sha256: target,
            target_version: 7,
            existing_plan_id: "plan-1".into(),
        }
    );
    assert_eq!(store.revision(), 1);
}

#[test]
fn target_version_is_part_of_apply_eligibility_exclusion() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("plans.json");
    let mut store = ProposalPlanStore::open(&path).unwrap();
    store
        .propose(
            "plan-v7",
            payload("run focused verification", "repo-x", "verification"),
            ProposalRisk::Low,
            100,
            300,
            7,
        )
        .unwrap();
    store
        .propose(
            "plan-v8",
            payload("run all verification", "repo-x", "verification"),
            ProposalRisk::Low,
            101,
            300,
            8,
        )
        .unwrap();
    assert_eq!(store.revision(), 2);
}

#[test]
fn different_semantic_targets_do_not_block_each_other() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("plans.json");
    let mut store = ProposalPlanStore::open(&path).unwrap();
    store
        .propose(
            "verification",
            payload("run verification", "repo-x", "verification"),
            ProposalRisk::Low,
            100,
            300,
            7,
        )
        .unwrap();
    store
        .propose(
            "formatting",
            payload("prefer jsonl", "repo-x", "formatting"),
            ProposalRisk::Low,
            101,
            300,
            7,
        )
        .unwrap();
    store
        .propose(
            "other-scope",
            payload("run all verification", "repo-y", "verification"),
            ProposalRisk::Low,
            102,
            300,
            7,
        )
        .unwrap();
    assert_eq!(store.revision(), 3);
}

#[test]
fn no_longer_apply_eligible_expired_candidate_does_not_hold_target_slot() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("plans.json");
    let mut store = ProposalPlanStore::open(&path).unwrap();
    store
        .propose(
            "short-lived",
            payload("first wording", "repo-x", "verification"),
            ProposalRisk::Low,
            10,
            5,
            7,
        )
        .unwrap();
    store
        .propose(
            "replacement",
            payload("replacement wording", "repo-x", "verification"),
            ProposalRisk::Low,
            15,
            300,
            7,
        )
        .unwrap();
    assert_eq!(store.revision(), 2);
}
