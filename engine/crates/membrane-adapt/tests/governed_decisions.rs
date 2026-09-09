use membrane_adapt::comparison::*;
use membrane_adapt::delivery::select_preferences;
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

// BM06/BM07 collaboration: Cortex owns the durable projection and traversal
// (blueprint-membrane-amendment.md), Pull owns final admission; Adapt's own
// role is query-independent standing/scoped selection over already-admitted
// preference records via `delivery::select_preferences`. These tests exercise
// only the Adapt-side collaborator contract with existing public APIs.
#[test]
fn standing_preference_is_delivered_regardless_of_query_shape() {
    use membrane_adapt::record::{InfluenceClass, LifecycleState, PreferenceRecordV1, RecordClass};
    use membrane_adapt::scope::ScopeDimensions;

    let mut standing = PreferenceRecordV1::new_candidate(
        "Always run focused tests before claiming done",
        "verification",
        RecordClass::StandingPreference,
        "repo",
        ScopeDimensions::default(),
        1.0,
        vec!["ev-1".into()],
        "2026-09-09T00:00:00Z",
    )
    .unwrap();
    standing.lifecycle_state = LifecycleState::Active;
    standing.influence_class = InfluenceClass::BehavioralDirective;

    // An unrelated query context (distinct, unrelated scope dimensions) must
    // still receive the applicable standing preference: standing selection is
    // query-independent, not keyed to the caller's specific ask.
    let unrelated_query_context =
        ScopeDimensions::normalize(&[("task".into(), "unrelated-topic".into())].into()).unwrap();
    let result = select_preferences(&[standing.clone()], &unrelated_query_context, 8, "t");
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].id, standing.id);
    assert!(result.receipts.iter().any(|r| r.selected));
}

#[test]
fn arbitrary_memory_text_cannot_become_authoritative_preference() {
    use membrane_adapt::authority::AuthorityEffect;
    use membrane_adapt::record::{PreferenceRecordV1, RecordClass};
    use membrane_adapt::scope::ScopeDimensions;

    // Plain scratch/durable-memory text with no protective/restrictive shape
    // classifies as Neutral: it cannot be laundered into a security-weakening
    // or permission-expanding authority effect merely by being stored.
    let arbitrary_text = "the sky looked nice during the deploy window";
    assert_eq!(
        membrane_adapt::authority::classify_authority_effect(arbitrary_text),
        AuthorityEffect::Neutral
    );

    // Constructing a candidate record from that text still requires an
    // explicit RecordClass and passes through the same deterministic
    // classification; it never gains authority beyond what the text implies.
    let record = PreferenceRecordV1::new_candidate(
        arbitrary_text,
        "verification",
        RecordClass::ScopedPreference,
        "repo",
        ScopeDimensions::default(),
        1.0,
        vec!["ev-2".into()],
        "2026-09-09T00:00:00Z",
    )
    .unwrap();
    assert_eq!(record.authority_effect, AuthorityEffect::Neutral);
}
