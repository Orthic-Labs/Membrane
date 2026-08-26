//! Scenarios 5–7: remediation separation + precision gate, outcome ledger
//! reopen logic & exposure adjustment, context cost class separation.

use std::collections::BTreeSet;

use membrane_adapt::context_cost::{ContextCostReportV1, CostAmount, CostClass};
use membrane_adapt::outcomes::{Exposure, OutcomeLedger, RawOutcome};
use membrane_adapt::remediation::{RemediationEffect, RemediationProposalV1};

fn proposal(effect: RemediationEffect) -> RemediationProposalV1 {
    RemediationProposalV1::build(
        "issue-1",
        "repeated_ask",
        effect,
        "add a guard that runs focused tests before completion claims",
        vec!["ev-1".to_string()],
    )
}

#[test]
fn remediation_is_a_separate_artifact_from_issues_and_taste() {
    // Proposal ID binds issue + effect + text; it is not an issue ID and not
    // a preference ID.
    let p = proposal(RemediationEffect::ProcessChange);
    assert!(p.proposal_id.starts_with("rem_"));
    assert_ne!(p.proposal_id, p.issue_id);
}

#[test]
fn taste_candidate_requires_authenticated_user_evidence() {
    let p = proposal(RemediationEffect::TasteCandidate);
    let mut authenticated = BTreeSet::new();
    assert!(p.validate_evidence(&authenticated).is_err());
    authenticated.insert("ev-1".into());
    assert!(p.validate_evidence(&authenticated).is_ok());
}

#[test]
fn precision_gate_blocks_families_below_threshold() {
    let p = proposal(RemediationEffect::GuardrailAddition);
    assert!(p.precision_gate(Some(0.94)).is_err());
    assert!(p.precision_gate(Some(0.95)).is_ok());
    // Unknown family precision blocks (fail closed).
    assert!(p.precision_gate(None).is_err());
}

#[test]
fn ledger_reopens_only_on_same_signature_recurrence() {
    let mut ledger = OutcomeLedger::default();
    let high_exposure = Exposure {
        opportunities: 9,
        baseline: 10,
    };
    ledger.record("i", "m", RawOutcome::NoRecurrence, high_exposure, "");
    assert!(!ledger.should_reopen("i"));
    ledger.record(
        "i",
        "m",
        RawOutcome::RecurredDifferentSignature,
        high_exposure,
        "",
    );
    assert!(!ledger.should_reopen("i"));
    ledger.record(
        "i",
        "m",
        RawOutcome::RecurredSameSignature,
        high_exposure,
        "",
    );
    assert!(ledger.should_reopen("i"));
}

#[test]
fn exposure_adjusts_outcome_class() {
    let mut ledger = OutcomeLedger::default();
    let strong = ledger.record(
        "a",
        "m",
        RawOutcome::NoRecurrence,
        Exposure {
            opportunities: 10,
            baseline: 10,
        },
        "",
    );
    assert_eq!(format!("{:?}", strong.adjusted), "Effective");
    let weak = ledger.record(
        "b",
        "m",
        RawOutcome::NoRecurrence,
        Exposure {
            opportunities: 3,
            baseline: 10,
        },
        "",
    );
    assert_eq!(format!("{:?}", weak.adjusted), "ProbablyEffective");
}

#[test]
fn cost_classes_are_reported_separately() {
    let mut report = ContextCostReportV1::new("inst");
    report
        .attribute(
            "r1",
            CostClass::Measured,
            CostAmount {
                bytes: 100,
                tokens: Some(20),
            },
        )
        .unwrap();
    report
        .attribute(
            "r2",
            CostClass::Inferred,
            CostAmount {
                bytes: 50,
                tokens: None,
            },
        )
        .unwrap();
    report
        .attribute(
            "r3",
            CostClass::Unattributed,
            CostAmount {
                bytes: 25,
                tokens: None,
            },
        )
        .unwrap();
    assert_eq!(report.measured_bytes(), 100);
    assert_eq!(report.inferred_bytes(), 50);
    assert_eq!(report.unattributed_bytes(), 25);
}
