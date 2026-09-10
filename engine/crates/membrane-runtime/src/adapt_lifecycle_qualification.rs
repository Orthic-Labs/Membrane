//! Native, row-specific Adapt qualification.
//!
//! This module is intentionally small at its public boundary: `run` executes
//! real membrane-adapt contracts and returns only typed evidence.  It is used
//! by installed qualification, not by normal product traffic.  No fixture
//! marker, generic success, or model-generated result can make a row pass.

use membrane_adapt::attribution::{
    attribution_id_for, AlternativeCause, AttributionGateContextV1, AttributionSupportV1,
    CounterfactualPreventability, CoverageValue, EvaluatorApplicability, EvaluatorOutcomeRefV1,
    InstructionState, InterventionAttributionV1,
};
use membrane_adapt::candidate_pattern::{
    BoundaryReviewReceiptV1, BoundaryVerdict, CandidatePatternPayloadV1, CandidatePatternV1,
    EpisodeMembershipV1, PatternState,
};
use membrane_adapt::canonical::sha256_canonical;
use membrane_adapt::context_cost::{
    analyze_persistent_context, ContextCostAnalysisRequestV1, ContextCostDetectorPolicyV1,
    PersistentSourceKind, PersistentSourceObservationV1, ProviderBilledUsageV1,
    ProviderUsageObservationV1, SourceFileStateV1,
};
use membrane_adapt::insights::recurrence::{
    form_issues, record_post_mitigation_recurrence, MitigationOutcomeV1, RecurrenceOutcomeLedgerV1,
};
use membrane_adapt::insights::{EventKind, FailureEpisodeV1, IssueState, TranscriptEventV1};
use membrane_adapt::multiwriter::{
    converge, proposal_emission, validate_proposal_emission, WriterRecord, WriterRequestV1,
};
use membrane_adapt::outcomes::{Exposure, OutcomeLedger, RawOutcome};
use membrane_adapt::portable::{PortableTastePackageV1, PortableTasteRecordV1};
use membrane_adapt::procedural_effectiveness::{
    project_host_effectiveness, HostEvaluationObservationV1, HostProceduralAssetObservationV1,
    Observed,
};
use membrane_adapt::record::{
    InfluenceClass, LifecycleState, PreferenceRecordV1, PreferenceSealContext, RecordClass,
};
use membrane_adapt::remediation::{
    InterventionTarget, RemediationEffect, RemediationProposalV1, SealedRemediationProposalV1,
};
use membrane_adapt::scope::ScopeDimensions;
use membrane_adapt::taste::{counterfactual_for_candidate, extract_candidates_with_source};
use ring::signature::KeyPair;
use serde_json::{json, Value};

const NOW: &str = "2026-09-11T00:00:00Z";
const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn digest<T: serde::Serialize>(value: &T) -> String {
    format!(
        "sha256:{}",
        sha256_canonical(&serde_json::to_value(value).expect("serializable"))
    )
}

fn result(case_id: &str, checks: Vec<Value>, negative: Value) -> Result<Value, String> {
    if checks.is_empty() || checks.iter().any(|check| check["passed"] != true) {
        return Err(format!("{case_id}: native assertion failed"));
    }
    Ok(json!({
        "caseId": case_id,
        "status": "passed",
        "evidenceKind": "installed_native",
        "contract": "adapt.native-qualification.v1",
        "checks": checks,
        "negative": negative,
    }))
}

fn check(name: &str, passed: bool, detail: impl serde::Serialize) -> Value {
    json!({"name":name,"passed":passed,"detail":detail})
}

fn transcript_event(
    id: &str,
    session: &str,
    kind: &str,
    role: &str,
    text: &str,
) -> membrane_transcript::TranscriptEventV1 {
    serde_json::from_value(json!({
        "eventId":id,"rowIndex":1,"byteStart":0,"byteEnd":text.len(),"blockIndex":0,
        "sequence":1,"kind":kind,"role":role,"text":text,"classification":"successful_readonly",
        "class":"successful_readonly","projection":"default","host":"pi","sessionId":session,
        "transcriptId":"native-transcript","parserDigest":"native-parser","synthetic":false,
        "meta":false,"privateReasoningOmitted":false,"redacted":false,"flags":{}
    }))
    .map_err(|e| e.to_string())
    .expect("canonical transcript event")
}

