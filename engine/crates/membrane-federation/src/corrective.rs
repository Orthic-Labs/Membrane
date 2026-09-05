//! Bounded corrective Pull retrieval primitives.
//!
//! Sufficiency is evaluated only when the planner supplies an explicit,
//! request-bound contract. Candidate source class/ref and lane status are the
//! only observations used here; missing planner requirements stay typed
//! unknown and never trigger a fabricated retry.
//!
//! First-party corrective retrieval (pending §13.1/§13.2): the first-party
//! caller (the native `membrane_context` tool) authors the contract and
//! transports it unchanged; the engine re-evaluates after the single
//! alternate-lane corrective stage and republishes a typed receipt. The
//! trigger provider is never retried, and the terminal second insufficiency
//! stays typed instead of repeating the request.

use crate::normalize::NormalizedProviderOutput;
use membrane_protocol::{
    FederationProviderStatusV1, FederationRequestV1, ProviderId, ProviderOutputV1,
    ProviderWarningV1, ReasonCode,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CORRECTIVE_RETRIEVAL_SCHEMA_VERSION: u32 = 1;
pub const CORRECTIVE_RETRIEVAL_POLICY: &str = "membrane-corrective-retrieval-v1";
pub const SUFFICIENCY_CONTRACT_SCHEMA_VERSION: u32 = 1;
pub const SUFFICIENCY_POLICY: &str = "membrane-sufficiency-v1";
pub const MAX_SUFFICIENCY_REQUIREMENTS: usize = 64;
pub const MAX_CORRECTIVE_STAGES: u32 = 1;

/// Corrective retrieval policy invariant (pending §13.1): the trigger
/// provider is never retried — the single corrective action targets one
/// acceptable alternate lane, or it is terminal and typed.
pub const PROVIDER_NOT_RETRIED_AFTER_TRIGGER_V1: &str = "provider_not_retried_after_trigger_v1";

fn default_max_corrective_stages() -> u32 {
    MAX_CORRECTIVE_STAGES
}

/// Planner-owned requirement that can be checked against normalized provider
/// evidence without interpreting candidate text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SufficiencyRequirementV1 {
    pub id: String,
    pub evidence_class: String,
    #[serde(default)]
    pub acceptable_providers: Vec<ProviderId>,
    #[serde(default)]
    pub acceptable_source_refs: Vec<String>,
    #[serde(default = "default_minimum_candidates")]
    pub minimum_candidates: u32,
}

fn default_minimum_candidates() -> u32 {
    1
}

/// Explicit planner contract required before federation can classify a
/// request as insufficient or plan one corrective stage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SufficiencyContractV1 {
    pub schema_version: u32,
    #[serde(default = "default_sufficiency_policy")]
    pub policy: String,
    pub requirements: Vec<SufficiencyRequirementV1>,
    #[serde(default = "default_max_corrective_stages")]
    pub max_corrective_stages: u32,
}

