//! Remediation proposals — separate artifact class from issues and taste.
//!
//! Remediation proposals never modify issue or preference state directly;
//! they are proposals that a human or deterministic verifier accepts. The
//! `taste_candidate` effect additionally requires qualifying user evidence,
//! and the precision gate (>= 0.95 measured on the family's labelled corpus)
//! must pass before any proposal may be surfaced as actionable.

use serde::{Deserialize, Serialize};

/// Remediation effect classes (canon §6.4). `taste_candidate` is special: it
/// proposes a new preference and therefore inherits evidence requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemediationEffect {
    ProcessChange,
    GuardrailAddition,
    DocumentationUpdate,
    ToolingFix,
    /// Proposes converting an insight into a Taste preference candidate.
    TasteCandidate,
}

impl RemediationEffect {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProcessChange => "process_change",
            Self::GuardrailAddition => "guardrail_addition",
            Self::DocumentationUpdate => "documentation_update",
            Self::ToolingFix => "tooling_fix",
            Self::TasteCandidate => "taste_candidate",
        }
    }

    /// Default surface used by the backwards-compatible builder. Callers
    /// that know the intended surface should use `build_with_target`.
    pub const fn default_target(self) -> InterventionTarget {
        match self {
            Self::ProcessChange => InterventionTarget::SkillOrProcedure,
            Self::GuardrailAddition => InterventionTarget::Guard,
            Self::DocumentationUpdate => InterventionTarget::DocumentationPolicy,
            Self::ToolingFix => InterventionTarget::ToolImplementation,
            Self::TasteCandidate => InterventionTarget::ModelBehaviorPolicy,
        }
    }
}

/// The surface changed by a remediation. This is orthogonal to
/// `RemediationEffect`: one effect can be directed at several surfaces.
/// `TasteCandidate` intentionally is not a target; it remains an effect that
/// requires separate qualifying user evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum InterventionTarget {
    ModelBehaviorPolicy,
    SkillOrProcedure,
    SystemPrompt,
    ToolDescription,
    ToolImplementation,
    RoutingPolicy,
    ContextRetrieval,
    ContextReduction,
    Orchestration,
    Guard,
    Evaluator,
    DocumentationPolicy,
}

impl InterventionTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelBehaviorPolicy => "model_behavior_policy",
            Self::SkillOrProcedure => "skill_or_procedure",
            Self::SystemPrompt => "system_prompt",
            Self::ToolDescription => "tool_description",
            Self::ToolImplementation => "tool_implementation",
            Self::RoutingPolicy => "routing_policy",
            Self::ContextRetrieval => "context_retrieval",
            Self::ContextReduction => "context_reduction",
            Self::Orchestration => "orchestration",
            Self::Guard => "guard",
            Self::Evaluator => "evaluator",
            Self::DocumentationPolicy => "documentation_policy",
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::ModelBehaviorPolicy,
            Self::SkillOrProcedure,
            Self::SystemPrompt,
            Self::ToolDescription,
            Self::ToolImplementation,
            Self::RoutingPolicy,
            Self::ContextRetrieval,
            Self::ContextReduction,
            Self::Orchestration,
            Self::Guard,
            Self::Evaluator,
            Self::DocumentationPolicy,
        ]
    }
}

impl Default for InterventionTarget {
    fn default() -> Self {
        Self::SkillOrProcedure
    }
}

/// Canonical output kind consumed by host variant generation. The kind names
/// describe the proposed artifact, not the effect or target surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum RemediationProposalKind {
    GuardProposal,
    EvaluatorProposal,
    RoutingRecommendation,
    ReviewWarning,
    WorkflowChangeProposal,
    TasteCandidate,
}