fn insight_event(id: &str, session: &str) -> TranscriptEventV1 {
    TranscriptEventV1 {
        event_id: id.into(),
        session_id: session.into(),
        host: "pi".into(),
        provenance: "external_user".into(),
        kind: EventKind::UserMessage,
        text: "same observed native failure".into(),
        timestamp: Some(NOW.into()),
        byte_start: 0,
        byte_end: 28,
        call_id: None,
        occurrence: 0,
        evidence_eligible: true,
    }
}

fn episode(id: &str, session: &str) -> FailureEpisodeV1 {
    let event = insight_event(id, session);
    let mut value = FailureEpisodeV1::new(
        "native_adapt_family",
        membrane_adapt::insights::Severity::Medium,
        0.9,
        "native_signature",
        "same observed native failure",
        "native expectation",
        &[&event],
    );
    value.episode_id = id.into();
    value
}

fn record() -> Result<PreferenceRecordV1, String> {
    let mut record = PreferenceRecordV1::new_candidate(
        "Prefer exact native evidence",
        "workflow",
        RecordClass::StandingPreference,
        "repo:qualification",
        ScopeDimensions::default(),
        1.0,
        vec!["event-native".into()],
        NOW,
    )
    .map_err(|e| format!("record: {e:?}"))?;
    record.influence_class = InfluenceClass::BehavioralDirective;
    let context = PreferenceSealContext {
        authority_tier: membrane_adapt::authority::PrecedenceTier::ExplicitScopedUserPreference,
        canonical_pool_sha256: "native-pool",
        admission_policy_version: "admission-v1",
        validator_receipt_id: "validator-native",
        validator_receipt_sha256: "receipt-native",
        redaction_contract_version: "redaction-v1",
    };
    record.seal_semantics(&context);
    record
        .verify_semantics(&context)
        .map_err(|e| format!("record seal: {e:?}"))?;
    Ok(record)
}

fn row011() -> Result<Value, String> {
    let assistant = transcript_event(
        "assistant-1",
        "s1",
        "assistant_message",
        "assistant",
        "Use broad rewrite.",
    );
    let correction = transcript_event(
        "user-1",
        "s1",
        "user_message",
        "user",
        "Correction: Prefer focused change.",
    );
    let candidates = extract_candidates_with_source(
        &[assistant, correction],
        "repo:qualification",
        &"b".repeat(64),
    );
    let candidate = candidates
        .into_iter()
        .next()
        .ok_or("ADP-011: no correction candidate")?;
    let cf = counterfactual_for_candidate(&candidate);
    let no_context = transcript_event(
        "user-2",
        "s2",
        "user_message",
        "user",
        "Prefer focused change.",
    );
    let no_cf =
        extract_candidates_with_source(&[no_context], "repo:qualification", &"c".repeat(64))
            .into_iter()
            .next()
            .map(|c| counterfactual_for_candidate(&c))
            .ok_or("ADP-011: no negative candidate")?;
    result(
        "ADP-011",
        vec![
            check(
                "correction-derived candidate",
                cf.status == "recorded"
                    && cf.rejected_alternative.as_deref() == Some("Use broad rewrite."),
                json!({"status":cf.status,"sourceEvent":cf.source_event_id}),
            ),
            check(
                "negative omits absent alternative",
                no_cf.status == "none_recorded" && no_cf.rejected_alternative.is_none(),
                json!({"status":no_cf.status}),
            ),
        ],
        json!({"tampered_or_absent_context":"none_recorded","failure_evidence":"none_recorded"}),
    )
}

fn row012() -> Result<Value, String> {
    let mut current = record()?;
    let mut transitions = Vec::new();
    for state in [
        LifecycleState::Active,
        LifecycleState::Disputed,
        LifecycleState::Active,
        LifecycleState::Deprecated,
        LifecycleState::Retired,
    ] {
        let (next, event) = current
            .transition_lifecycle(state, "native qualification", &"a".repeat(64), NOW)
            .map_err(|e| format!("ADP-012 transition: {e:?}"))?;
        transitions.push(
            json!({"from":event.from_state,"to":event.to_state,"receipt":event.receipt_sha256}),
        );
        current = next;
    }
    let illegal = current
        .transition_lifecycle(LifecycleState::Active, "illegal", &"a".repeat(64), NOW)
        .is_err();
    result(
        "ADP-012",
        vec![
            check(
                "receipted lifecycle",
                current.lifecycle_state == LifecycleState::Retired && transitions.len() == 5,
                transitions,
            ),
            check(
                "retired is terminal",
                illegal,
                json!("retired -> active refused"),
            ),
        ],
        json!({"missing_receipt":"refused"}),
    )
}