fn default_sufficiency_policy() -> String {
    SUFFICIENCY_POLICY.to_owned()
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SufficiencyContractError {
    #[error("sufficiency contract schema version is unsupported")]
    SchemaVersion,
    #[error("sufficiency contract policy is unsupported")]
    Policy,
    #[error("sufficiency contract must contain one to 64 requirements")]
    RequirementCount,
    #[error("corrective stage limit must be exactly one")]
    CorrectiveStageLimit,
    #[error("sufficiency requirement id is invalid")]
    RequirementId,
    #[error("sufficiency evidence class is invalid")]
    EvidenceClass,
    #[error("sufficiency provider list is invalid")]
    Providers,
    #[error("sufficiency source reference is invalid")]
    SourceRefs,
    #[error("sufficiency minimum candidate count is invalid")]
    MinimumCandidates,
}

impl SufficiencyContractV1 {
    pub fn validate(&self) -> Result<(), SufficiencyContractError> {
        if self.schema_version != SUFFICIENCY_CONTRACT_SCHEMA_VERSION {
            return Err(SufficiencyContractError::SchemaVersion);
        }
        if self.policy != SUFFICIENCY_POLICY {
            return Err(SufficiencyContractError::Policy);
        }
        if self.requirements.is_empty() || self.requirements.len() > MAX_SUFFICIENCY_REQUIREMENTS {
            return Err(SufficiencyContractError::RequirementCount);
        }
        if !(1..=MAX_CORRECTIVE_STAGES).contains(&self.max_corrective_stages) {
            return Err(SufficiencyContractError::CorrectiveStageLimit);
        }
        let mut requirement_ids = BTreeSet::new();
        for requirement in &self.requirements {
            if requirement.id.trim().is_empty()
                || requirement.id.len() > 128
                || !requirement_ids.insert(&requirement.id)
            {
                return Err(SufficiencyContractError::RequirementId);
            }
            if requirement.evidence_class.trim().is_empty()
                || requirement.evidence_class.len() > 128
            {
                return Err(SufficiencyContractError::EvidenceClass);
            }
            if requirement.minimum_candidates == 0 || requirement.minimum_candidates > 64 {
                return Err(SufficiencyContractError::MinimumCandidates);
            }
            let mut providers = BTreeSet::new();
            for provider in &requirement.acceptable_providers {
                if !providers.insert(provider.as_str()) {
                    return Err(SufficiencyContractError::Providers);
                }
            }
            if requirement.acceptable_providers.len() > ProviderId::ALL.len()
                || requirement.acceptable_source_refs.len() > 64
            {
                return Err(SufficiencyContractError::Providers);
            }
            let mut source_refs = BTreeSet::new();
            for source_ref in &requirement.acceptable_source_refs {
                if source_ref.trim().is_empty()
                    || source_ref.len() > 512
                    || !source_refs.insert(source_ref)
                {
                    return Err(SufficiencyContractError::SourceRefs);
                }
            }
        }
        Ok(())
    }

    /// Choose one acceptable alternate target lane for the bounded
    /// corrective action, or `None` when no acceptable alternate remains.
    /// The trigger provider is never selected here (pending §13.1) — a
    /// missing alternate is terminal, not a same-provider retry.
    pub fn alternate_target(
        &self,
        trigger: ProviderId,
        requirement_id: &str,
        expected_providers: &[ProviderId],
    ) -> Option<ProviderId> {
        let requirement = self
            .requirements
            .iter()
            .find(|candidate| candidate.id == requirement_id)?;
        let acceptable: &[ProviderId] = if requirement.acceptable_providers.is_empty() {
            expected_providers
        } else {
            &requirement.acceptable_providers
        };
        alternate_provider_for_requirement(expected_providers, trigger, acceptable)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SufficiencyStateV1 {
    Sufficient,
    Insufficient,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementCoverageStateV1 {
    Satisfied,
    Missing,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SufficiencyReasonV1 {
    Satisfied,
    NoMatchingCandidate,
    ProviderUnavailable,
    ProviderIncomplete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequirementCoverageV1 {
    pub requirement_id: String,
    pub state: RequirementCoverageStateV1,
    pub matching_candidates: u32,
    pub required_candidates: u32,
    pub reason: SufficiencyReasonV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SufficiencyAssessmentV1 {
    pub schema_version: u32,
    pub policy: String,
    pub state: SufficiencyStateV1,
    pub requirements: Vec<RequirementCoverageV1>,
}

impl SufficiencyAssessmentV1 {
    pub fn first_missing_requirement(&self) -> Option<&RequirementCoverageV1> {
        self.requirements
            .iter()
            .find(|requirement| requirement.state == RequirementCoverageStateV1::Missing)
    }
}

/// Read one valid planner-owned contract without inventing requirements from
/// task text. Invalid or absent extensions stay outside corrective execution;
/// [`receipt_for_request`] retains their typed unknown receipt.
pub fn validated_contract_for_request(
    request: &FederationRequestV1,
) -> Option<SufficiencyContractV1> {
    let value = request.extensions.get("sufficiencyContract")?;
    let contract = serde_json::from_value::<SufficiencyContractV1>(value.clone()).ok()?;
    contract.validate().ok()?;
    Some(contract)
}

/// Content-free receipt for the future single bounded corrective pass.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectiveRetrievalReceiptV1 {
    pub schema_version: u32,
    pub policy: String,
    pub triggered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_provider: Option<ProviderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_reason: Option<ReasonCode>,
    pub stage_limit: u32,
    pub attempted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_provider: Option<ProviderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_requirement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sufficiency: Option<SufficiencyAssessmentV1>,
    pub outcome: String,
}

impl CorrectiveRetrievalReceiptV1 {
    pub fn not_evaluated() -> Self {
        Self {
            schema_version: CORRECTIVE_RETRIEVAL_SCHEMA_VERSION,
            policy: CORRECTIVE_RETRIEVAL_POLICY.to_owned(),
            triggered: false,
            trigger_provider: None,
            trigger_reason: None,
            stage_limit: 1,
            attempted: false,
            target_provider: None,
            target_requirement: None,
            sufficiency: None,
            outcome: "not_evaluated_missing_sufficiency_contract".to_owned(),
        }
    }

    /// Compatibility constructor for callers that only need a non-triggered
    /// receipt. Production uses [`Self::not_evaluated`] explicitly.
    pub fn not_triggered() -> Self {
        Self::not_evaluated()
    }

    /// Construct a bounded-stage receipt once planner-owned sufficiency
    /// evidence exists. Federation does not call this until that contract is
    /// available.
    pub fn triggered(provider: ProviderId, reason: ReasonCode, target: Option<ProviderId>) -> Self {
        Self {
            schema_version: CORRECTIVE_RETRIEVAL_SCHEMA_VERSION,
            policy: CORRECTIVE_RETRIEVAL_POLICY.to_owned(),
            triggered: true,
            trigger_provider: Some(provider),
            trigger_reason: Some(reason),
            stage_limit: 1,
            attempted: false,
            target_provider: target,
            target_requirement: None,
            sufficiency: None,
            outcome: if target.is_some() {
                "pending".to_owned()
            } else {
                "unknown_no_alternate_lane".to_owned()
            },
        }
    }

    /// Publish the result of the single corrective stage. The final
    /// assessment is retained so a terminal second insufficiency is typed,
    /// rather than silently represented as an ordinary partial lane.
    pub fn after_stage(
        assessment: SufficiencyAssessmentV1,
        trigger_provider: ProviderId,
        target_provider: ProviderId,
        target_requirement: String,
        attempted: bool,
        outcome: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: CORRECTIVE_RETRIEVAL_SCHEMA_VERSION,
            policy: CORRECTIVE_RETRIEVAL_POLICY.to_owned(),
            triggered: true,
            trigger_provider: Some(trigger_provider),
            trigger_reason: None,
            stage_limit: MAX_CORRECTIVE_STAGES,
            attempted,
            target_provider: Some(target_provider),
            target_requirement: Some(target_requirement),
            sufficiency: Some(assessment),
            outcome: outcome.into(),
        }
    }

    /// Publish a typed terminal insufficiency when no alternate stage can be
    /// run. No provider I/O is implied by this constructor.
    pub fn terminal_insufficiency(
        assessment: SufficiencyAssessmentV1,
        trigger_provider: Option<ProviderId>,
        target_provider: Option<ProviderId>,
        target_requirement: Option<String>,
        outcome: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: CORRECTIVE_RETRIEVAL_SCHEMA_VERSION,
            policy: CORRECTIVE_RETRIEVAL_POLICY.to_owned(),
            triggered: target_provider.is_some(),
            trigger_provider,
            trigger_reason: None,
            stage_limit: MAX_CORRECTIVE_STAGES,
            attempted: false,
            target_provider,
            target_requirement,
            sufficiency: Some(assessment),
            outcome: outcome.into(),
        }
    }
}

impl SufficiencyContractV1 {
    /// Run the deterministic ingestion pre-filter on an explicit contract
    /// before the corrective stage may consume it: shape validation, then a
    /// specificity gate. Never calls a model; an ambiguous contract surfaces
    /// as a typed acceptance outcome, never as silently ignored requirements.
    pub fn ingest(&self) -> Result<SufficiencyContractIngestV1, SufficiencyContractError> {
        // Specificity gate first, then shape validation, so an empty
        // requirement set is a typed acceptance outcome rather than a
        // generic validation error. No model call on this path.
        if self.requirements.is_empty() {
            return Ok(SufficiencyContractIngestV1 {
                contract: self.clone(),
                outcome: ContractAcceptanceV1::InsufficientRequirements,
            });
        }
        self.validate()?;
        Ok(SufficiencyContractIngestV1 {
            contract: self.clone(),
            outcome: ContractAcceptanceV1::Accepted,
        })
    }
}

/// Typed outcome of the deterministic contract ingestion pre-filter. Every
/// rejected contract is observable; nothing is silently dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SufficiencyContractIngestV1 {
    pub contract: SufficiencyContractV1,
    pub outcome: ContractAcceptanceV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractAcceptanceV1 {
    Accepted,
    InsufficientRequirements,
}

/// Evaluate planner requirements against exactly the normalized provider
/// lanes expected for this request. Missing/partial lanes remain unknown;
/// absence of a matching candidate on complete lanes is a real insufficiency.
pub fn evaluate_sufficiency(
    contract: &SufficiencyContractV1,
    providers: &[NormalizedProviderOutput],
    expected_providers: &[ProviderId],
) -> Result<SufficiencyAssessmentV1, SufficiencyContractError> {
    contract.validate()?;
    let mut requirements = Vec::with_capacity(contract.requirements.len());

    for requirement in &contract.requirements {
        let allowed = if requirement.acceptable_providers.is_empty() {
            expected_providers.to_vec()
        } else {
            requirement.acceptable_providers.clone()
        };
        let lanes = providers
            .iter()
            .filter(|lane| {
                expected_providers.contains(&lane.provider) && allowed.contains(&lane.provider)
            })
            .collect::<Vec<_>>();
        let matching_candidates = lanes
            .iter()
            .flat_map(|lane| lane.candidates.iter())
            .filter(|candidate| candidate_matches(candidate, requirement))
            .count();
        let matching_candidates = u32::try_from(matching_candidates).unwrap_or(u32::MAX);
        let required_candidates = requirement.minimum_candidates;

        let (state, reason) = if matching_candidates >= required_candidates {
            (
                RequirementCoverageStateV1::Satisfied,
                SufficiencyReasonV1::Satisfied,
            )
        } else if allowed.is_empty()
            || allowed
                .iter()
                .any(|provider| !expected_providers.contains(provider))
            || allowed
                .iter()
                .any(|provider| !lanes.iter().any(|lane| lane.provider == *provider))
        {
            (
                RequirementCoverageStateV1::Unavailable,
                SufficiencyReasonV1::ProviderUnavailable,
            )
        } else if lanes
            .iter()
            .any(|lane| lane.status != FederationProviderStatusV1::Complete)
        {
            (
                RequirementCoverageStateV1::Unavailable,
                SufficiencyReasonV1::ProviderIncomplete,
            )
        } else {
            (
                RequirementCoverageStateV1::Missing,
                SufficiencyReasonV1::NoMatchingCandidate,
            )
        };

        requirements.push(RequirementCoverageV1 {
            requirement_id: requirement.id.clone(),
            state,
            matching_candidates,
            required_candidates,
            reason,
        });
    }

    let state = if requirements
        .iter()
        .any(|requirement| requirement.state == RequirementCoverageStateV1::Unavailable)
    {
        SufficiencyStateV1::Unknown
    } else if requirements
        .iter()
        .all(|requirement| requirement.state == RequirementCoverageStateV1::Satisfied)
    {
        SufficiencyStateV1::Sufficient
    } else {
        SufficiencyStateV1::Insufficient
    };

    Ok(SufficiencyAssessmentV1 {
        schema_version: SUFFICIENCY_CONTRACT_SCHEMA_VERSION,
        policy: SUFFICIENCY_POLICY.to_owned(),
        state,
        requirements,
    })
}

fn candidate_matches(
    candidate: &crate::normalize::NormalizedCandidate,
    requirement: &SufficiencyRequirementV1,
) -> bool {
    candidate.candidate.source_kind == requirement.evidence_class
        && (requirement.acceptable_source_refs.is_empty()
            || requirement
                .acceptable_source_refs
                .iter()
                .any(|source_ref| source_ref == &candidate.candidate.source_ref))
}

fn invalid_contract_receipt() -> CorrectiveRetrievalReceiptV1 {
    CorrectiveRetrievalReceiptV1 {
        schema_version: CORRECTIVE_RETRIEVAL_SCHEMA_VERSION,
        policy: CORRECTIVE_RETRIEVAL_POLICY.to_owned(),
        triggered: false,
        trigger_provider: None,
        trigger_reason: None,
        stage_limit: MAX_CORRECTIVE_STAGES,
        attempted: false,
        target_provider: None,
        target_requirement: None,
        sufficiency: None,
        outcome: "unknown_invalid_sufficiency_contract".to_owned(),
    }
}

/// Read a planner contract from the extensible request envelope and produce
/// a typed decision receipt. This function never performs provider I/O.
pub fn receipt_for_request(
    request: &FederationRequestV1,
    providers: &[NormalizedProviderOutput],
    expected_providers: &[ProviderId],
) -> CorrectiveRetrievalReceiptV1 {
    let Some(value) = request.extensions.get("sufficiencyContract") else {
        return CorrectiveRetrievalReceiptV1::not_evaluated();
    };
    let Ok(contract) = serde_json::from_value::<SufficiencyContractV1>(value.clone()) else {
        return invalid_contract_receipt();
    };
    if contract.validate().is_err() {
        return invalid_contract_receipt();
    }
    let Ok(assessment) = evaluate_sufficiency(&contract, providers, expected_providers) else {
        return invalid_contract_receipt();
    };
    let first_missing = assessment.first_missing_requirement();
    let target = assessment
        .requirements
        .iter()
        .filter(|requirement| requirement.state == RequirementCoverageStateV1::Missing)
        .find_map(|coverage| {
            let provider = contract
                .requirements
                .iter()
                .find(|requirement| requirement.id == coverage.requirement_id)
                .and_then(|requirement| {
                    requirement
                        .acceptable_providers
                        .iter()
                        .copied()
                        .find(|provider| expected_providers.contains(provider))
                });
            provider.map(|provider| (coverage.requirement_id.clone(), provider))
        });
    let target_requirement = target
        .as_ref()
        .map(|(requirement_id, _)| requirement_id.clone())
        .or_else(|| first_missing.map(|requirement| requirement.requirement_id.clone()));
    let target_provider = target.map(|(_, provider)| provider);
    CorrectiveRetrievalReceiptV1::from_assessment(
        assessment,
        target_provider,
        target_requirement,
        contract.max_corrective_stages,
    )
}

/// Select deterministic trigger and alternate lanes for first-party
/// corrective retrieval. Trigger is the first canonical complete lane that
/// participated in the missing requirement; target is one acceptable active
/// lane other than trigger, with Cortex/Blueprint preference.
pub fn corrective_plan(
    contract: &SufficiencyContractV1,
    assessment: &SufficiencyAssessmentV1,
    providers: &[NormalizedProviderOutput],
    expected_providers: &[ProviderId],
) -> Option<(ProviderId, ProviderId, String)> {
    let (trigger, requirement_id) =
        corrective_trigger(contract, assessment, providers, expected_providers)?;
    let requirement = contract
        .requirements
        .iter()
        .find(|candidate| candidate.id == requirement_id)?;
    let allowed = if requirement.acceptable_providers.is_empty() {
        expected_providers.to_vec()
    } else {
        requirement.acceptable_providers.clone()
    };
    let target = alternate_provider_for_requirement(expected_providers, trigger, &allowed)?;
    Some((trigger, target, requirement_id))
}

/// Select first canonical complete lane that participated in first missing
/// requirement. This identity is recorded as corrective trigger even when no
/// acceptable alternate lane remains active.
pub fn corrective_trigger(
    contract: &SufficiencyContractV1,
    assessment: &SufficiencyAssessmentV1,
    providers: &[NormalizedProviderOutput],
    expected_providers: &[ProviderId],
) -> Option<(ProviderId, String)> {
    let missing = assessment.first_missing_requirement()?;
    let requirement = contract
        .requirements
        .iter()
        .find(|candidate| candidate.id == missing.requirement_id)?;
    let allowed = if requirement.acceptable_providers.is_empty() {
        expected_providers.to_vec()
    } else {
        requirement.acceptable_providers.clone()
    };
    let trigger = ProviderId::ALL.into_iter().find(|provider| {
        expected_providers.contains(provider)
            && allowed.contains(provider)
            && providers.iter().any(|lane| {
                lane.provider == *provider && lane.status == FederationProviderStatusV1::Complete
            })
    })?;
    Some((trigger, missing.requirement_id.clone()))
}

impl CorrectiveRetrievalReceiptV1 {
    /// Convert a planner-owned assessment into a bounded, content-free
    /// corrective decision. Federation only plans one stage here; execution
    /// remains an explicit follow-up owned by the planner.
    pub fn from_assessment(
        assessment: SufficiencyAssessmentV1,
        target_provider: Option<ProviderId>,
        target_requirement: Option<String>,
        stage_limit: u32,
    ) -> Self {
        let stage_limit = stage_limit.clamp(1, MAX_CORRECTIVE_STAGES);
        let (triggered, target_provider, target_requirement, outcome) = match assessment.state {
            SufficiencyStateV1::Sufficient => (false, None, None, "sufficient".to_owned()),
            SufficiencyStateV1::Insufficient if target_provider.is_some() => (
                true,
                target_provider,
                target_requirement,
                "corrective_stage_planned".to_owned(),
            ),
            SufficiencyStateV1::Insufficient => (
                false,
                None,
                target_requirement,
                "insufficient_unknown_no_alternate_lane".to_owned(),
            ),
            SufficiencyStateV1::Unknown => (
                false,
                None,
                None,
                "unknown_provider_evidence_incomplete".to_owned(),
            ),
        };
        Self {
            schema_version: CORRECTIVE_RETRIEVAL_SCHEMA_VERSION,
            policy: CORRECTIVE_RETRIEVAL_POLICY.to_owned(),
            triggered,
            trigger_provider: None,
            trigger_reason: None,
            stage_limit,
            attempted: false,
            target_provider,
            target_requirement,
            sufficiency: Some(assessment),
            outcome,
        }
    }
}

/// Deterministic alternate-lane preference. Cortex and Blueprint are both
/// request-bound read lanes with no provider prerequisites in native runtime.
pub fn alternate_provider(active: &[ProviderId], trigger: ProviderId) -> Option<ProviderId> {
    alternate_provider_for_requirement(active, trigger, &[])
}

/// Deterministic alternate-lane preference constrained by planner-accepted
/// providers. An empty acceptance list means all active lanes.
pub fn alternate_provider_for_requirement(
    active: &[ProviderId],
    trigger: ProviderId,
    acceptable: &[ProviderId],
) -> Option<ProviderId> {
    [ProviderId::Cortex, ProviderId::Blueprint]
        .into_iter()
        .chain(ProviderId::ALL)
        .find(|provider| {
            *provider != trigger
                && active.contains(provider)
                && (acceptable.is_empty() || acceptable.contains(provider))
        })
}

/// Merge one future corrective output into its existing provider lane. The lane
/// keeps its original identity/generation, while candidate/warning/omission
/// evidence from one bounded retry is retained for ordinary normalization and
/// fusion.
pub fn append_output(outputs: &mut Vec<ProviderOutputV1>, mut correction: ProviderOutputV1) {
    let Some(initial) = outputs
        .iter_mut()
        .find(|output| output.provider == correction.provider)
    else {
        outputs.push(correction);
        return;
    };
    initial.candidates.append(&mut correction.candidates);
    initial.warnings.append(&mut correction.warnings);
    initial.omissions.append(&mut correction.omissions);
    if initial.status != membrane_protocol::FederationProviderStatusV1::Complete
        || correction.status != membrane_protocol::FederationProviderStatusV1::Complete
    {
        initial.status = membrane_protocol::FederationProviderStatusV1::Partial;
    }
    if initial.generation.is_none() {
        initial.generation = correction.generation;
    }
    if initial.diagnostics.is_none() {
        initial.diagnostics = correction.diagnostics;
    }
    let mut extensions = BTreeMap::new();
    extensions.append(&mut initial.extensions);
    extensions.extend(correction.extensions);
    initial.extensions = extensions;
}

/// Add content-free trigger evidence when a future corrective pass cannot
/// obtain a second output. Existing provider reasons remain authoritative; this
/// warning only labels the bounded stage outcome.
pub fn unknown_warning(provider: ProviderId) -> ProviderWarningV1 {
    ProviderWarningV1 {
        provider,
        reason: ReasonCode::ProviderUnavailable,
        severity: membrane_protocol::WarningSeverity::Warning,
        detail_id: Some("corrective_retrieval_insufficient".to_owned()),
        stage: Some("corrective_retrieval".to_owned()),
        message: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::{normalize_candidate, NORMALIZED_PROVIDER_VERSION};
    use membrane_protocol::{CandidateV1, FEDERATION_REQUEST_SCHEMA_VERSION};
    use std::collections::BTreeMap;

    fn candidate(source_kind: &str, source_ref: &str) -> CandidateV1 {
        CandidateV1 {
            id: format!("candidate-{source_kind}"),
            layer: 0,
            provider: None,
            source_kind: source_kind.to_owned(),
            source_ref: source_ref.to_owned(),
            source_hash: format!("sha256:{}", "0".repeat(64)),
            trust_class: "workspace".to_owned(),
            instruction_policy: "data_only".to_owned(),
            provider_score: 0.9,
            score_components: BTreeMap::new(),
            base_commit: None,
            overlay_digest: None,
            freshness_class: None,
            snapshot_id: None,
            estimated_tokens: 1,
            protected: false,
            exact: false,
            recoverable: true,
            resolver: "test".to_owned(),
            text: "content".to_owned(),
        }
    }

    fn lane(
        provider: ProviderId,
        status: FederationProviderStatusV1,
        candidates: Vec<CandidateV1>,
    ) -> NormalizedProviderOutput {
        NormalizedProviderOutput {
            schema_version: NORMALIZED_PROVIDER_VERSION,
            provider,
            status,
            generation: None,
            candidates: candidates
                .into_iter()
                .map(|candidate| {
                    normalize_candidate(
                        provider,
                        NORMALIZED_PROVIDER_VERSION,
                        None,
                        None,
                        candidate,
                    )
                    .expect("test candidate should normalize")
                })
                .collect(),
            warnings: Vec::new(),
            omissions: Vec::new(),
            extensions: BTreeMap::new(),
        }
    }

    fn contract(requirement: SufficiencyRequirementV1) -> SufficiencyContractV1 {
        SufficiencyContractV1 {
            schema_version: SUFFICIENCY_CONTRACT_SCHEMA_VERSION,
            policy: SUFFICIENCY_POLICY.to_owned(),
            requirements: vec![requirement],
            max_corrective_stages: MAX_CORRECTIVE_STAGES,
        }
    }

    fn requirement(
        evidence_class: &str,
        acceptable_providers: Vec<ProviderId>,
    ) -> SufficiencyRequirementV1 {
        SufficiencyRequirementV1 {
            id: "required-repository-evidence".to_owned(),
            evidence_class: evidence_class.to_owned(),
            acceptable_providers,
            acceptable_source_refs: Vec::new(),
            minimum_candidates: 1,
        }
    }

    fn request() -> FederationRequestV1 {
        FederationRequestV1 {
            schema_version: FEDERATION_REQUEST_SCHEMA_VERSION,
            request_id: "request-1".to_owned(),
            trace_id: "trace-1".to_owned(),
            task: "task".to_owned(),
            repository_root: "C:\\repo".to_owned(),
            client: "test".to_owned(),
            session_id: "session".to_owned(),
            deadline_ms: 1_000,
            max_tokens: 100,
            anchors: Vec::new(),
            scope_grant_id: None,
            manifest_digest: None,
            release_generation: None,
            blueprint_generation: None,
            skills_generation: None,
            extensions: BTreeMap::new(),
        }
    }

    #[test]
    fn production_receipt_defers_without_typed_sufficiency() {
        let receipt = CorrectiveRetrievalReceiptV1::not_evaluated();
        assert!(!receipt.triggered);
        assert!(!receipt.attempted);
        assert_eq!(receipt.stage_limit, 1);
        assert_eq!(
            receipt.outcome,
            "not_evaluated_missing_sufficiency_contract"
        );
        assert!(receipt.sufficiency.is_none());
        let encoded = serde_json::to_value(receipt).expect("receipt JSON");
        assert!(encoded.get("sufficiency").is_none());
        assert!(encoded.get("targetRequirement").is_none());
    }

    #[test]
    fn complete_matching_lane_is_sufficient() {
        let contract = contract(requirement("repository_file", vec![ProviderId::Blueprint]));
        let assessment = evaluate_sufficiency(
            &contract,
            &[lane(
                ProviderId::Blueprint,
                FederationProviderStatusV1::Complete,
                vec![candidate("repository_file", "README.md")],
            )],
            &[ProviderId::Blueprint],
        )
        .expect("valid contract");
        assert_eq!(assessment.state, SufficiencyStateV1::Sufficient);
        assert_eq!(
            assessment.requirements[0].reason,
            SufficiencyReasonV1::Satisfied
        );
    }

    #[test]
    fn complete_lane_without_matching_evidence_is_insufficient() {
        let contract = contract(requirement("repository_file", vec![ProviderId::Blueprint]));
        let assessment = evaluate_sufficiency(
            &contract,
            &[lane(
                ProviderId::Blueprint,
                FederationProviderStatusV1::Complete,
                vec![candidate("memory", "memory://one")],
            )],
            &[ProviderId::Blueprint],
        )
        .expect("valid contract");
        assert_eq!(assessment.state, SufficiencyStateV1::Insufficient);
        assert_eq!(
            assessment.requirements[0].state,
            RequirementCoverageStateV1::Missing
        );
        assert_eq!(
            assessment.requirements[0].reason,
            SufficiencyReasonV1::NoMatchingCandidate
        );
    }

    #[test]
    fn incomplete_lane_keeps_sufficiency_unknown() {
        let contract = contract(requirement("repository_file", vec![ProviderId::Blueprint]));
        let assessment = evaluate_sufficiency(
            &contract,
            &[lane(
                ProviderId::Blueprint,
                FederationProviderStatusV1::Partial,
                Vec::new(),
            )],
            &[ProviderId::Blueprint],
        )
        .expect("valid contract");
        assert_eq!(assessment.state, SufficiencyStateV1::Unknown);
        assert_eq!(
            assessment.requirements[0].reason,
            SufficiencyReasonV1::ProviderIncomplete
        );
    }

    #[test]
    fn missing_expected_lane_keeps_sufficiency_unknown() {
        let contract = contract(requirement("repository_file", vec![ProviderId::Blueprint]));
        let assessment =
            evaluate_sufficiency(&contract, &[], &[ProviderId::Blueprint]).expect("valid contract");
        assert_eq!(assessment.state, SufficiencyStateV1::Unknown);
        assert_eq!(
            assessment.requirements[0].reason,
            SufficiencyReasonV1::ProviderUnavailable
        );
    }

    #[test]
    fn insufficient_contract_plans_one_targeted_stage_without_attempting_it() {
        let mut request = request();
        let contract = contract(requirement(
            "repository_file",
            vec![ProviderId::Blueprint, ProviderId::Cortex],
        ));
        request.extensions.insert(
            "sufficiencyContract".to_owned(),
            serde_json::to_value(contract).expect("contract JSON"),
        );
        let receipt = receipt_for_request(
            &request,
            &[
                lane(
                    ProviderId::Blueprint,
                    FederationProviderStatusV1::Complete,
                    Vec::new(),
                ),
                lane(
                    ProviderId::Cortex,
                    FederationProviderStatusV1::Complete,
                    Vec::new(),
                ),
            ],
            &[ProviderId::Blueprint, ProviderId::Cortex],
        );
        assert!(receipt.triggered);
        assert!(!receipt.attempted);
        assert_eq!(receipt.target_provider, Some(ProviderId::Blueprint));
        assert_eq!(
            receipt.target_requirement.as_deref(),
            Some("required-repository-evidence")
        );
        assert_eq!(receipt.outcome, "corrective_stage_planned");
        assert_eq!(
            receipt
                .sufficiency
                .as_ref()
                .map(|assessment| assessment.state),
            Some(SufficiencyStateV1::Insufficient)
        );
    }

    #[test]
    fn unknown_contract_state_never_triggers_retry() {
        let assessment = SufficiencyAssessmentV1 {
            schema_version: SUFFICIENCY_CONTRACT_SCHEMA_VERSION,
            policy: SUFFICIENCY_POLICY.to_owned(),
            state: SufficiencyStateV1::Unknown,
            requirements: Vec::new(),
        };
        let receipt = CorrectiveRetrievalReceiptV1::from_assessment(
            assessment,
            Some(ProviderId::Blueprint),
            Some("required-repository-evidence".to_owned()),
            1,
        );
        assert!(!receipt.triggered);
        assert!(!receipt.attempted);
        assert!(receipt.target_provider.is_none());
        assert_eq!(receipt.outcome, "unknown_provider_evidence_incomplete");
    }

    #[test]
    fn invalid_or_malformed_request_contract_is_unknown_without_retry() {
        let mut malformed = request();
        malformed.extensions.insert(
            "sufficiencyContract".to_owned(),
            serde_json::json!({"schemaVersion": 99}),
        );
        let receipt = receipt_for_request(&malformed, &[], &[ProviderId::Blueprint]);
        assert!(!receipt.triggered);
        assert!(!receipt.attempted);
        assert_eq!(receipt.outcome, "unknown_invalid_sufficiency_contract");

        let mut invalid = request();
        invalid.extensions.insert(
            "sufficiencyContract".to_owned(),
            serde_json::to_value(SufficiencyContractV1 {
                schema_version: SUFFICIENCY_CONTRACT_SCHEMA_VERSION,
                policy: SUFFICIENCY_POLICY.to_owned(),
                requirements: Vec::new(),
                max_corrective_stages: 1,
            })
            .expect("contract JSON"),
        );
        let receipt = receipt_for_request(&invalid, &[], &[ProviderId::Blueprint]);
        assert_eq!(receipt.outcome, "unknown_invalid_sufficiency_contract");
    }

    #[test]
    fn alternate_lane_preference_is_stable_and_excludes_trigger() {
        let active = ProviderId::ALL.to_vec();
        assert_eq!(
            alternate_provider(&active, ProviderId::Blueprint),
            Some(ProviderId::Cortex)
        );
        assert_eq!(
            alternate_provider(&active, ProviderId::Cortex),
            Some(ProviderId::Blueprint)
        );
    }
}