impl RemediationProposalKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GuardProposal => "guard_proposal",
            Self::EvaluatorProposal => "evaluator_proposal",
            Self::RoutingRecommendation => "routing_recommendation",
            Self::ReviewWarning => "review_warning",
            Self::WorkflowChangeProposal => "workflow_change_proposal",
            Self::TasteCandidate => "taste_candidate",
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::GuardProposal,
            Self::EvaluatorProposal,
            Self::RoutingRecommendation,
            Self::ReviewWarning,
            Self::WorkflowChangeProposal,
            Self::TasteCandidate,
        ]
    }
}

impl InterventionTarget {
    pub const fn proposal_kind(self) -> RemediationProposalKind {
        match self {
            Self::ModelBehaviorPolicy
            | Self::SkillOrProcedure
            | Self::SystemPrompt
            | Self::ContextReduction => RemediationProposalKind::WorkflowChangeProposal,
            Self::ToolDescription | Self::DocumentationPolicy => {
                RemediationProposalKind::ReviewWarning
            }
            Self::ToolImplementation | Self::Evaluator => {
                RemediationProposalKind::EvaluatorProposal
            }
            Self::RoutingPolicy | Self::ContextRetrieval | Self::Orchestration => {
                RemediationProposalKind::RoutingRecommendation
            }
            Self::Guard => RemediationProposalKind::GuardProposal,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemediationError {
    MissingUserEvidenceForTasteCandidate,
    PrecisionGateNotMet { measured: f64, required: f64 },
    Seal(RemediationSealError),
}

impl std::fmt::Display for RemediationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemediationError::MissingUserEvidenceForTasteCandidate => write!(
                f,
                "taste_candidate remediation requires qualifying selected-transcript user evidence"
            ),
            RemediationError::PrecisionGateNotMet { measured, required } => write!(
                f,
                "precision gate not met: measured {measured:.3} < required {required:.3}"
            ),
            RemediationError::Seal(error) => write!(f, "remediation seal rejected: {error:?}"),
        }
    }
}

impl std::error::Error for RemediationError {}

pub const PRECISION_GATE_THRESHOLD: f64 = 0.95;