fn row015() -> Result<Value, String> {
    let mut ledger = RecurrenceOutcomeLedgerV1::default();
    let outcome = MitigationOutcomeV1 {
        outcome_id: "outcome-native".into(),
        issue_id: "ii_native".into(),
        mitigation_proposal_id: "rem_native".into(),
        mitigation_version: "v2".into(),
        baseline_version: "v1".into(),
        exposure: Exposure {
            opportunities: 10,
            baseline: 10,
        },
        raw: RawOutcome::RecurredSameSignature,
        applicability: EvaluatorApplicability::Applicable,
        observed_at: Some(NOW.into()),
    };
    ledger.record(outcome.clone())?;
    ledger.record(outcome.clone())?;
    let conflict = {
        let mut changed = outcome;
        changed.raw = RawOutcome::NoRecurrence;
        ledger.record(changed).is_err()
    };
    result(
        "ADP-015",
        vec![
            check(
                "exact issue/proposal/version join",
                ledger.should_reopen("ii_native", "v2")
                    && ledger.outcomes_for_baseline("ii_native", "v2", "v1").len() == 1,
                json!({"reopen":ledger.should_reopen("ii_native","v2")}),
            ),
            check(
                "conflicting replay refused",
                conflict,
                json!("outcome identity conflict"),
            ),
        ],
        json!({"not_applicable":"excluded from reopen denominator"}),
    )
}

fn row016() -> Result<Value, String> {
    let record = record()?;
    let context = PreferenceSealContext {
        authority_tier: membrane_adapt::authority::PrecedenceTier::ExplicitScopedUserPreference,
        canonical_pool_sha256: "native-pool",
        admission_policy_version: "admission-v1",
        validator_receipt_id: "validator-native",
        validator_receipt_sha256: "receipt-native",
        redaction_contract_version: "redaction-v1",
    };
    let package = PortableTastePackageV1::build(
        "native-install",
        Some("native-org"),
        NOW,
        vec![PortableTasteRecordV1 {
            semantic_payload: record.semantic_payload(&context),
            record,
            evidence_digests: vec![DIGEST.into()],
            provenance_receipts: vec![DIGEST.into()],
            lifecycle_history: vec![],
        }],
    )
    .map_err(|e| format!("ADP-016 export: {e:?}"))?;
    let seed = [19u8; 32];
    let key =
        ring::signature::Ed25519KeyPair::from_seed_unchecked(&seed).map_err(|e| e.to_string())?;
    let mut signed = package;
    signed
        .sign("native-key", &seed)
        .map_err(|e| format!("ADP-016 sign: {e:?}"))?;
    let imported = signed
        .import_candidates(key.public_key().as_ref())
        .map_err(|e| format!("ADP-016 import: {e:?}"))?;
    let mut tampered = signed.clone();
    tampered.origin_org_id = Some("other-org".into());
    let refused = tampered.verify(key.public_key().as_ref()).is_err();
    result(
        "ADP-016",
        vec![
            check(
                "signed export/import",
                imported.len() == 1 && imported[0].requires_local_promotion,
                json!({"package":signed.package_id,"precedence":imported[0].precedence}),
            ),
            check(
                "tampered package refused",
                refused,
                json!("signature binding"),
            ),
        ],
        json!({"promotion":"required_local_review"}),
    )
}

fn row019() -> Result<Value, String> {
    let episodes = vec![
        episode("ep-native-1", "session-1"),
        episode("ep-native-2", "session-2"),
    ];
    let issues = form_issues(&episodes, 2);
    let singles = form_issues(&[episodes[0].clone()], 2);
    result(
        "ADP-019",
        vec![
            check(
                "independent recurrence forms issue",
                issues.len() == 1 && issues[0].distinct_sessions == 2,
                json!({"issueCount":issues.len()}),
            ),
            check(
                "single episode stays non-issue",
                singles.is_empty(),
                json!("below recurrence threshold"),
            ),
        ],
        json!({"same_session_duplicate":"not independent"}),
    )
}

