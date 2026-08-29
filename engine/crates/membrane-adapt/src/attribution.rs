//! Intervention attribution — the typed gate between an insight issue and a
//! mutation of a mutable instruction surface.
//!
//! `intervention_target` names the surface a proposal would change; an
//! attribution record answers why changing that surface would have prevented
//! the observed failures (pending doc §2.5, Adapt canon §6.9). Attribution is
//! proposal-class: a model may draft it, deterministic code binds it to
//! episode evidence, the current surface digest, and the eligibility gates.
//! It grants no authority and bypasses no existing proposal, review,
//! precision, or admission gate.

use serde::{Deserialize, Serialize};

use crate::remediation::{InterventionTarget, RemediationSealError};

/// Versioned policy identity for attribution derivation. Changing gate
/// semantics changes this version; it never silently reinterprets old
/// records.
pub const ATTRIBUTION_POLICY_VERSION: &str = "adapt-attribution-v1";

/// Default independent-session threshold for the support gate: recurrence
/// across independent sessions, not repetition inside one trajectory.
pub const DEFAULT_INDEPENDENT_SESSION_THRESHOLD: u32 = 2;

/// Whether the examined surface already carries the correct instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionState {
    Missing,
    Wrong,
    Underspecified,
    AlreadyCorrect,
    NotApplicable,
}

/// Counterfactual preventability. A preventability claim without
/// episode-level evidence is `unknown`, never `supported`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CounterfactualPreventability {
    Supported,
    Unsupported,
    Unknown,
}

/// Plausible non-target causes the attribution considered. Any member other
/// than `none` marks a dominant alternative cause; `insufficient_evidence`
/// is itself ineligible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlternativeCause {
    None,
    ModelVariance,
    RoutingFailure,
    Infrastructure,
    Product,
    ToolImplementation,
    EvaluatorError,
    InsufficientEvidence,
}

/// Three-valued evaluator applicability (pending doc §2.7). An
/// `insufficient_evidence` outcome is removed from the applicable
/// denominator; it never becomes a success, a failure, or a zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluatorApplicability {
    Applicable,
    NotApplicable,
    InsufficientEvidence,
}

/// Per-field coverage: a measured value or typed `unavailable`. Absent
/// evidence is never represented as zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CoverageValue<T> {
    Measured(T),
    Unavailable,
}

/// Attribution support with per-field coverage markers (pending doc §2.5).
/// `severity` and `recurrence_rate` are opaque caller-owned representations;
/// the deterministic gates consume only their coverage and the typed
/// independent-session count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionSupportV1 {
    pub episode_count: CoverageValue<u32>,
    pub independent_session_count: CoverageValue<u32>,
    pub severity: CoverageValue<String>,
    pub recurrence_rate: CoverageValue<String>,
}

impl AttributionSupportV1 {
    pub fn unavailable() -> Self {
        Self {
            episode_count: CoverageValue::Unavailable,
            independent_session_count: CoverageValue::Unavailable,
            severity: CoverageValue::Unavailable,
            recurrence_rate: CoverageValue::Unavailable,
        }
    }
}

/// One joined evaluator outcome with its applicability for this attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluatorOutcomeRefV1 {
    pub outcome_id: String,
    pub evaluator: String,
    pub applicability: EvaluatorApplicability,
}

/// Typed ineligibility reasons, one per failed gate; also the error type of
/// live eligibility checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationIneligibility {
    /// The bound surface digest differs from the digest the caller examined;
    /// rebase and re-derive (pending doc §2.5, constraint 3).
    StaleSurfaceDigest,
    /// An instruction-surface attribution without a bound surface digest.
    SurfaceDigestUnavailable,
    PreventabilityUnknown,
    PreventabilityUnsupported,
    AlreadyCorrectSurface,
    InstructionStateNotActionable,
    /// A dominant alternative cause is present; the first one in list order
    /// is named.
    DominantAlternativeCause(AlternativeCause),
    InsufficientEvidenceAlternativeCause,
    SupportUnavailable,
    SupportBelowThreshold {
        measured: u32,
        required: u32,
    },
    /// The proposed change restates or hedges guidance the surface already
    /// carries instead of altering its behavioral contract.
    RedundantRestatement,
}