/// Additive schema revision for the orthogonal effect/target split.
pub const SEALED_REMEDIATION_SCHEMA: &str = "1.1.0";
pub const REMEDIATION_PAYLOAD_DIGEST_VERSION: &str = "sha256-canonical-json-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationUserEvidenceV1 {
    pub signal_class: String,
    pub act: String,
    pub evidence_digest: String,
    pub captured_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationUserEvidenceEnvelopeV1 {
    pub qualifying: bool,
    pub signals: Vec<RemediationUserEvidenceV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationPayloadV1 {
    pub record_kind: String,
    pub proposal_kind: String,
    pub effect: String,
    pub intervention_target: String,
    pub source_issue_ids: Vec<String>,
    pub canonical_proposal_text: String,
    pub authority_class: String,
    pub effect_boundary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_evidence: Option<RemediationUserEvidenceEnvelopeV1>,
    pub honesty_limit: String,
    pub admission_policy_version: String,
    pub redaction_contract_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_validator_receipt_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationStateReceiptV1 {
    pub transition: String,
    pub at: String,
    pub actor: String,
    pub prev_status: Option<String>,
    pub new_status: String,
    pub receipt_id: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationStateV1 {
    pub status: String,
    pub updated_at: String,
    pub receipts: Vec<RemediationStateReceiptV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedRemediationProposalV1 {
    pub schema_version: String,
    pub contract: String,
    pub proposal_id: String,
    pub payload_sha256: String,
    pub payload: RemediationPayloadV1,
    pub state: RemediationStateV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemediationSealError {
    InvalidSchemaVersion,
    InvalidContract,
    InvalidIssueId,
    InvalidProposalId,
    EmptyProposal,
    InvalidAuthority,
    InvalidEffect,
    InvalidInterventionTarget,
    InvalidProposalKind,
    InvalidUserEvidence,
    UnexpectedUserEvidence,
    InvalidValidatorReceipt,
    PayloadDigestMismatch,
}

pub fn proposal_kind_for(
    effect: RemediationEffect,
    target: InterventionTarget,
) -> RemediationProposalKind {
    if effect == RemediationEffect::TasteCandidate {
        RemediationProposalKind::TasteCandidate
    } else {
        target.proposal_kind()
    }
}

/// Backwards-compatible default mapping for callers that only have an effect.
/// New callers should use [`proposal_kind_for`] so target remains explicit.
pub fn proposal_kind(effect: RemediationEffect) -> &'static str {
    proposal_kind_for(effect, effect.default_target()).as_str()
}

fn valid_prefixed_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| digest.len() == 64 && digest.bytes().all(is_lower_hex))
}

fn is_lower_hex(byte: u8) -> bool {
    matches!(byte, b'0'..=b'9' | b'a'..=b'f')
}

fn valid_issue_id(value: &str) -> bool {
    value
        .strip_prefix("ii_")
        .is_some_and(|suffix| suffix.len() == 64 && suffix.bytes().all(is_lower_hex))
}

fn valid_proposal_id(value: &str) -> bool {
    value
        .strip_prefix("rem_")
        .is_some_and(|suffix| suffix.len() == 64 && suffix.bytes().all(is_lower_hex))
}

fn valid_receipt_id(value: &str) -> bool {
    value
        .strip_prefix("rcpt_")
        .is_some_and(|suffix| suffix.len() == 32 && suffix.bytes().all(is_lower_hex))
}

fn parse_effect(value: &str) -> Option<RemediationEffect> {
    match value {
        "process_change" => Some(RemediationEffect::ProcessChange),
        "guardrail_addition" => Some(RemediationEffect::GuardrailAddition),
        "documentation_update" => Some(RemediationEffect::DocumentationUpdate),
        "tooling_fix" => Some(RemediationEffect::ToolingFix),
        "taste_candidate" => Some(RemediationEffect::TasteCandidate),
        _ => None,
    }
}

fn parse_target(value: &str) -> Option<InterventionTarget> {
    InterventionTarget::all()
        .iter()
        .copied()
        .find(|target| target.as_str() == value)
}

fn parse_proposal_kind(value: &str) -> Option<RemediationProposalKind> {
    RemediationProposalKind::all()
        .iter()
        .copied()
        .find(|kind| kind.as_str() == value)
}

impl SealedRemediationProposalV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn seal(
        proposal: &RemediationProposalV1,
        effect_boundary: &str,
        honesty_limit: &str,
        admission_policy_version: &str,
        redaction_contract_version: &str,
        semantic_validator_receipt_id: Option<&str>,
        user_evidence: Vec<RemediationUserEvidenceV1>,
        actor: &str,
        at: &str,
    ) -> Result<Self, RemediationSealError> {
        if !valid_issue_id(&proposal.issue_id) {
            return Err(RemediationSealError::InvalidIssueId);
        }
        if !valid_proposal_id(&proposal.proposal_id) {
            return Err(RemediationSealError::InvalidProposalId);
        }
        if proposal.text.trim().is_empty()
            || effect_boundary.trim().is_empty()
            || honesty_limit.trim().is_empty()
            || admission_policy_version.trim().is_empty()
            || redaction_contract_version.trim().is_empty()
            || actor.trim().is_empty()
            || at.trim().is_empty()
        {
            return Err(RemediationSealError::EmptyProposal);
        }
        if let Some(receipt) = semantic_validator_receipt_id {
            if !valid_receipt_id(receipt) {
                return Err(RemediationSealError::InvalidValidatorReceipt);
            }
        }
        if proposal.effect == RemediationEffect::TasteCandidate {
            if user_evidence.is_empty()
                || user_evidence.iter().any(|evidence| {
                    !matches!(
                        evidence.signal_class.as_str(),
                        "user_authoritative" | "user_behavioral"
                    ) || !matches!(
                        evidence.act.as_str(),
                        "explicit_statement"
                            | "accept"
                            | "reject"
                            | "post_accept_edit"
                            | "named_choice"
                    ) || !valid_prefixed_digest(&evidence.evidence_digest)
                        || evidence.captured_at.trim().is_empty()
                })
            {
                return Err(RemediationSealError::InvalidUserEvidence);
            }
        } else if !user_evidence.is_empty() {
            return Err(RemediationSealError::UnexpectedUserEvidence);
        }
        let payload = RemediationPayloadV1 {
            record_kind: "remediation_proposal".into(),
            proposal_kind: proposal_kind_for(proposal.effect, proposal.intervention_target)
                .as_str()
                .into(),
            effect: proposal.effect.as_str().into(),
            intervention_target: proposal.intervention_target.as_str().into(),
            source_issue_ids: vec![proposal.issue_id.clone()],
            canonical_proposal_text: crate::canonical::normalize_text(&proposal.text),
            authority_class: "none".into(),
            effect_boundary: effect_boundary.into(),
            user_evidence: (proposal.effect == RemediationEffect::TasteCandidate).then_some(
                RemediationUserEvidenceEnvelopeV1 {
                    qualifying: true,
                    signals: user_evidence,
                },
            ),
            honesty_limit: honesty_limit.into(),
            admission_policy_version: admission_policy_version.into(),
            redaction_contract_version: redaction_contract_version.into(),
            semantic_validator_receipt_id: semantic_validator_receipt_id.map(str::to_owned),
        };
        let payload_sha256 = crate::canonical::sha256_canonical(
            &serde_json::to_value(&payload).expect("remediation payload serializes"),
        );
        let receipt_material = format!("{}\0{}\0{}", proposal.proposal_id, payload_sha256, at);
        Ok(Self {
            schema_version: SEALED_REMEDIATION_SCHEMA.into(),
            contract: "RemediationProposalV1".into(),
            proposal_id: proposal.proposal_id.clone(),
            payload_sha256,
            payload,
            state: RemediationStateV1 {
                status: "proposed".into(),
                updated_at: at.into(),
                receipts: vec![RemediationStateReceiptV1 {
                    transition: "proposal_sealed".into(),
                    at: at.into(),
                    actor: actor.into(),
                    prev_status: None,
                    new_status: "proposed".into(),
                    receipt_id: format!(
                        "rcpt_{}",
                        &crate::canonical::sha256_hex(receipt_material.as_bytes())[..32]
                    ),
                    note: "immutable remediation semantics sealed".into(),
                }],
            },
        })
    }

    pub fn verify(&self) -> Result<(), RemediationSealError> {
        if self.schema_version != SEALED_REMEDIATION_SCHEMA {
            return Err(RemediationSealError::InvalidSchemaVersion);
        }
        if self.contract != "RemediationProposalV1" {
            return Err(RemediationSealError::InvalidContract);
        }
        if !valid_proposal_id(&self.proposal_id) {
            return Err(RemediationSealError::InvalidProposalId);
        }
        if self.payload.record_kind != "remediation_proposal" {
            return Err(RemediationSealError::InvalidContract);
        }
        if self.payload.authority_class != "none" {
            return Err(RemediationSealError::InvalidAuthority);
        }
        if self.payload.source_issue_ids.is_empty()
            || self
                .payload
                .source_issue_ids
                .iter()
                .any(|issue_id| !valid_issue_id(issue_id))
        {
            return Err(RemediationSealError::InvalidIssueId);
        }
        let effect =
            parse_effect(&self.payload.effect).ok_or(RemediationSealError::InvalidEffect)?;
        let target = parse_target(&self.payload.intervention_target)
            .ok_or(RemediationSealError::InvalidInterventionTarget)?;
        let kind = parse_proposal_kind(&self.payload.proposal_kind)
            .ok_or(RemediationSealError::InvalidProposalKind)?;
        if proposal_kind_for(effect, target) != kind {
            return Err(RemediationSealError::InvalidProposalKind);
        }
        if self.payload.canonical_proposal_text.trim().is_empty()
            || self.payload.effect_boundary.trim().is_empty()
            || self.payload.honesty_limit.trim().is_empty()
            || self.payload.admission_policy_version.trim().is_empty()
            || self.payload.redaction_contract_version.trim().is_empty()
        {
            return Err(RemediationSealError::EmptyProposal);
        }
        if let Some(receipt) = &self.payload.semantic_validator_receipt_id {
            if !valid_receipt_id(receipt) {
                return Err(RemediationSealError::InvalidValidatorReceipt);
            }
        }
        match (kind, &self.payload.user_evidence) {
            (RemediationProposalKind::TasteCandidate, Some(evidence))
                if evidence.qualifying
                    && !evidence.signals.is_empty()
                    && evidence.signals.iter().all(|signal| {
                        matches!(
                            signal.signal_class.as_str(),
                            "user_authoritative" | "user_behavioral"
                        ) && matches!(
                            signal.act.as_str(),
                            "explicit_statement"
                                | "accept"
                                | "reject"
                                | "post_accept_edit"
                                | "named_choice"
                        ) && valid_prefixed_digest(&signal.evidence_digest)
                            && !signal.captured_at.trim().is_empty()
                    }) => {}
            (RemediationProposalKind::TasteCandidate, _) => {
                return Err(RemediationSealError::InvalidUserEvidence)
            }
            (_, Some(_)) => return Err(RemediationSealError::UnexpectedUserEvidence),
            (_, None) => {}
        }
        let digest = crate::canonical::sha256_canonical(
            &serde_json::to_value(&self.payload).expect("remediation payload serializes"),
        );
        if digest != self.payload_sha256 {
            return Err(RemediationSealError::PayloadDigestMismatch);
        }
        Ok(())
    }
}

/// A remediation proposal bound to one insight issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationProposalV1 {
    pub proposal_id: String,
    pub issue_id: String,
    pub effect: RemediationEffect,
    /// New callers should select this explicitly with `build_with_target`.
    /// Missing targets in legacy proposal JSON use the historical procedure
    /// surface so additive deserialization remains possible.
    #[serde(default)]
    pub intervention_target: InterventionTarget,
    pub text: String,
    /// For `TasteCandidate`: IDs of qualifying user-evidence objects backing
    /// the proposed preference. Empty is valid only for non-taste effects.
    #[serde(default)]
    pub supporting_user_evidence_ids: Vec<String>,
    /// Family of the originating issue; used to look up corpus precision.
    pub origin_family: String,
}