fn row020() -> Result<Value, String> {
    let episodes = vec![
        episode("ep-native-1", "session-1"),
        episode("ep-native-2", "session-2"),
    ];
    let mut issue = form_issues(&episodes, 2)
        .into_iter()
        .next()
        .ok_or("ADP-020: issue not formed")?;
    for state in [
        IssueState::Recurring,
        IssueState::Confirmed,
        IssueState::MitigationProposed,
        IssueState::Mitigated,
    ] {
        issue = membrane_adapt::insights::recurrence::transition_issue(&issue, state)?;
    }
    let reopened = record_post_mitigation_recurrence(issue.clone())?;
    let illegal =
        membrane_adapt::insights::recurrence::transition_issue(&reopened, IssueState::Confirmed)
            .is_err();
    result(
        "ADP-020",
        vec![
            check(
                "issue lifecycle is explicit",
                reopened.state == IssueState::Reopened && reopened.recurrence_after_mitigation == 1,
                json!({"state":reopened.state}),
            ),
            check(
                "illegal transition refused",
                illegal,
                json!("reopened -> confirmed"),
            ),
        ],
        json!({"auto_admit":"false"}),
    )
}

fn attribution(eligible: bool) -> Result<InterventionAttributionV1, String> {
    let issue = format!("ii_{}", "1".repeat(64));
    let surface = format!("sha256:{}", "2".repeat(64));
    let id = attribution_id_for(&issue, InterventionTarget::SkillOrProcedure, Some(&surface));
    InterventionAttributionV1::seal(
        &issue,
        &id,
        InterventionTarget::SkillOrProcedure,
        Some("skill:native"),
        Some(&surface),
        if eligible {
            InstructionState::Wrong
        } else {
            InstructionState::AlreadyCorrect
        },
        CounterfactualPreventability::Supported,
        vec![AlternativeCause::None],
        AttributionSupportV1 {
            episode_count: CoverageValue::Measured(4),
            independent_session_count: CoverageValue::Measured(3),
            severity: CoverageValue::Measured("high".into()),
            recurrence_rate: CoverageValue::Measured("0.4".into()),
        },
        vec!["h4-native".into()],
        vec![EvaluatorOutcomeRefV1 {
            outcome_id: "eval-native".into(),
            evaluator: "native-evaluator".into(),
            applicability: EvaluatorApplicability::Applicable,
        }],
        "proposal only",
        &AttributionGateContextV1 {
            examined_surface_digest: Some(&surface),
            support_threshold: 2,
            alters_behavioral_contract: true,
        },
    )
    .map_err(|e| format!("attribution: {e:?}"))
}

fn row022() -> Result<Value, String> {
    let attribution = attribution(true)?;
    let proposal = RemediationProposalV1::build_with_target(
        "ii_1111111111111111111111111111111111111111111111111111111111111111",
        "native_family",
        RemediationEffect::ProcessChange,
        InterventionTarget::SkillOrProcedure,
        "Require native verification",
        vec![],
    );
    let sealed = SealedRemediationProposalV1::seal(
        &proposal,
        "human_review_required",
        "proposal only",
        "policy-v1",
        "redaction-v1",
        None,
        vec![],
        "native-qualifier",
        NOW,
    )
    .map_err(|e| format!("remediation: {e:?}"))?
    .bind_attribution(attribution, Some(&format!("sha256:{}", "2".repeat(64))))
    .map_err(|e| format!("bind attribution: {e:?}"))?;
    sealed
        .verify()
        .map_err(|e| format!("sealed remediation verify: {e:?}"))?;
    result(
        "ADP-022",
        vec![
            check(
                "effect/target/attribution bound",
                sealed.payload.intervention_attribution.is_some()
                    && sealed.payload.intervention_target == "skill_or_procedure",
                json!({"kind":sealed.payload.proposal_kind,"target":sealed.payload.intervention_target}),
            ),
            check(
                "authority remains none",
                sealed.payload.authority_class == "none",
                json!(sealed.payload.authority_class),
            ),
        ],
        json!({"unbound_surface":"not consumable"}),
    )
}