impl std::fmt::Display for MutationIneligibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleSurfaceDigest => {
                write!(f, "surface digest changed since the attribution was bound")
            }
            Self::SurfaceDigestUnavailable => write!(
                f,
                "instruction-surface attribution has no bound surface digest"
            ),
            Self::PreventabilityUnknown => write!(f, "counterfactual preventability is unknown"),
            Self::PreventabilityUnsupported => {
                write!(f, "counterfactual preventability is unsupported")
            }
            Self::AlreadyCorrectSurface => {
                write!(f, "surface already demanded the correct behavior")
            }
            Self::InstructionStateNotActionable => {
                write!(f, "instruction state is not actionable on this surface")
            }
            Self::DominantAlternativeCause(cause) => {
                write!(f, "dominant alternative cause: {cause:?}")
            }
            Self::InsufficientEvidenceAlternativeCause => {
                write!(f, "alternative causes carry insufficient evidence")
            }
            Self::SupportUnavailable => write!(f, "independent-session support is unavailable"),
            Self::SupportBelowThreshold { measured, required } => {
                write!(
                    f,
                    "independent sessions {measured} below required {required}"
                )
            }
            Self::RedundantRestatement => write!(
                f,
                "proposed change does not alter the surface's behavioral contract"
            ),
        }
    }
}

impl std::error::Error for MutationIneligibility {}

/// Caller-supplied gate inputs attribution cannot observe itself: the digest
/// of the surface version actually examined, the family's independent-session
/// threshold, and the reviewer finding on behavioral-contract alteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributionGateContextV1<'a> {
    pub examined_surface_digest: Option<&'a str>,
    pub support_threshold: u32,
    pub alters_behavioral_contract: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributionSealError {
    InvalidIssueId,
    InvalidAttributionId,
    InvalidSurfaceDigest,
    InvalidEvidenceRef,
    EmptyRecord,
    /// `mutation_eligible` and `ineligibility_reason` must be exact
    /// negations of each other.
    EligibilityMismatch,
    PayloadDigestMismatch,
}

/// Consumption-gate failure for host variant generation over a sealed
/// proposal (see [`crate::remediation::consumable_for_variant_generation`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributionGateError {
    /// A mutable instruction surface was proposed with no referenced
    /// attribution, or with one bound to a different target.
    AttributionRequired {
        target: InterventionTarget,
    },
    InvalidAttribution(AttributionSealError),
    InvalidProposal(RemediationSealError),
    NotMutationEligible(MutationIneligibility),
}

impl std::fmt::Display for AttributionGateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AttributionRequired { target } => write!(
                f,
                "mutable instruction surface {target:?} requires a mutation-eligible attribution"
            ),
            Self::InvalidAttribution(error) => write!(f, "invalid attribution: {error:?}"),
            Self::InvalidProposal(error) => write!(f, "invalid sealed proposal: {error:?}"),
            Self::NotMutationEligible(reason) => write!(f, "mutation ineligible: {reason}"),
        }
    }
}

impl std::error::Error for AttributionGateError {}

/// Sealed intervention attribution (pending doc §2.5). The `integrity`
/// digest covers every preceding field, so the sealed basis is exactly the
/// §2.5 field set; field order matches the pending specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterventionAttributionV1 {
    pub attribution_id: String,
    pub source_issue_id: String,
    pub candidate_target: InterventionTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owning_surface_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_surface_digest: Option<String>,
    pub instruction_state: InstructionState,
    pub counterfactual_preventability: CounterfactualPreventability,
    /// Dominant alternative causes present in the evidence. The
    /// deterministic gate rejects typed dominant causes and
    /// `insufficient_evidence`; naming every considered cause is a review
    /// duty enforced with the behavioral-contract finding, not an
    /// empty-list check.
    pub alternative_causes: Vec<AlternativeCause>,
    pub support: AttributionSupportV1,
    /// References to host-emitted H4 asset-activation observations (§2.6).
    /// Opaque identifiers: the host never emits `rule_relevant` or
    /// `rule_followed`, so the refs carry no semantic assertions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activation_evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evaluator_outcome_refs: Vec<EvaluatorOutcomeRefV1>,
    pub mutation_eligible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ineligibility_reason: Option<MutationIneligibility>,
    pub honesty_limit: String,
    pub attribution_policy_version: String,
    pub integrity: String,
}