impl RemediationProposalV1 {
    pub fn build(
        issue_id: &str,
        origin_family: &str,
        effect: RemediationEffect,
        text: &str,
        supporting_user_evidence_ids: Vec<String>,
    ) -> Self {
        // Preserve legacy identity derivation for existing callers. The
        // target-aware builder below includes target in identity so distinct
        // surfaces remain independently measurable.
        let id_src = format!("{issue_id}\u{0}{effect:?}\u{0}{text}");
        Self {
            proposal_id: format!("rem_{}", crate::canonical::sha256_hex(id_src.as_bytes())),
            issue_id: issue_id.to_string(),
            effect,
            intervention_target: effect.default_target(),
            text: text.to_string(),
            supporting_user_evidence_ids,
            origin_family: origin_family.to_string(),
        }
    }

    pub fn build_with_target(
        issue_id: &str,
        origin_family: &str,
        effect: RemediationEffect,
        intervention_target: InterventionTarget,
        text: &str,
        supporting_user_evidence_ids: Vec<String>,
    ) -> Self {
        let id_src = format!(
            "{issue_id}\u{0}{}\u{0}{}\u{0}{text}",
            effect.as_str(),
            intervention_target.as_str()
        );
        let id = format!("rem_{}", crate::canonical::sha256_hex(id_src.as_bytes()));
        Self {
            proposal_id: id,
            issue_id: issue_id.to_string(),
            effect,
            intervention_target,
            text: text.to_string(),
            supporting_user_evidence_ids,
            origin_family: origin_family.to_string(),
        }
    }