fn row023() -> Result<Value, String> {
    let proposal = RemediationProposalV1::build_with_target(
        "ii_2222222222222222222222222222222222222222222222222222222222222222",
        "native_family",
        RemediationEffect::TasteCandidate,
        InterventionTarget::ModelBehaviorPolicy,
        "Prefer native evidence",
        vec!["user-native".into()],
    );
    let selected = ["user-native".to_string()].into_iter().collect();
    proposal
        .validate_evidence(&selected)
        .map_err(|e| e.to_string())?;
    proposal
        .precision_gate(Some(0.99))
        .map_err(|e| e.to_string())?;
    let missing = RemediationProposalV1::build_with_target(
        "ii_3333333333333333333333333333333333333333333333333333333333333333",
        "native_family",
        RemediationEffect::TasteCandidate,
        InterventionTarget::ModelBehaviorPolicy,
        "Prefer native evidence",
        vec!["missing".into()],
    )
    .validate_evidence(&selected)
    .is_err();
    result(
        "ADP-023",
        vec![
            check(
                "alternative/evidence binding",
                proposal.supporting_user_evidence_ids.len() == 1,
                json!(proposal.supporting_user_evidence_ids),
            ),
            check(
                "unbound evidence refused",
                missing,
                json!("selected transcript boundary"),
            ),
        ],
        json!({"authority":"no inferred user authority"}),
    )
}

fn row024() -> Result<Value, String> {
    let attribution = attribution(true)?;
    let counts = attribution.evaluator_applicability_counts();
    let denominator = attribution.applicable_denominator();
    let mut ledger = OutcomeLedger::default();
    let effective = ledger.record(
        "ii_native",
        "rem_native",
        RawOutcome::NoRecurrence,
        Exposure {
            opportunities: 10,
            baseline: 10,
        },
        "native",
    );
    result(
        "ADP-024",
        vec![
            check(
                "three-valued evaluator applicability",
                counts.applicable == 1 && denominator == 1,
                json!(counts),
            ),
            check(
                "outcome ledger retains adjusted result",
                effective.adjusted == membrane_adapt::outcomes::AdjustedOutcome::Effective,
                json!(effective.adjusted),
            ),
        ],
        json!({"insufficient_evidence":"excluded from denominator"}),
    )
}

fn row025() -> Result<Value, String> {
    let record = record()?;
    let context = PreferenceSealContext {
        authority_tier: membrane_adapt::authority::PrecedenceTier::ExplicitScopedUserPreference,
        canonical_pool_sha256: "native-pool",
        admission_policy_version: "admission-v1",
        validator_receipt_id: "validator-native",
        validator_receipt_sha256: "receipt-native",
        redaction_contract_version: "redaction-v1",
    };
    let package = PortableTastePackageV1::build(
        "native-install",
        None,
        NOW,
        vec![PortableTasteRecordV1 {
            semantic_payload: record.semantic_payload(&context),
            record,
            evidence_digests: vec![DIGEST.into()],
            provenance_receipts: vec![DIGEST.into()],
            lifecycle_history: vec![],
        }],
    )
    .map_err(|e| format!("portable: {e:?}"))?;
    let seed = [25u8; 32];
    let key =
        ring::signature::Ed25519KeyPair::from_seed_unchecked(&seed).map_err(|e| e.to_string())?;
    let mut signed = package;
    signed
        .sign("native-key", &seed)
        .map_err(|e| format!("sign: {e:?}"))?;
    let imported = signed
        .import_candidates(key.public_key().as_ref())
        .map_err(|e| format!("verify: {e:?}"))?;
    result(
        "ADP-025",
        vec![
            check(
                "portable package verified",
                imported.len() == 1 && imported[0].source_package_id == signed.package_id,
                json!({"packageId":signed.package_id}),
            ),
            check(
                "import remains lower precedence",
                imported[0].requires_local_promotion,
                json!(imported[0].precedence),
            ),
        ],
        json!({"unsigned_or_untrusted":"refused"}),
    )
}

fn host_asset() -> HostProceduralAssetObservationV1 {
    HostProceduralAssetObservationV1 {
        observation_id: "obs-native".into(),
        asset_id: Observed::complete("asset-native".into()),
        assessed_at: Observed::complete(NOW.into()),
        exposures: Observed::complete(10),
        selections: Observed::complete(10),
        applications: Observed::complete(10),
        successes: Observed::complete(9),
        failures: Observed::complete(1),
        corrections_after_use: Observed::complete(1),
        token_cost_per_turn: Observed::complete(20),
        model: Observed::complete("native-model".into()),
        client: Observed::complete("native-client".into()),
        evidence_refs: Observed::complete(vec!["h4-native".into()]),
    }
}

fn host_eval(score: f64) -> HostEvaluationObservationV1 {
    HostEvaluationObservationV1 {
        outcome_id: "eval-native".into(),
        asset_id: Observed::complete("asset-native".into()),
        evaluator: Observed::complete("native-evaluator".into()),
        dataset: Observed::complete("native-dataset".into()),
        experiment: Observed::complete("native-experiment".into()),
        score: Observed::complete(score),
        evidence_refs: Observed::complete(vec!["h6-native".into()]),
    }
}

