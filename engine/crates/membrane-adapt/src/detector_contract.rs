//! Versioned contract for the deterministic Insights detector family.
//!
//! Detector code existed before this catalog, but its semantic identity was
//! implicit in function names. This module freezes the production family IDs,
//! detector version, input contract and hard-negative boundary that accompany
//! emitted episodes. Changing semantics requires a version bump rather than a
//! silent reinterpretation of historical evidence.

use serde::{Deserialize, Serialize};

use crate::insights::{FailureEpisodeV1, TranscriptEventV1};

pub const INSIGHTS_DETECTOR_CATALOG_CONTRACT: &str = "adapt.insights-detector-catalog.v1";
pub const INSIGHTS_DETECTOR_VERSION: u32 = 1;
pub const INSIGHTS_INPUT_CONTRACT: &str = "adapt.transcript-event.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InsightsDetectorContractV1 {
    pub contract: String,
    pub family_id: String,
    pub detector_version: u32,
    pub input_contract: String,
    pub evidence_policy: String,
    pub hard_negative_policy: String,
}

fn contract(family_id: &str, hard_negative_policy: &str) -> InsightsDetectorContractV1 {
    InsightsDetectorContractV1 {
        contract: INSIGHTS_DETECTOR_CATALOG_CONTRACT.into(),
        family_id: family_id.into(),
        detector_version: INSIGHTS_DETECTOR_VERSION,
        input_contract: INSIGHTS_INPUT_CONTRACT.into(),
        evidence_policy: "only evidence_eligible transcript events; private/redacted/synthetic/meta material is ineligible".into(),
        hard_negative_policy: hard_negative_policy.into(),
    }
}