    /// Validate evidence requirements. `selected_user_evidence_ids` are
    /// event IDs from caller-selected external-user transcript sources; the
    /// proposal's claimed IDs must all be present in that set.
    pub fn validate_evidence(
        &self,
        selected_user_evidence_ids: &std::collections::BTreeSet<String>,
    ) -> Result<(), RemediationError> {
        if self.effect != RemediationEffect::TasteCandidate {
            return Ok(());
        }
        if self.supporting_user_evidence_ids.is_empty()
            || !self
                .supporting_user_evidence_ids
                .iter()
                .all(|id| selected_user_evidence_ids.contains(id))
        {
            return Err(RemediationError::MissingUserEvidenceForTasteCandidate);
        }
        Ok(())
    }

    /// Precision gate: the family's measured precision on its labelled corpus
    /// must be >= 0.95 before this proposal may be surfaced as actionable.
    pub fn precision_gate(&self, family_precision: Option<f64>) -> Result<(), RemediationError> {
        let measured = family_precision.unwrap_or(0.0);
        if measured + 1e-9 >= PRECISION_GATE_THRESHOLD {
            Ok(())
        } else {
            Err(RemediationError::PrecisionGateNotMet {
                measured,
                required: PRECISION_GATE_THRESHOLD,
            })
        }
    }