fn is_lower_hex(byte: u8) -> bool {
    matches!(byte, b'0'..=b'9' | b'a'..=b'f')
}

fn valid_prefixed_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| digest.len() == 64 && digest.bytes().all(is_lower_hex))
}

fn valid_issue_id(value: &str) -> bool {
    value
        .strip_prefix("ii_")
        .is_some_and(|suffix| suffix.len() == 64 && suffix.bytes().all(is_lower_hex))
}

fn valid_attribution_id(value: &str) -> bool {
    value
        .strip_prefix("att_")
        .is_some_and(|suffix| suffix.len() == 64 && suffix.bytes().all(is_lower_hex))
}

fn integrity_basis(value: &InterventionAttributionV1) -> serde_json::Value {
    let mut object = serde_json::to_value(value).expect("attribution serializes");
    object
        .as_object_mut()
        .expect("attribution serializes to an object")
        .remove("integrity");
    object
}

/// Deterministic attribution identity: bound to the source issue, the target
/// surface, and the surface version examined. Unbound attributions digest a
/// literal `none` so they remain reproducible.
pub fn attribution_id_for(
    source_issue_id: &str,
    candidate_target: InterventionTarget,
    current_surface_digest: Option<&str>,
) -> String {
    let id_src = format!(
        "{source_issue_id}\u{0}{}\u{0}{}",
        candidate_target.as_str(),
        current_surface_digest.unwrap_or("none")
    );
    format!("att_{}", crate::canonical::sha256_hex(id_src.as_bytes()))
}

impl InterventionAttributionV1 {
    /// Seal an attribution. `mutation_eligible` is derived here from the
    /// five §2.5 gates and paired with a typed `ineligibility_reason`;
    /// ineligible attributions are valid records, they are just never
    /// consumable.
    #[allow(clippy::too_many_arguments)]
    pub fn seal(
        source_issue_id: &str,
        attribution_id: &str,
        candidate_target: InterventionTarget,
        owning_surface_ref: Option<&str>,
        current_surface_digest: Option<&str>,
        instruction_state: InstructionState,
        counterfactual_preventability: CounterfactualPreventability,
        alternative_causes: Vec<AlternativeCause>,
        support: AttributionSupportV1,
        activation_evidence_refs: Vec<String>,
        evaluator_outcome_refs: Vec<EvaluatorOutcomeRefV1>,
        honesty_limit: &str,
        gate: &AttributionGateContextV1<'_>,
    ) -> Result<Self, AttributionSealError> {
        if !valid_issue_id(source_issue_id) {
            return Err(AttributionSealError::InvalidIssueId);
        }
        if !valid_attribution_id(attribution_id) {
            return Err(AttributionSealError::InvalidAttributionId);
        }
        if let Some(digest) = current_surface_digest {
            if !valid_prefixed_digest(digest) {
                return Err(AttributionSealError::InvalidSurfaceDigest);
            }
        }
        if owning_surface_ref.is_some_and(|reference| reference.trim().is_empty())
            || activation_evidence_refs
                .iter()
                .any(|reference| reference.trim().is_empty())
            || evaluator_outcome_refs.iter().any(|outcome| {
                outcome.outcome_id.trim().is_empty() || outcome.evaluator.trim().is_empty()
            })
        {
            return Err(AttributionSealError::InvalidEvidenceRef);
        }
        if honesty_limit.trim().is_empty() {
            return Err(AttributionSealError::EmptyRecord);
        }
        let mut attribution = Self {
            attribution_id: attribution_id.to_string(),
            source_issue_id: source_issue_id.to_string(),
            candidate_target,
            owning_surface_ref: owning_surface_ref.map(str::to_owned),
            current_surface_digest: current_surface_digest.map(str::to_owned),
            instruction_state,
            counterfactual_preventability,
            alternative_causes,
            support,
            activation_evidence_refs,
            evaluator_outcome_refs,
            mutation_eligible: false,
            ineligibility_reason: None,
            honesty_limit: honesty_limit.to_string(),
            attribution_policy_version: ATTRIBUTION_POLICY_VERSION.into(),
            integrity: String::new(),
        };
        match attribution.check_mutation_eligibility(gate) {
            Ok(()) => attribution.mutation_eligible = true,
            Err(reason) => attribution.ineligibility_reason = Some(reason),
        }
        attribution.integrity = crate::canonical::sha256_canonical(&integrity_basis(&attribution));
        Ok(attribution)
    }