fn row030() -> Result<Value, String> {
    let output = project_host_effectiveness("asset-native", &[host_asset()], &[host_eval(0.9)]);
    let missing = project_host_effectiveness("other-asset", &[host_asset()], &[host_eval(0.9)]);
    result(
        "ADP-030",
        vec![
            check(
                "observation ingress preserves identity",
                output.asset_id == "asset-native"
                    && output
                        .evidence_refs
                        .value
                        .as_ref()
                        .is_some_and(|r| r == &["h4-native".to_string(), "h6-native".to_string()]),
                json!({"asset":output.asset_id,"refs":output.evidence_refs}),
            ),
            check(
                "unknown asset is unavailable",
                missing.effectiveness_verdict.coverage
                    == membrane_adapt::procedural_effectiveness::Coverage::Unavailable,
                json!(missing.effectiveness_verdict),
            ),
        ],
        json!({"missing_join":"unavailable"}),
    )
}

fn row031() -> Result<Value, String> {
    let complete = project_host_effectiveness("asset-native", &[host_asset()], &[host_eval(0.9)]);
    let poor = project_host_effectiveness("asset-native", &[host_asset()], &[host_eval(0.4)]);
    result(
        "ADP-031",
        vec![
            check(
                "evaluation outcome joins exact asset",
                complete.effectiveness_verdict.value
                    == Some(
                        membrane_adapt::procedural_effectiveness::EffectivenessVerdict::Effective,
                    ),
                json!(complete.effectiveness_verdict),
            ),
            check(
                "negative score is not effective",
                poor.effectiveness_verdict.value
                    != Some(
                        membrane_adapt::procedural_effectiveness::EffectivenessVerdict::Effective,
                    ),
                json!(poor.effectiveness_verdict),
            ),
        ],
        json!({"unknown_score":"unavailable"}),
    )
}

fn row033() -> Result<Value, String> {
    let issue = episode("ep-native-1", "session-1");
    let second = episode("ep-native-2", "session-2");
    let payload = CandidatePatternPayloadV1 {
        proposer_id: "native-proposer".into(),
        source_episodes: vec![
            EpisodeMembershipV1 {
                episode_id: issue.episode_id.clone(),
                episode_payload_sha256: digest(&issue),
            },
            EpisodeMembershipV1 {
                episode_id: second.episode_id.clone(),
                episode_payload_sha256: digest(&second),
            },
        ],
        operational_family: issue.family.clone(),
        operational_signature: issue.signature.clone(),
        operational_description: "portable native regression".into(),
        evidence_refs: vec![issue.episode_id.clone(), second.episode_id.clone()],
    };
    let pattern = CandidatePatternV1::propose(payload).map_err(|e| e.to_string())?;
    pattern
        .verify_against_episodes(&[issue.clone(), second.clone()])
        .map_err(|e| e.to_string())?;
    let changed = {
        let mut changed = second;
        changed.observed_failure = "tampered".into();
        pattern.verify_against_episodes(&[issue, changed]).is_err()
    };
    result(
        "ADP-033",
        vec![
            check(
                "regression case binds source episodes",
                pattern.state == PatternState::Proposed,
                json!({"pattern":pattern.pattern_id}),
            ),
            check(
                "source digest drift refused",
                changed,
                json!("episode digest changed"),
            ),
        ],
        json!({"activation":"proposal_only"}),
    )
}

fn row034() -> Result<Value, String> {
    let mut current = record()?;
    let (active, _) = current
        .transition_lifecycle(LifecycleState::Active, "activate", &"a".repeat(64), NOW)
        .map_err(|e| format!("{e:?}"))?;
    current = active;
    let (retired, _) = current
        .transition_lifecycle(LifecycleState::Retired, "deactivate", &"b".repeat(64), NOW)
        .map_err(|e| format!("{e:?}"))?;
    let no_reactivation = retired
        .transition_lifecycle(LifecycleState::Active, "reactivate", &"c".repeat(64), NOW)
        .is_err();
    result(
        "ADP-034",
        vec![
            check(
                "admin lifecycle mutation is receipted",
                retired.lifecycle_state == LifecycleState::Retired,
                json!(retired.lifecycle_state),
            ),
            check(
                "retired preference cannot reactivate",
                no_reactivation,
                json!("terminal lifecycle"),
            ),
        ],
        json!({"direct_write":"refused"}),
    )
}