    /// Accepting a remediation proposal returns a receipt; it does NOT itself
    /// mutate the issue — callers use `outcomes` to record mitigation.
    pub fn acceptance_receipt(&self) -> String {
        let src = format!("accept\u{0}{}\u{0}{}", self.proposal_id, self.text);
        crate::canonical::sha256_hex(src.as_bytes())
    }
}

/// Seal a proposal for an actionable host review after both independent gates
/// have passed. This is the production boundary; direct `seal` remains the
/// lower-level semantic constructor used by deterministic callers.
#[allow(clippy::too_many_arguments)]
pub fn seal_actionable(
    proposal: &RemediationProposalV1,
    selected_user_evidence_ids: &std::collections::BTreeSet<String>,
    family_precision: Option<f64>,
    effect_boundary: &str,
    honesty_limit: &str,
    admission_policy_version: &str,
    redaction_contract_version: &str,
    semantic_validator_receipt_id: Option<&str>,
    user_evidence: Vec<RemediationUserEvidenceV1>,
    actor: &str,
    at: &str,
) -> Result<SealedRemediationProposalV1, RemediationError> {
    proposal.validate_evidence(selected_user_evidence_ids)?;
    proposal.precision_gate(family_precision)?;
    SealedRemediationProposalV1::seal(
        proposal,
        effect_boundary,
        honesty_limit,
        admission_policy_version,
        redaction_contract_version,
        semantic_validator_receipt_id,
        user_evidence,
        actor,
        at,
    )
    .map_err(RemediationError::Seal)
}

pub const REVIEW_REMEDIATION_EFFECT_BOUNDARY: &str = "requires_human_review";
pub const REVIEW_REMEDIATION_HONESTY_LIMIT: &str =
    "diagnostic proposal only; no approval, user preference, or execution is implied";
pub const REVIEW_REMEDIATION_ADMISSION_POLICY: &str = "adapt-remediation-v1";
pub const REVIEW_REMEDIATION_REDACTION_CONTRACT: &str = "adapt-redaction-v1";

/// Deterministic target choice for the review-only remediation emitted by the
/// native mining path. It never marks a proposal actionable; host review and
/// the precision gate remain separate.
pub fn target_for_issue_family(family: &str) -> InterventionTarget {
    match family {
        "ignored_tool_failure"
        | "verification_theatre"
        | "false_completion_claim"
        | "instruction_noncompliance" => InterventionTarget::Guard,
        "unproductive_broad_searching" | "false_not_found" => InterventionTarget::ContextRetrieval,
        "repeated_scope_expansion" | "unaccepted_plan_change" => InterventionTarget::Orchestration,
        _ => InterventionTarget::SkillOrProcedure,
    }
}