    /// Re-run the five eligibility gates against caller-supplied gate inputs.
    /// Staleness is an adoption precondition checked before the gates: a
    /// digest bound here that differs from the examined digest fails first.
    pub fn check_mutation_eligibility(
        &self,
        gate: &AttributionGateContextV1<'_>,
    ) -> Result<(), MutationIneligibility> {
        if self.candidate_target.is_mutable_instruction_surface()
            && self.current_surface_digest.is_none()
        {
            return Err(MutationIneligibility::SurfaceDigestUnavailable);
        }
        if let (Some(bound), Some(examined)) =
            (&self.current_surface_digest, gate.examined_surface_digest)
        {
            if bound.as_str() != examined {
                return Err(MutationIneligibility::StaleSurfaceDigest);
            }
        }
        match self.counterfactual_preventability {
            CounterfactualPreventability::Supported => {}
            CounterfactualPreventability::Unknown => {
                return Err(MutationIneligibility::PreventabilityUnknown);
            }
            CounterfactualPreventability::Unsupported => {
                return Err(MutationIneligibility::PreventabilityUnsupported);
            }
        }
        if self.candidate_target.is_mutable_instruction_surface() {
            match self.instruction_state {
                InstructionState::Missing
                | InstructionState::Wrong
                | InstructionState::Underspecified => {}
                InstructionState::AlreadyCorrect => {
                    return Err(MutationIneligibility::AlreadyCorrectSurface);
                }
                InstructionState::NotApplicable => {
                    return Err(MutationIneligibility::InstructionStateNotActionable);
                }
            }
        }
        for cause in &self.alternative_causes {
            match cause {
                AlternativeCause::None => {}
                AlternativeCause::InsufficientEvidence => {
                    return Err(MutationIneligibility::InsufficientEvidenceAlternativeCause);
                }
                other => {
                    return Err(MutationIneligibility::DominantAlternativeCause(*other));
                }
            }
        }
        match self.support.independent_session_count {
            CoverageValue::Measured(count) if count >= gate.support_threshold => {}
            CoverageValue::Measured(count) => {
                return Err(MutationIneligibility::SupportBelowThreshold {
                    measured: count,
                    required: gate.support_threshold,
                });
            }
            CoverageValue::Unavailable => return Err(MutationIneligibility::SupportUnavailable),
        }
        if !gate.alters_behavioral_contract {
            return Err(MutationIneligibility::RedundantRestatement);
        }
        Ok(())
    }

    /// Staleness check at adoption: the attribution is stale when the caller
    /// examined a different surface version than the one bound here.
    pub fn is_stale(&self, examined_surface_digest: Option<&str>) -> bool {
        match (&self.current_surface_digest, examined_surface_digest) {
            (Some(bound), Some(examined)) => bound.as_str() != examined,
            _ => false,
        }
    }

    /// Adoption check for consumers: fresh digest plus the sealed derivation
    /// snapshot. Callers holding full gate inputs re-run
    /// [`Self::check_mutation_eligibility`] instead.
    pub fn confirm_adoption(
        &self,
        examined_surface_digest: Option<&str>,
    ) -> Result<(), MutationIneligibility> {
        if self.is_stale(examined_surface_digest) {
            return Err(MutationIneligibility::StaleSurfaceDigest);
        }
        if let Some(reason) = &self.ineligibility_reason {
            return Err(reason.clone());
        }
        if !self.mutation_eligible {
            // Seal pairs an ineligible flag with a typed reason; this arm is
            // a typed guard for records that never passed seal-time
            // derivation, not a default.
            return Err(MutationIneligibility::PreventabilityUnknown);
        }
        Ok(())
    }

    /// Applicable evaluator outcomes. `not_applicable` and
    /// `insufficient_evidence` observations are removed from the applicable
    /// denominator (pending doc §2.7); they never count as success, failure,
    /// or zero.
    pub fn applicable_evaluator_outcomes(&self) -> impl Iterator<Item = &EvaluatorOutcomeRefV1> {
        self.evaluator_outcome_refs
            .iter()
            .filter(|outcome| outcome.applicability == EvaluatorApplicability::Applicable)
    }