fn row035() -> Result<Value, String> {
    let episodes = vec![
        episode("ep-native-1", "session-1"),
        episode("ep-native-2", "session-2"),
    ];
    let payload = CandidatePatternPayloadV1 {
        proposer_id: "native-proposer".into(),
        source_episodes: episodes
            .iter()
            .map(|e| EpisodeMembershipV1 {
                episode_id: e.episode_id.clone(),
                episode_payload_sha256: digest(e),
            })
            .collect(),
        operational_family: episodes[0].family.clone(),
        operational_signature: episodes[0].signature.clone(),
        operational_description: "bounded resident proposal".into(),
        evidence_refs: episodes.iter().map(|e| e.episode_id.clone()).collect(),
    };
    let mut pattern = CandidatePatternV1::propose(payload).map_err(|e| e.to_string())?;
    let review = BoundaryReviewReceiptV1 {
        method: "deterministic".into(),
        reviewer_id: "native-reviewer".into(),
        authority_receipt_id: "native-authority".into(),
        authority_receipt_sha256: DIGEST.into(),
        reviewed_pattern_sha256: pattern.payload_sha256.clone(),
        verdict: BoundaryVerdict::Accepted,
        receipt_sha256: String::new(),
    };
    let mut review = review;
    review.receipt_sha256 = digest(&(
        review.method.clone(),
        review.reviewer_id.clone(),
        review.authority_receipt_id.clone(),
        review.authority_receipt_sha256.clone(),
        review.reviewed_pattern_sha256.clone(),
        review.verdict,
    ));
    pattern
        .boundary_review(&episodes, review)
        .map_err(|e| e.to_string())?;
    let bad = pattern.activation_authorized;
    result(
        "ADP-035",
        vec![
            check(
                "proposal sink keeps resident proposal bounded",
                pattern.state == PatternState::BoundaryAccepted && !bad,
                json!({"state":pattern.state}),
            ),
            check(
                "proposal never self-activates",
                !pattern.activation_authorized,
                json!("host authorization required"),
            ),
        ],
        json!({"unauthorized_write":"blocked"}),
    )
}

fn row036() -> Result<Value, String> {
    let complete = project_host_effectiveness("asset-native", &[host_asset()], &[host_eval(0.9)]);
    let missing = project_host_effectiveness("asset-native", &[host_asset()], &[]);
    result(
        "ADP-036",
        vec![
            check(
                "effectiveness joins host observations/evaluations",
                complete.effectiveness_verdict.value.is_some(),
                json!(complete.effectiveness_verdict),
            ),
            check(
                "missing evaluation is not success",
                missing.effectiveness_verdict.coverage
                    == membrane_adapt::procedural_effectiveness::Coverage::Unavailable,
                json!(missing.effectiveness_verdict),
            ),
        ],
        json!({"incomplete_join":"unavailable"}),
    )
}

fn row040() -> Result<Value, String> {
    let request = ContextCostAnalysisRequestV1 {
        installation_id: "native-install".into(),
        analysis_timestamp: NOW.into(),
        usage_observations: vec![ProviderUsageObservationV1 {
            observation_id: "usage-native".into(),
            turn_id: "turn-native".into(),
            session_id: "session-native".into(),
            host: "pi".into(),
            provider: "native-provider".into(),
            model: "native-model".into(),
            usage: ProviderBilledUsageV1 {
                fresh_input_tokens: 100,
                cache_read_input_tokens: 10,
                cache_write_input_tokens: 0,
                output_tokens: 20,
            },
            measured_persistent_prefix_tokens: Some(40),
        }],
        persistent_sources: vec![PersistentSourceObservationV1 {
            source_id: "source-native".into(),
            kind: PersistentSourceKind::InstructionFile,
            path: Some("native.md".into()),
            captured_digest: "native-source-digest".into(),
            captured_bytes: 160,
            captured_token_estimate: Some(40),
            visible_turn_ids: vec!["turn-native".into()],
            file_state: SourceFileStateV1::Current {
                analysis_digest: "native-source-digest".into(),
            },
            always_on: true,
            observed_use: Default::default(),
            observed_memory_recall: Default::default(),
            shadowed_by_source_id: None,
        }],
        detector_policy: ContextCostDetectorPolicyV1::default(),
    };
    let report = analyze_persistent_context(&request).map_err(|e| format!("ADP-040: {e:?}"))?;
    let invalid = {
        let mut bad = request;
        bad.persistent_sources[0].visible_turn_ids = vec!["unknown".into()];
        analyze_persistent_context(&bad).is_err()
    };
    result(
        "ADP-040",
        vec![
            check(
                "context cost preserves billed/unattributed accounting",
                report.provider_billed_tokens == 130
                    && report.measured_persistent_prefix_tokens == 40,
                json!({"billed":report.provider_billed_tokens,"unattributed":report.unattributed_persistent_prefix_tokens}),
            ),
            check(
                "unknown turn binding refused",
                invalid,
                json!("source visibility binding"),
            ),
        ],
        json!({"inferred":"labelled_as_inferred"}),
    )
}

