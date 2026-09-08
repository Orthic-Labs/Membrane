//! Focused closure coverage for proposal lineage, attribution, applicability,
//! and exposure-aware recurrence semantics.

use membrane_adapt::attribution::{
    AlternativeCause, AttributionGateContextV1, AttributionSupportV1, CounterfactualPreventability,
    EvaluatorApplicability, EvaluatorOutcomeRefV1, InstructionState, InterventionAttributionV1,
};
use membrane_adapt::insights::recurrence::{
    apply_mitigation_outcome, MitigationOutcomeV1, RecurrenceOutcomeLedgerV1,
};
use membrane_adapt::insights::{InsightIssueV1, IssueState};
use membrane_adapt::outcomes::{Exposure, RawOutcome};
use membrane_adapt::remediation::{
    consumable_for_variant_generation, InterventionTarget, ProposalKindProvenance,
    RemediationEffect, RemediationProposalKind, RemediationProposalV1,
    SealedRemediationProposalV1,
};

fn issue_id() -> String {
    format!("ii_{}", "1".repeat(64))
}

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn attribution() -> InterventionAttributionV1 {
    let issue = issue_id();
    let surface = digest('a');
    InterventionAttributionV1::seal(
        &issue,
        &membrane_adapt::attribution::attribution_id_for(
            &issue,
            InterventionTarget::SkillOrProcedure,
            Some(&surface),
        ),
        InterventionTarget::SkillOrProcedure,
        Some("skill:review"),
        Some(&surface),
        InstructionState::Missing,
        CounterfactualPreventability::Supported,
        vec![AlternativeCause::None],
        AttributionSupportV1 {
            episode_count: membrane_adapt::attribution::CoverageValue::Measured(4),
            independent_session_count: membrane_adapt::attribution::CoverageValue::Measured(3),
            severity: membrane_adapt::attribution::CoverageValue::Measured("high".into()),
            recurrence_rate: membrane_adapt::attribution::CoverageValue::Measured("0.4".into()),
        },
        vec!["h4-1".into()],
        vec![EvaluatorOutcomeRefV1 {
            outcome_id: "eval-1".into(),
            evaluator: "completion".into(),
            applicability: EvaluatorApplicability::Applicable,
        }],
        "diagnostic only",
        &AttributionGateContextV1 {
            examined_surface_digest: Some(&surface),
            support_threshold: 2,
            alters_behavioral_contract: true,
        },
    )
    .unwrap()
}

#[test]
fn explicit_kind_is_independent_and_legacy_is_marked() {
    let proposal = RemediationProposalV1::build_with_kind(
        &issue_id(),
        "repeated_ask",
        RemediationProposalKind::WorkflowChangeProposal,
        RemediationEffect::ProcessChange,
        InterventionTarget::SkillOrProcedure,
        "require review",
        vec![],
    );
    let sealed = SealedRemediationProposalV1::seal(
        &proposal,
        "requires_human_review",
        "proposal only",
        "policy",
        "redaction",
        None,
        vec![],
        "test",
        "2026-09-08T00:00:00Z",
    )
    .unwrap();
    assert_eq!(sealed.payload.proposal_kind_provenance, ProposalKindProvenance::Explicit);

    let legacy = RemediationProposalV1::build(
        &issue_id(),
        "repeated_ask",
        RemediationEffect::ProcessChange,
        "require review",
        vec![],
    );
    let legacy_sealed = SealedRemediationProposalV1::seal(
        &legacy,
        "requires_human_review",
        "proposal only",
        "policy",
        "redaction",
        None,
        vec![],
        "test",
        "2026-09-08T00:00:00Z",
    )
    .unwrap();
    assert_eq!(
        legacy_sealed.payload.proposal_kind_provenance,
        ProposalKindProvenance::LegacyDerived
    );
}

#[test]
fn bound_attribution_and_digest_gate_variant_consumption() {
    let attr = attribution();
    let proposal = RemediationProposalV1::build_with_target(
        &issue_id(),
        "repeated_ask",
        RemediationEffect::ProcessChange,
        InterventionTarget::SkillOrProcedure,
        "require review",
        vec![],
    );
    let sealed = SealedRemediationProposalV1::seal(
        &proposal,
        "requires_human_review",
        "proposal only",
        "policy",
        "redaction",
        None,
        vec![],
        "test",
        "2026-09-08T00:00:00Z",
    )
    .unwrap()
    .bind_attribution(attr.clone(), Some(&digest('a')))
    .unwrap();
    assert!(consumable_for_variant_generation(&sealed, None, Some(&digest('a'))).is_ok());
    assert!(consumable_for_variant_generation(&sealed, None, Some(&digest('b'))).is_err());
    assert_eq!(attr.evaluator_applicability_counts().denominator(), 1);
}

#[test]
fn recurrence_is_replay_safe_and_timestamp_ordered() {
    let mk = |id: &str, raw: RawOutcome, at: &str| MitigationOutcomeV1 {
        outcome_id: id.into(),
        issue_id: issue_id(),
        mitigation_proposal_id: "rem_1".into(),
        mitigation_version: "v1".into(),
        baseline_version: "b1".into(),
        exposure: Exposure {
            opportunities: 10,
            baseline: 10,
        },
        raw,
        applicability: EvaluatorApplicability::Applicable,
        observed_at: Some(at.into()),
    };
    let first = mk("o1", RawOutcome::RecurredSameSignature, "2026-09-08T01:00:00Z");
    let later = mk("o2", RawOutcome::NoRecurrence, "2026-09-08T02:00:00Z");
    let mut ledger = RecurrenceOutcomeLedgerV1::default();
    ledger.record(later).unwrap();
    ledger.record(first.clone()).unwrap();
    ledger.record(first).unwrap();
    assert!(!ledger.should_reopen(&issue_id(), "v1"));
}

#[test]
fn insufficient_outcome_does_not_reopen_or_invent_count() {
    let issue = InsightIssueV1 {
        schema_version: "adapt.insight-issue.v1".into(),
        issue_id: issue_id(),
        family: "f".into(),
        recurrence_signature: "s".into(),
        canonical_description: "d".into(),
        applicability: Default::default(),
        episode_ids: vec!["a".into(), "b".into()],
        recurrence_count: 2,
        distinct_sessions: 2,
        first_seen: None,
        last_seen: None,
        confidence: 0.5,
        state: IssueState::Mitigated,
        candidate_mechanisms: vec![],
        mitigation_links: vec![],
        recurrence_after_mitigation: 0,
        honesty_limit: "diagnostic".into(),
    };
    let outcome = MitigationOutcomeV1 {
        outcome_id: "unknown".into(),
        issue_id: issue_id(),
        mitigation_proposal_id: "rem_1".into(),
        mitigation_version: "v1".into(),
        baseline_version: "b1".into(),
        exposure: Exposure {
            opportunities: 10,
            baseline: 10,
        },
        raw: RawOutcome::RecurredSameSignature,
        applicability: EvaluatorApplicability::InsufficientEvidence,
        observed_at: None,
    };
    let mut ledger = RecurrenceOutcomeLedgerV1::default();
    ledger.record(outcome).unwrap();
    assert_eq!(ledger.recurrence_count(&issue_id(), "v1"), 0);
    assert_eq!(apply_mitigation_outcome(issue, &ledger, "v1").unwrap().state, IssueState::Mitigated);
}
