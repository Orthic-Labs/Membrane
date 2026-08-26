//! Scenarios 3 + 8: three-gate separation and the model/proposal boundary.

use std::collections::BTreeSet;

use membrane_adapt::canonical::sha256_hex;
use membrane_adapt::gates::{
    gate1_pass_implies_nothing, ContextAdmissionRecord, ContextDecision, CortexAdmissionEnvelope,
    ProposalEligibilityDecision,
};
use membrane_adapt::model_boundary::{ModelExtractionProposal, ModelProposalError};
use membrane_adapt::record::InfluenceClass;

#[test]
fn eligibility_pass_does_not_create_durable_or_delivered_state() {
    let decision = ProposalEligibilityDecision {
        eligible: true,
        reason: "clean candidate".into(),
    };
    let (durable, delivered) = gate1_pass_implies_nothing(&decision);
    assert!(!durable && !delivered);
}

#[test]
fn cortex_envelope_is_not_durable_until_cortex_decides() {
    let env = CortexAdmissionEnvelope {
        envelope_id: "e".into(),
        record_kind: "preference".into(),
        seal_digest: sha256_hex(b"payload"),
        influence_class: InfluenceClass::Provisional,
        idempotency_key: "idem".into(),
        installation_id: "inst".into(),
        cortex_verdict: None,
    };
    assert!(!env.is_durable());
}

#[test]
fn context_delivery_requires_explicit_planner_inclusion() {
    let rec = ContextAdmissionRecord {
        cortex_record_ref: "r".into(),
        authority_ok: true,
        fresh: true,
        sufficient: true,
        within_budget: true,
        planner_decision: Some(ContextDecision::Omitted),
    };
    assert!(!rec.delivered());
}

#[test]
fn model_proposal_carries_no_authority_fields() {
    let p = ModelExtractionProposal {
        proposer_id: "m".into(),
        rule_text: "always run focused tests".into(),
        category_hint: "verification".into(),
        scope_hint: "repo-x".into(),
        bound_evidence_ids: vec!["ev-1".into()],
        bound_evidence_excerpt: "always run focused tests".into(),
    };
    let json = serde_json::to_string(&p).unwrap();
    for forbidden in ["authority", "permission", "precedence", "signal_strength"] {
        assert!(!json.contains(forbidden), "proposal leaked {forbidden}");
    }
}

#[test]
fn unverified_evidence_bindings_are_rejected() {
    let p = ModelExtractionProposal {
        proposer_id: "m".into(),
        rule_text: "always run focused tests".into(),
        category_hint: "verification".into(),
        scope_hint: "repo-x".into(),
        bound_evidence_ids: vec!["ev-1".into()],
        bound_evidence_excerpt: "always run focused tests".into(),
    };
    // Wrong digest: the claimed binding does not check out.
    let wrong: Vec<(String, String)> = vec![("ev-1".into(), sha256_hex(b"other text"))];
    assert_eq!(
        p.verify_bindings(&wrong),
        Err(ModelProposalError::UnboundEvidence)
    );
    // Empty authenticated set: refused outright.
    assert_eq!(
        p.verify_bindings(&Vec::<(String, String)>::new()),
        Err(ModelProposalError::UnboundEvidence)
    );
    // Matching digest passes — but only because deterministic code confirmed
    // it, not because the model asserted it.
    let good: Vec<(String, String)> =
        vec![("ev-1".into(), sha256_hex(b"always run focused tests"))];
    assert_eq!(p.verify_bindings(&good), Ok(vec!["ev-1".to_string()]));
    let _ = BTreeSet::<String>::new();
}