/// Production mining emits sealed, non-actionable review proposals for each
/// recurring issue. Missing timestamps are retained as an explicit omission:
/// no fabricated observation time is introduced into a sealed record.
pub fn seal_review_proposals(
    issues: &[crate::insights::InsightIssueV1],
) -> Vec<SealedRemediationProposalV1> {
    issues
        .iter()
        .filter_map(|issue| {
            let at = issue.last_seen.as_deref().or(issue.first_seen.as_deref())?;
            let target = target_for_issue_family(&issue.family);
            let text = format!(
                "Review {} remediation: {}",
                issue.family, issue.canonical_description
            );
            let proposal = RemediationProposalV1::build_with_target(
                &issue.issue_id,
                &issue.family,
                RemediationEffect::GuardrailAddition,
                target,
                &text,
                Vec::new(),
            );
            SealedRemediationProposalV1::seal(
                &proposal,
                REVIEW_REMEDIATION_EFFECT_BOUNDARY,
                REVIEW_REMEDIATION_HONESTY_LIMIT,
                REVIEW_REMEDIATION_ADMISSION_POLICY,
                REVIEW_REMEDIATION_REDACTION_CONTRACT,
                None,
                Vec::new(),
                "adapt_mine",
                at,
            )
            .ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn prop(effect: RemediationEffect, evidence: Vec<String>) -> RemediationProposalV1 {
        RemediationProposalV1::build("issue-1", "repeated_ask", effect, "do x", evidence)
    }

    #[test]
    fn taste_candidate_requires_selected_transcript_evidence() {
        let p = prop(RemediationEffect::TasteCandidate, vec!["ev-9".into()]);
        let mut auth = BTreeSet::new();
        assert!(p.validate_evidence(&auth).is_err());
        auth.insert("ev-9".into());
        assert!(p.validate_evidence(&auth).is_ok());
        // Non-taste effects need no evidence.
        assert!(prop(RemediationEffect::ProcessChange, vec![])
            .validate_evidence(&BTreeSet::new())
            .is_ok());
    }

    #[test]
    fn precision_gate_blocks_low_precision_families() {
        let p = prop(RemediationEffect::ProcessChange, vec![]);
        assert!(p.precision_gate(Some(0.94)).is_err());
        assert!(p.precision_gate(Some(0.95)).is_ok());
        assert!(p.precision_gate(None).is_err());
    }

    #[test]
    fn ids_are_deterministic() {
        let a = prop(RemediationEffect::ProcessChange, vec![]);
        let b = prop(RemediationEffect::ProcessChange, vec![]);
        assert_eq!(a.proposal_id, b.proposal_id);
        assert_eq!(a.acceptance_receipt(), b.acceptance_receipt());
    }

    #[test]
    fn target_is_orthogonal_and_all_kinds_are_reachable() {
        let mut kinds = BTreeSet::new();
        for target in InterventionTarget::all() {
            kinds.insert(proposal_kind_for(
                RemediationEffect::GuardrailAddition,
                *target,
            ));
        }
        kinds.insert(proposal_kind_for(
            RemediationEffect::TasteCandidate,
            InterventionTarget::ModelBehaviorPolicy,
        ));
        assert_eq!(
            kinds,
            RemediationProposalKind::all().iter().copied().collect()
        );
        for effect in [
            RemediationEffect::ProcessChange,
            RemediationEffect::GuardrailAddition,
            RemediationEffect::DocumentationUpdate,
            RemediationEffect::ToolingFix,
            RemediationEffect::TasteCandidate,
        ] {
            assert!(RemediationProposalKind::all()
                .contains(&proposal_kind_for(effect, effect.default_target())));
        }

        let route = RemediationProposalV1::build_with_target(
            "issue",
            "family",
            RemediationEffect::ProcessChange,
            InterventionTarget::RoutingPolicy,
            "route work",
            vec![],
        );
        assert_eq!(route.effect, RemediationEffect::ProcessChange);
        assert_eq!(route.intervention_target, InterventionTarget::RoutingPolicy);
        assert_eq!(
            proposal_kind_for(route.effect, route.intervention_target),
            RemediationProposalKind::RoutingRecommendation
        );
    }

    #[test]
    fn schema_revision_and_target_change_sealed_digest() {
        let issue = format!("ii_{}", "a".repeat(64));
        let first = RemediationProposalV1::build_with_target(
            &issue,
            "family",
            RemediationEffect::ProcessChange,
            InterventionTarget::SkillOrProcedure,
            "review procedure",
            vec![],
        );
        let second = RemediationProposalV1::build_with_target(
            &issue,
            "family",
            RemediationEffect::ProcessChange,
            InterventionTarget::RoutingPolicy,
            "review procedure",
            vec![],
        );
        let seal = |proposal: &RemediationProposalV1| {
            SealedRemediationProposalV1::seal(
                proposal,
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
        };
        let first_sealed = seal(&first);
        let second_sealed = seal(&second);
        assert_eq!(first_sealed.schema_version, SEALED_REMEDIATION_SCHEMA);
        assert_eq!(first_sealed.verify(), Ok(()));
        assert_ne!(first_sealed.payload_sha256, second_sealed.payload_sha256);
        assert_eq!(
            first_sealed.payload.intervention_target,
            "skill_or_procedure"
        );
    }

    #[test]
    fn actionable_seal_keeps_evidence_and_precision_gates() {
        let issue = format!("ii_{}", "b".repeat(64));
        let proposal = RemediationProposalV1::build_with_target(
            &issue,
            "family",
            RemediationEffect::TasteCandidate,
            InterventionTarget::ModelBehaviorPolicy,
            "prefer focused verification",
            vec!["ev-1".into()],
        );
        let selected = BTreeSet::from(["ev-1".to_string()]);
        let evidence = vec![RemediationUserEvidenceV1 {
            signal_class: "user_authoritative".into(),
            act: "explicit_statement".into(),
            evidence_digest: format!("sha256:{}", "c".repeat(64)),
            captured_at: "2026-08-25T00:00:00Z".into(),
        }];
        assert!(matches!(
            seal_actionable(
                &proposal,
                &selected,
                Some(0.94),
                "requires_human_review",
                "proposal only",
                "policy-v1",
                "redaction-v1",
                None,
                evidence.clone(),
                "adapt",
                "2026-08-25T00:00:00Z",
            ),
            Err(RemediationError::PrecisionGateNotMet { .. })
        ));
        assert!(matches!(
            seal_actionable(
                &proposal,
                &BTreeSet::new(),
                Some(0.95),
                "requires_human_review",
                "proposal only",
                "policy-v1",
                "redaction-v1",
                None,
                evidence.clone(),
                "adapt",
                "2026-08-25T00:00:00Z",
            ),
            Err(RemediationError::MissingUserEvidenceForTasteCandidate)
        ));
        assert!(seal_actionable(
            &proposal,
            &selected,
            Some(0.95),
            "requires_human_review",
            "proposal only",
            "policy-v1",
            "redaction-v1",
            None,
            evidence,
            "adapt",
            "2026-08-25T00:00:00Z",
        )
        .is_ok());
    }

    #[test]
    fn remediation_semantics_are_sealed() {
        let issue = format!("ii_{}", "a".repeat(64));
        let proposal = RemediationProposalV1::build(
            &issue,
            "repeated_ask",
            RemediationEffect::ProcessChange,
            "Require checklist verification",
            vec![],
        );
        let mut sealed = SealedRemediationProposalV1::seal(
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
        .unwrap();
        assert!(sealed.verify().is_ok());
        sealed.payload.canonical_proposal_text.push_str(" changed");
        assert_eq!(
            sealed.verify(),
            Err(RemediationSealError::PayloadDigestMismatch)
        );
    }
}