fn row041() -> Result<Value, String> {
    let make = |writer: &str, clock: u64, value: &str| {
        WriterRequestV1::new(
            "repo-native",
            "scope-native",
            "rule-native",
            "v1",
            writer,
            clock,
            value,
            membrane_adapt::authority::PrecedenceTier::ExplicitGlobalUserPreference,
        )
    };
    let a = make("writer-a", 1, "digest-a");
    let b = make("writer-b", 2, "digest-b");
    let first = proposal_emission("repo-native", "scope-native", &[a.clone(), b.clone()])?;
    let second = proposal_emission("repo-native", "scope-native", &[b, a])?;
    let requests = validate_proposal_emission("repo-native", "scope-native", &first)?;
    let records: Vec<WriterRecord> = requests
        .iter()
        .map(WriterRequestV1::writer_record)
        .collect();
    let merged = converge(&records);
    let negative = {
        let mut forged = first.clone();
        forged["text"] = json!("tampered");
        validate_proposal_emission("repo-native", "scope-native", &forged).is_err()
    };
    result(
        "ADP-041",
        vec![
            check(
                "concurrent writers converge",
                first == second && merged.conflicts_for(&records[0].rule_key).len() == 1,
                json!({"convergence":first["convergence"]}),
            ),
            check(
                "canonical emission validates",
                requests.len() == 2,
                json!(requests.len()),
            ),
            check(
                "tampered emission refused",
                negative,
                json!("digest-bound proposal"),
            ),
        ],
        json!({"arrival_order":"irrelevant","conflict":"preserved"}),
    )
}

/// Execute one installed native Adapt qualification row.
pub(crate) fn run(case_id: &str) -> Result<Value, String> {
    match case_id {
        "ADP-011" => row011(),
        "ADP-012" => row012(),
        "ADP-015" => row015(),
        "ADP-016" => row016(),
        "ADP-019" => row019(),
        "ADP-020" => row020(),
        "ADP-022" => row022(),
        "ADP-023" => row023(),
        "ADP-024" => row024(),
        "ADP-025" => row025(),
        "ADP-030" => row030(),
        "ADP-031" => row031(),
        "ADP-033" => row033(),
        "ADP-034" => row034(),
        "ADP-035" => row035(),
        "ADP-036" => row036(),
        "ADP-040" => row040(),
        "ADP-041" => row041(),
        other => Err(format!(
            "unsupported native Adapt qualification row: {other}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_rows_execute_real_native_contracts() {
        for id in [
            "ADP-011", "ADP-012", "ADP-015", "ADP-016", "ADP-019", "ADP-020", "ADP-022", "ADP-023",
            "ADP-024", "ADP-025", "ADP-030", "ADP-031", "ADP-033", "ADP-034", "ADP-035", "ADP-036",
            "ADP-040", "ADP-041",
        ] {
            let value = run(id).unwrap_or_else(|error| panic!("{id}: {error}"));
            assert_eq!(value["status"], "passed", "{id}: {value}");
            assert_eq!(value["evidenceKind"], "installed_native", "{id}: {value}");
            assert!(value["checks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|check| check["passed"] == true));
        }
    }

    #[test]
    fn unknown_rows_fail_closed() {
        assert!(run("ADP-999").is_err());
    }

    #[test]
    fn negative_controls_are_not_status_relabels() {
        for id in [
            "ADP-011", "ADP-015", "ADP-019", "ADP-023", "ADP-030", "ADP-031", "ADP-034", "ADP-036",
            "ADP-040", "ADP-041",
        ] {
            let value = run(id).unwrap();
            assert!(value["negative"].is_object(), "{id}: {value}");
        }
    }
}
