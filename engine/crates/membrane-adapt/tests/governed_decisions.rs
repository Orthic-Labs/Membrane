use membrane_adapt::comparison::*;
use membrane_adapt::guard_rollout::*;

fn h(c: char) -> String {
    c.to_string().repeat(64)
}
fn comparison() -> CandidateComparisonV1 {
    let rows = |prefix: &str| {
        ["successful", "hard_negative", "nonapplicable", "failure"]
            .iter()
            .enumerate()
            .flat_map(|(i, stratum)| {
                let digest = if prefix == "dev" {
                    h(['1', '2', '3', '4'][i])
                } else {
                    h(['5', '6', '7', '8'][i])
                };
                ['a', 'b'].into_iter().map(move |c| CandidateCaseV1 {
                    candidate_sha256: h(c),
                    case_id: format!("{prefix}-{i}"),
                    case_sha256: digest.clone(),
                    stratum: stratum.to_string(),
                    receipt_id: format!("{prefix}-{i}-{c}"),
                    correct: true,
                    adherent: c == 'b' || *stratum != "failure",
                    recurred: false,
                    false_block: false,
                    authority_violation: false,
                    latency_ms: 10,
                    cost_microunits: 10,
                })
            })
            .collect()
    };
    CandidateComparisonV1 {
        schema_version: 1,
        comparison_id: "trial-1".into(),
        target: "skill:test".into(),
        target_version: 3,
        scope: "repo".into(),
        allowed_change_sha256: h('c'),
        baseline_sha256: h('a'),
        candidates: vec![h('b')],
        development_dataset_sha256: h('d'),
        test_dataset_sha256: h('e'),
        evaluator_sha256: h('f'),
        host_configuration_sha256: h('0'),
        limits: ComparisonLimitsV1 {
            candidates: 2,
            cases: 16,
            evaluator_calls: 32,
            proposal_iterations: 2,
            cost_microunits: 1000,
            elapsed_ms: 1000,
            concurrency: 2,
        },
        usage: ComparisonUsageV1 {
            evaluator_calls: 16,
            proposal_iterations: 1,
            cost_microunits: 160,
            elapsed_ms: 100,
            concurrency: 1,
        },
        cancelled: false,
        development: rows("dev"),
        frozen_test: rows("test"),
    }
}
#[test]
fn comparison_selects_without_admitting_or_activating() {
    let request = comparison();
    let d = compare(&request).unwrap();
    assert_eq!(d.disposition, ComparisonDisposition::CandidateSelected);
    assert_eq!(d.selected_sha256, h('b'));
    assert!(!d.activation_authorized);
    assert!(d.requires_independent_admission && d.requires_target_revalidation);
    assert_eq!(
        d.decision_sha256,
        compare(&request).unwrap().decision_sha256
    );
}
#[test]
fn comparison_stops_on_budget_and_cancellation() {
    let mut r = comparison();
    r.usage.cost_microunits = 1001;
    let d = compare(&r).unwrap();
    assert_eq!(d.disposition, ComparisonDisposition::BudgetExhausted);
    assert_eq!(d.selected_sha256, r.baseline_sha256);
    r.cancelled = true;
    assert_eq!(
        compare(&r).unwrap().disposition,
        ComparisonDisposition::Cancelled
    );
}
#[test]
fn final_test_cannot_become_a_search_set() {
    let mut r = comparison();
    r.frozen_test[0].case_sha256 = r.development[0].case_sha256.clone();
    assert!(compare(&r).is_err());
    let mut r = comparison();
    r.frozen_test[1].correct = false;
    assert_eq!(
        compare(&r).unwrap().disposition,
        ComparisonDisposition::Regression
    );
}
#[test]
fn successful_tasks_and_hard_negatives_cannot_be_sacrificed() {
    let mut r = comparison();
    r.development[1].authority_violation = true;
    let d = compare(&r).unwrap();
    assert_eq!(d.selected_sha256, r.baseline_sha256);
    let mut r = comparison();
    r.frozen_test.clear();
    assert_eq!(
        compare(&r).unwrap().disposition,
        ComparisonDisposition::InsufficientEvidence
    );
}
#[test]
fn no_improvement_is_a_terminal_baseline_decision() {
    let mut r = comparison();
    for row in &mut r.development {
        row.adherent = true;
    }
    assert_eq!(
        compare(&r).unwrap().disposition,
        ComparisonDisposition::NoImprovement
    );
}
fn transition() -> GuardTransitionRequestV1 {
    GuardTransitionRequestV1 {
        schema_version: 1,
        issue_id: "issue".into(),
        mitigation_sha256: h('a'),
        target: "skill".into(),
        target_sha256: h('b'),
        host_configuration_sha256: h('c'),
        current_scope: "repo".into(),
        proposed_scope: "repo".into(),
        current_stage: GuardStage::Reviewed,
        proposed_stage: GuardStage::Shadow,
        now_ms: 100,
        comparable_exposures: 0,
        evaluated_exposures: 0,
        false_blocks: 0,
        minimum_exposures: 10,
        maximum_false_block_bps: 10,
        rollback_ref: "rollback-v1".into(),
        evidence: [
            "review",
            "detector",
            "attribution",
            "target",
            "host_configuration",
        ]
        .into_iter()
        .map(|kind| GuardEvidenceV1 {
            kind: kind.into(),
            receipt_id: format!("r-{kind}"),
            receipt_sha256: h('d'),
            subject_sha256: h(match kind {
                "target" => 'b',
                "host_configuration" => 'c',
                _ => 'a',
            }),
            scope: "repo".into(),
            valid_until_ms: 200,
            passed: true,
        })
        .collect(),
    }
}
#[test]
fn shadow_eligibility_never_grants_permission() {
    let d = evaluate(&transition()).unwrap();
    assert!(d.eligible);
    assert!(!d.activation_authorized);
    assert!(d.host_authorization_required);
}
#[test]
fn blocking_cannot_skip_stages_or_expand_scope() {
    let mut r = transition();
    r.proposed_stage = GuardStage::ScopedBlocking;
    r.proposed_scope = "global".into();
    let d = evaluate(&r).unwrap();
    assert!(!d.eligible);
    assert!(d.reasons.iter().any(|r| r.contains("stage_transition")));
    assert!(d.reasons.iter().any(|r| r.contains("scope_change")));
}
#[test]
fn expiry_and_missing_evidence_fail_closed() {
    let mut r = transition();
    r.now_ms = 200;
    assert!(!evaluate(&r).unwrap().eligible);
    r.evidence.clear();
    assert!(!evaluate(&r).unwrap().eligible);
}
#[test]
fn narrowing_alias_and_path_sibling_do_not_widen() {
    use membrane_adapt::scope::ScopeDimensions;
    assert!(ScopeDimensions::normalize(
        &[("repo".into(), "a".into()), ("repo ".into(), "b".into())].into()
    )
    .is_err());
    let desired =
        ScopeDimensions::normalize(&[("path_prefix".into(), "src/auth".into())].into()).unwrap();
    let sibling =
        ScopeDimensions::normalize(&[("path_prefix".into(), "src/authentication".into())].into())
            .unwrap();
    assert!(!desired.matches(&sibling));
}
#[test]
fn no_exposure_is_not_effectiveness() {
    use membrane_adapt::outcomes::*;
    let mut ledger = OutcomeLedger::default();
    let e = ledger.record(
        "issue",
        "mitigation",
        RawOutcome::NoRecurrence,
        Exposure {
            opportunities: 0,
            baseline: 0,
        },
        "",
    );
    assert_eq!(e.adjusted, AdjustedOutcome::Indeterminate);
}