    /// Verify seal integrity: structural validity, the eligibility/reason
    /// pairing invariant, and the integrity digest.
    pub fn verify(&self) -> Result<(), AttributionSealError> {
        if !valid_attribution_id(&self.attribution_id) {
            return Err(AttributionSealError::InvalidAttributionId);
        }
        if !valid_issue_id(&self.source_issue_id) {
            return Err(AttributionSealError::InvalidIssueId);
        }
        if let Some(digest) = &self.current_surface_digest {
            if !valid_prefixed_digest(digest) {
                return Err(AttributionSealError::InvalidSurfaceDigest);
            }
        }
        if self
            .activation_evidence_refs
            .iter()
            .any(|reference| reference.trim().is_empty())
            || self.evaluator_outcome_refs.iter().any(|outcome| {
                outcome.outcome_id.trim().is_empty() || outcome.evaluator.trim().is_empty()
            })
        {
            return Err(AttributionSealError::InvalidEvidenceRef);
        }
        if self.honesty_limit.trim().is_empty() || self.attribution_policy_version.trim().is_empty()
        {
            return Err(AttributionSealError::EmptyRecord);
        }
        if self.mutation_eligible == self.ineligibility_reason.is_some() {
            return Err(AttributionSealError::EligibilityMismatch);
        }
        if crate::canonical::sha256_canonical(&integrity_basis(self)) != self.integrity {
            return Err(AttributionSealError::PayloadDigestMismatch);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remediation::{
        consumable_for_variant_generation, RemediationEffect, RemediationProposalV1,
        SealedRemediationProposalV1,
    };

    fn issue() -> String {
        format!("ii_{}", "1".repeat(64))
    }

    fn surface_digest(seed: char) -> String {
        format!("sha256:{}", seed.to_string().repeat(64))
    }

    fn eligible_support() -> AttributionSupportV1 {
        AttributionSupportV1 {
            episode_count: CoverageValue::Measured(4),
            independent_session_count: CoverageValue::Measured(3),
            severity: CoverageValue::Measured("high".into()),
            recurrence_rate: CoverageValue::Measured("0.4".into()),
        }
    }

    fn eligible_gate(examined: &str) -> AttributionGateContextV1<'_> {
        AttributionGateContextV1 {
            examined_surface_digest: Some(examined),
            support_threshold: DEFAULT_INDEPENDENT_SESSION_THRESHOLD,
            alters_behavioral_contract: true,
        }
    }

    fn seal_eligible_instruction_attribution() -> InterventionAttributionV1 {
        let surface = surface_digest('a');
        let id = attribution_id_for(
            &issue(),
            InterventionTarget::SkillOrProcedure,
            Some(&surface),
        );
        InterventionAttributionV1::seal(
            &issue(),
            &id,
            InterventionTarget::SkillOrProcedure,
            Some("skill:review-checklist"),
            Some(&surface),
            InstructionState::Missing,
            CounterfactualPreventability::Supported,
            vec![AlternativeCause::None],
            eligible_support(),
            vec!["h4-observation-1".into()],
            vec![EvaluatorOutcomeRefV1 {
                outcome_id: "ev-ok".into(),
                evaluator: "completion-integrity".into(),
                applicability: EvaluatorApplicability::Applicable,
            }],
            "diagnostic attribution; no authority is granted",
            &eligible_gate(&surface),
        )
        .expect("eligible attribution seals")
    }

    fn sealed_proposal(target: InterventionTarget, text: &str) -> SealedRemediationProposalV1 {
        let proposal = RemediationProposalV1::build_with_target(
            &issue(),
            "repeated_ask",
            RemediationEffect::ProcessChange,
            target,
            text,
            vec![],
        );
        SealedRemediationProposalV1::seal(
            &proposal,
            "requires_human_review",
            "proposal only",
            "policy-v1",
            "redaction-v1",
            None,
            vec![],
            "adapt",
            "2026-08-25T00:00:00Z",
        )
        .unwrap()
    }

    #[test]
    fn already_correct_is_never_mutation_eligible_for_instruction_surface() {
        let surface = surface_digest('a');
        let id = attribution_id_for(
            &issue(),
            InterventionTarget::SkillOrProcedure,
            Some(&surface),
        );
        let attribution = InterventionAttributionV1::seal(
            &issue(),
            &id,
            InterventionTarget::SkillOrProcedure,
            None,
            Some(&surface),
            InstructionState::AlreadyCorrect,
            CounterfactualPreventability::Supported,
            vec![AlternativeCause::None],
            eligible_support(),
            vec![],
            vec![],
            "diagnostic attribution",
            &eligible_gate(&surface),
        )
        .expect("ineligible attributions still seal as records");
        assert!(!attribution.mutation_eligible);
        assert_eq!(
            attribution.ineligibility_reason,
            Some(MutationIneligibility::AlreadyCorrectSurface)
        );
        assert!(attribution.verify().is_ok());
        assert_eq!(
            attribution.confirm_adoption(Some(&surface)),
            Err(MutationIneligibility::AlreadyCorrectSurface)
        );
    }

    #[test]
    fn unsupported_or_unknown_counterfactual_is_never_mutation_eligible() {
        for (preventability, expected) in [
            (
                CounterfactualPreventability::Unsupported,
                MutationIneligibility::PreventabilityUnsupported,
            ),
            (
                CounterfactualPreventability::Unknown,
                MutationIneligibility::PreventabilityUnknown,
            ),
        ] {
            let surface = surface_digest('a');
            let id = attribution_id_for(&issue(), InterventionTarget::SystemPrompt, Some(&surface));
            let attribution = InterventionAttributionV1::seal(
                &issue(),
                &id,
                InterventionTarget::SystemPrompt,
                None,
                Some(&surface),
                InstructionState::Wrong,
                preventability,
                vec![AlternativeCause::None],
                eligible_support(),
                vec![],
                vec![],
                "diagnostic attribution",
                &eligible_gate(&surface),
            )
            .expect("ineligible attributions still seal as records");
            assert!(!attribution.mutation_eligible);
            assert_eq!(attribution.ineligibility_reason, Some(expected));
        }
    }

    #[test]
    fn stale_surface_digest_fails_adoption() {
        let attribution = seal_eligible_instruction_attribution();
        let examined = surface_digest('b');
        assert!(attribution.is_stale(Some(&examined)));
        assert_eq!(
            attribution.confirm_adoption(Some(&examined)),
            Err(MutationIneligibility::StaleSurfaceDigest)
        );
        assert!(!attribution.is_stale(Some(&surface_digest('a'))));
        assert!(attribution
            .confirm_adoption(Some(&surface_digest('a')))
            .is_ok());
    }

    #[test]
    fn mutable_surface_proposal_without_attribution_is_not_consumable() {
        let sealed = sealed_proposal(
            InterventionTarget::SkillOrProcedure,
            "Require checklist verification before completion claims",
        );
        assert_eq!(
            consumable_for_variant_generation(&sealed, None, Some(&surface_digest('a'))),
            Err(AttributionGateError::AttributionRequired {
                target: InterventionTarget::SkillOrProcedure
            })
        );

        // Additive guard targets are informed by attribution, not blocked.
        let guard_sealed =
            sealed_proposal(InterventionTarget::Guard, "Add a completion-receipt guard");
        assert!(consumable_for_variant_generation(&guard_sealed, None, None).is_ok());
    }

    #[test]
    fn mutation_eligible_attribution_unlocks_stale_aware_consumption() {
        let attribution = seal_eligible_instruction_attribution();
        assert!(attribution.mutation_eligible);
        assert!(attribution.ineligibility_reason.is_none());
        assert!(attribution
            .check_mutation_eligibility(&eligible_gate(&surface_digest('a')))
            .is_ok());

        let sealed = sealed_proposal(
            InterventionTarget::SkillOrProcedure,
            "Require checklist verification before completion claims",
        );
        assert!(consumable_for_variant_generation(
            &sealed,
            Some(&attribution),
            Some(&surface_digest('a'))
        )
        .is_ok());
        // Stale adoption fails at the consumption gate.
        assert_eq!(
            consumable_for_variant_generation(
                &sealed,
                Some(&attribution),
                Some(&surface_digest('b'))
            ),
            Err(AttributionGateError::NotMutationEligible(
                MutationIneligibility::StaleSurfaceDigest
            ))
        );

        // An attribution bound to a different target does not satisfy the
        // proposal's target.
        let surface = surface_digest('a');
        let guard_id = attribution_id_for(&issue(), InterventionTarget::Guard, None);
        let guard_attribution = InterventionAttributionV1::seal(
            &issue(),
            &guard_id,
            InterventionTarget::Guard,
            None,
            None,
            InstructionState::NotApplicable,
            CounterfactualPreventability::Supported,
            vec![AlternativeCause::None],
            eligible_support(),
            vec![],
            vec![],
            "diagnostic attribution",
            &eligible_gate(&surface),
        )
        .expect("guard attribution seals");
        assert!(guard_attribution.mutation_eligible);
        assert_eq!(
            consumable_for_variant_generation(&sealed, Some(&guard_attribution), Some(&surface)),
            Err(AttributionGateError::AttributionRequired {
                target: InterventionTarget::SkillOrProcedure
            })
        );
    }

    #[test]
    fn insufficient_evidence_evaluator_outcomes_leave_the_applicable_denominator() {
        let surface = surface_digest('a');
        let id = attribution_id_for(
            &issue(),
            InterventionTarget::SkillOrProcedure,
            Some(&surface),
        );
        let attribution = InterventionAttributionV1::seal(
            &issue(),
            &id,
            InterventionTarget::SkillOrProcedure,
            None,
            Some(&surface),
            InstructionState::Underspecified,
            CounterfactualPreventability::Supported,
            vec![AlternativeCause::None],
            eligible_support(),
            vec![],
            vec![
                EvaluatorOutcomeRefV1 {
                    outcome_id: "ev-ok".into(),
                    evaluator: "completion-integrity".into(),
                    applicability: EvaluatorApplicability::Applicable,
                },
                EvaluatorOutcomeRefV1 {
                    outcome_id: "excluded-na".into(),
                    evaluator: "completion-integrity".into(),
                    applicability: EvaluatorApplicability::NotApplicable,
                },
                EvaluatorOutcomeRefV1 {
                    outcome_id: "excluded-unknown".into(),
                    evaluator: "completion-integrity".into(),
                    applicability: EvaluatorApplicability::InsufficientEvidence,
                },
            ],
            "diagnostic attribution",
            &eligible_gate(&surface),
        )
        .expect("seals");
        let applicable: Vec<_> = attribution.applicable_evaluator_outcomes().collect();
        assert_eq!(applicable.len(), 1);
        assert_eq!(applicable[0].outcome_id, "ev-ok");
    }

    #[test]
    fn dominant_and_insufficient_alternative_causes_are_ineligible() {
        let surface = surface_digest('a');
        let id = attribution_id_for(
            &issue(),
            InterventionTarget::ToolDescription,
            Some(&surface),
        );
        let dominant = InterventionAttributionV1::seal(
            &issue(),
            &id,
            InterventionTarget::ToolDescription,
            None,
            Some(&surface),
            InstructionState::Wrong,
            CounterfactualPreventability::Supported,
            vec![AlternativeCause::ModelVariance],
            eligible_support(),
            vec![],
            vec![],
            "diagnostic attribution",
            &eligible_gate(&surface),
        )
        .expect("seals");
        assert_eq!(
            dominant.ineligibility_reason,
            Some(MutationIneligibility::DominantAlternativeCause(
                AlternativeCause::ModelVariance
            ))
        );

        let insufficient = InterventionAttributionV1::seal(
            &issue(),
            &id,
            InterventionTarget::ToolDescription,
            None,
            Some(&surface),
            InstructionState::Wrong,
            CounterfactualPreventability::Supported,
            vec![
                AlternativeCause::None,
                AlternativeCause::InsufficientEvidence,
            ],
            eligible_support(),
            vec![],
            vec![],
            "diagnostic attribution",
            &eligible_gate(&surface),
        )
        .expect("seals");
        assert_eq!(
            insufficient.ineligibility_reason,
            Some(MutationIneligibility::InsufficientEvidenceAlternativeCause)
        );
    }

    #[test]
    fn support_gates_fail_typed_on_unavailable_and_below_threshold() {
        let surface = surface_digest('a');
        let id = attribution_id_for(
            &issue(),
            InterventionTarget::SkillOrProcedure,
            Some(&surface),
        );

        let mut support_unavailable = eligible_support();
        support_unavailable.independent_session_count = CoverageValue::Unavailable;
        let unavailable = InterventionAttributionV1::seal(
            &issue(),
            &id,
            InterventionTarget::SkillOrProcedure,
            None,
            Some(&surface),
            InstructionState::Wrong,
            CounterfactualPreventability::Supported,
            vec![AlternativeCause::None],
            support_unavailable,
            vec![],
            vec![],
            "diagnostic attribution",
            &eligible_gate(&surface),
        )
        .expect("seals");
        assert_eq!(
            unavailable.ineligibility_reason,
            Some(MutationIneligibility::SupportUnavailable)
        );

        let mut support_below = eligible_support();
        support_below.independent_session_count = CoverageValue::Measured(1);
        let below = InterventionAttributionV1::seal(
            &issue(),
            &id,
            InterventionTarget::SkillOrProcedure,
            None,
            Some(&surface),
            InstructionState::Wrong,
            CounterfactualPreventability::Supported,
            vec![AlternativeCause::None],
            support_below,
            vec![],
            vec![],
            "diagnostic attribution",
            &eligible_gate(&surface),
        )
        .expect("seals");
        assert_eq!(
            below.ineligibility_reason,
            Some(MutationIneligibility::SupportBelowThreshold {
                measured: 1,
                required: DEFAULT_INDEPENDENT_SESSION_THRESHOLD
            })
        );
    }

    #[test]
    fn redundant_restatement_is_ineligible() {
        let surface = surface_digest('a');
        let id = attribution_id_for(
            &issue(),
            InterventionTarget::SkillOrProcedure,
            Some(&surface),
        );
        let gate = AttributionGateContextV1 {
            examined_surface_digest: Some(&surface),
            support_threshold: DEFAULT_INDEPENDENT_SESSION_THRESHOLD,
            alters_behavioral_contract: false,
        };
        let attribution = InterventionAttributionV1::seal(
            &issue(),
            &id,
            InterventionTarget::SkillOrProcedure,
            None,
            Some(&surface),
            InstructionState::Wrong,
            CounterfactualPreventability::Supported,
            vec![AlternativeCause::None],
            eligible_support(),
            vec![],
            vec![],
            "diagnostic attribution",
            &gate,
        )
        .expect("seals");
        assert_eq!(
            attribution.ineligibility_reason,
            Some(MutationIneligibility::RedundantRestatement)
        );
    }

    #[test]
    fn seal_is_verified_and_identity_is_deterministic() {
        let attribution = seal_eligible_instruction_attribution();
        assert!(attribution.verify().is_ok());
        assert!(attribution.attribution_id.starts_with("att_"));
        assert_eq!(attribution.attribution_id.len(), 68);

        let mut tampered = attribution.clone();
        tampered.instruction_state = InstructionState::AlreadyCorrect;
        assert_eq!(
            tampered.verify(),
            Err(AttributionSealError::PayloadDigestMismatch)
        );

        assert_eq!(
            attribution_id_for(
                &issue(),
                InterventionTarget::SkillOrProcedure,
                Some(&surface_digest('a'))
            ),
            attribution_id_for(
                &issue(),
                InterventionTarget::SkillOrProcedure,
                Some(&surface_digest('a'))
            )
        );
        assert_ne!(
            attribution_id_for(
                &issue(),
                InterventionTarget::SkillOrProcedure,
                Some(&surface_digest('a'))
            ),
            attribution_id_for(&issue(), InterventionTarget::SkillOrProcedure, None)
        );
    }

    #[test]
    fn unavailable_support_serializes_as_null_never_zero() {
        let support = AttributionSupportV1::unavailable();
        let json = serde_json::to_value(&support).expect("support serializes");
        assert_eq!(json["independent_session_count"], serde_json::Value::Null);
        assert_eq!(json["episode_count"], serde_json::Value::Null);

        let measured: AttributionSupportV1 = serde_json::from_value(serde_json::json!({
            "episode_count": 3,
            "independent_session_count": null,
            "severity": null,
            "recurrence_rate": "0.5"
        }))
        .expect("coverage values round-trip");
        assert_eq!(measured.episode_count, CoverageValue::Measured(3));
        assert_eq!(
            measured.independent_session_count,
            CoverageValue::Unavailable
        );
    }
}