/// Frozen detector family register for the production native mining path.
pub fn insights_detector_catalog() -> Vec<InsightsDetectorContractV1> {
    vec![
        contract("visible_frustration", "external-user frustration language; quoted or non-user text is negative"),
        contract("user_swearing", "external-user profanity signal only; assistant/tool profanity is negative"),
        contract("repeated_ask", "normalized same-theme user requests across independent evidence; one occurrence is negative"),
        contract("verification_claim_without_tool_evidence", "assistant verification claim requires observable supporting verification/tool evidence; quoted/hypothetical claims are negative"),
        contract("claimed_verified_then_corrected", "assistant success/verification claim followed by eligible user correction; quoted/hypothetical claims are negative"),
        contract("ignored_tool_failure", "observable tool failure followed by behavior inconsistent with that failure; relayed or unavailable tool state is negative"),
        contract("degraded_provider_treated_as_success", "explicit degraded/unavailable provider evidence plus unsupported success treatment; typed unavailability alone is not failure"),
        contract("false_not_found_after_search", "not-found claim requires preceding observable search evidence and contradicting later evidence; genuine not-found remains negative"),
        contract("unproductive_broad_searching", "observable broad-search behavior; ordinary bounded search is negative"),
        contract("wrong_repo_or_subsystem", "explicit wrong-target evidence; mere multi-repository context is negative"),
        contract("stale_terminology_surfacing", "retired canonical terminology surfaced as current guidance; historical quotation is negative"),
        contract("silent_scope_narrowing", "assistant unilateral narrowing language without user authorization; user-requested narrowing is negative"),
        contract("omitted_requirement", "eligible user evidence that an explicit requirement was missed; unrelated follow-up is negative"),
        contract("unaccepted_plan_change", "assistant plan pivot without user acceptance; accepted/reviewed plan changes are negative"),
        contract("tests_that_cannot_fail", "mechanically tautological/disabled verification evidence; normal passing tests are negative"),
        contract("guard_firings", "explicit guard/admission refusal evidence; absence of a guard event is negative"),
        contract("postmortem_ask", "explicit eligible user postmortem/root-cause request; generic explanation requests are negative"),
        contract("verification_theatre", "verification-looking behavior without outcome-capable evidence; actual tool verification is negative"),
        contract("overengineering", "explicit over-engineering evidence or bounded assistant pattern with correction; necessary complexity is negative"),
        contract("unnecessary_abstraction", "explicit unnecessary abstraction evidence; domain-required abstraction is negative"),
        contract("unnecessary_dependency", "explicit unnecessary dependency evidence; requested/required dependency is negative"),
        contract("architecture_churn", "repeated redesign/rewrite evidence without stable progress; one reviewed redesign is negative"),
        contract("repeated_redesign", "explicit repeated redesign evidence; single redesign is negative"),
        contract("planning_instead_of_executing", "eligible evidence of repeated planning without execution; normal planning is negative"),
        contract("scope_expansion_without_request", "assistant-added work outside requested scope; user-authorized expansion is negative"),
        contract("repeated_scope_expansion", "recurrent scope expansion evidence; one authorized expansion is negative"),
        contract("false_completion_claim", "assistant completion claim contradicted by observable later evidence; truthful bounded completion is negative"),
        contract("instruction_noncompliance", "eligible evidence of explicit instruction violation; ambiguous preference is negative"),
        contract("model_or_client_specific_gotcha", "explicit model/client-specific failure evidence; generic model mention is negative"),
        contract("cross_agent_repeats", "same failure signature across independent agent/session evidence; same-agent repetition alone is negative"),
        contract("repeated_user_correction_same_theme", "repeated eligible corrections with matching normalized theme; unrelated corrections are negative"),
        contract("forge_opened_never_closed", "observable rubric/work item opened without a matching close in the bounded session; matched close is negative"),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DetectorContractError {
    #[error("detector emitted unregistered family: {0}")]
    UnregisteredFamily(String),
    #[error("detector function {expected} emitted mismatched family {observed}")]
    FamilyMismatch { expected: String, observed: String },
}

fn append_checked(
    out: &mut Vec<FailureEpisodeV1>,
    expected_family: &str,
    episodes: Vec<FailureEpisodeV1>,
) -> Result<(), DetectorContractError> {
    for episode in episodes {
        if episode.family != expected_family {
            return Err(DetectorContractError::FamilyMismatch {
                expected: expected_family.into(),
                observed: episode.family,
            });
        }
        out.push(episode);
    }
    Ok(())
}

/// Execute the frozen V1 detector catalog. This is the production mining
/// orchestrator used by the CLI API. Every emitted episode must belong to the
/// catalog; semantic changes require a new detector version/catalog contract.
pub fn run_versioned_detectors(
    events: &[TranscriptEventV1],
) -> Result<Vec<FailureEpisodeV1>, DetectorContractError> {
    use crate::insights::detectors::*;

    let mut out = Vec::new();
    append_checked(&mut out, "visible_frustration", detect_visible_frustration(events))?;
    append_checked(&mut out, "user_swearing", detect_user_swearing(events))?;
    append_checked(&mut out, "repeated_ask", detect_repeated_ask(events))?;
    append_checked(
        &mut out,
        "verification_claim_without_tool_evidence",
        detect_verification_claim_without_tool_evidence(events),
    )?;
    append_checked(
        &mut out,
        "claimed_verified_then_corrected",
        detect_claimed_verified_then_corrected(events),
    )?;
    append_checked(&mut out, "ignored_tool_failure", detect_ignored_tool_failure(events))?;
    append_checked(
        &mut out,
        "degraded_provider_treated_as_success",
        detect_degraded_provider_treated_as_success(events),
    )?;
    append_checked(
        &mut out,
        "false_not_found_after_search",
        detect_false_not_found_after_search(events),
    )?;
    append_checked(
        &mut out,
        "unproductive_broad_searching",
        detect_unproductive_broad_searching(events),
    )?;
    append_checked(
        &mut out,
        "wrong_repo_or_subsystem",
        detect_wrong_repo_or_subsystem(events),
    )?;
    append_checked(
        &mut out,
        "stale_terminology_surfacing",
        detect_stale_terminology_surfacing(events),
    )?;
    append_checked(
        &mut out,
        "silent_scope_narrowing",
        detect_silent_scope_narrowing(events),
    )?;
    append_checked(&mut out, "omitted_requirement", detect_omitted_requirement(events))?;
    append_checked(
        &mut out,
        "unaccepted_plan_change",
        detect_unaccepted_plan_change(events),
    )?;
    append_checked(
        &mut out,
        "tests_that_cannot_fail",
        detect_tests_that_cannot_fail(events),
    )?;
    append_checked(&mut out, "guard_firings", detect_guard_firings(events))?;
    append_checked(&mut out, "postmortem_ask", detect_postmortem_ask(events))?;
    append_checked(
        &mut out,
        "verification_theatre",
        detect_verification_theatre(events),
    )?;
    append_checked(
        &mut out,
        "overengineering",
        detect_overengineering_family(events),
    )?;
    append_checked(
        &mut out,
        "unnecessary_abstraction",
        detect_unnecessary_abstraction(events),
    )?;
    append_checked(
        &mut out,
        "unnecessary_dependency",
        detect_unnecessary_dependency(events),
    )?;
    append_checked(&mut out, "architecture_churn", detect_architecture_churn(events))?;
    append_checked(&mut out, "repeated_redesign", detect_repeated_redesign(events))?;
    append_checked(
        &mut out,
        "planning_instead_of_executing",
        detect_planning_instead_of_executing(events),
    )?;
    append_checked(
        &mut out,
        "scope_expansion_without_request",
        detect_scope_expansion_without_request(events),
    )?;
    append_checked(
        &mut out,
        "repeated_scope_expansion",
        detect_repeated_scope_expansion(events),
    )?;
    append_checked(
        &mut out,
        "false_completion_claim",
        detect_false_completion_claim(events),
    )?;
    append_checked(
        &mut out,
        "instruction_noncompliance",
        detect_instruction_noncompliance(events),
    )?;
    append_checked(
        &mut out,
        "model_or_client_specific_gotcha",
        detect_model_or_client_specific_gotcha(events),
    )?;
    append_checked(&mut out, "cross_agent_repeats", detect_cross_agent_repeats(events))?;
    append_checked(
        &mut out,
        "repeated_user_correction_same_theme",
        detect_repeated_user_correction_same_theme(events),
    )?;
    append_checked(
        &mut out,
        "forge_opened_never_closed",
        detect_forge_opened_never_closed(events),
    )?;

    let registered: std::collections::BTreeSet<_> = insights_detector_catalog()
        .into_iter()
        .map(|contract| contract.family_id)
        .collect();
    if let Some(unknown) = out.iter().find(|episode| !registered.contains(&episode.family)) {
        return Err(DetectorContractError::UnregisteredFamily(unknown.family.clone()));
    }
    out.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.episode_id.cmp(&b.episode_id))
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insights::EventKind;

    fn user(id: &str, session: &str, text: &str) -> TranscriptEventV1 {
        TranscriptEventV1 {
            event_id: id.into(),
            session_id: session.into(),
            host: "test".into(),
            provenance: "external_user".into(),
            kind: EventKind::UserMessage,
            text: text.into(),
            timestamp: None,
            byte_start: 0,
            byte_end: text.len() as i64,
            call_id: None,
            occurrence: 0,
            evidence_eligible: true,
        }
    }

    #[test]
    fn catalog_is_unique_and_versioned() {
        let catalog = insights_detector_catalog();
        assert_eq!(catalog.len(), 32);
        let ids: std::collections::BTreeSet<_> =
            catalog.iter().map(|row| row.family_id.as_str()).collect();
        assert_eq!(ids.len(), catalog.len());
        assert!(catalog
            .iter()
            .all(|row| row.detector_version == INSIGHTS_DETECTOR_VERSION));
    }

    #[test]
    fn production_orchestrator_binds_emitted_family_to_catalog() {
        let events = vec![
            user("a", "s1", "Please run the full test suite before claiming done"),
            user("b", "s2", "Please run the full test suite before claiming done"),
        ];
        let episodes = run_versioned_detectors(&events).expect("catalog-bound detector run");
        assert!(episodes.iter().any(|episode| episode.family == "repeated_ask"));
    }
}
