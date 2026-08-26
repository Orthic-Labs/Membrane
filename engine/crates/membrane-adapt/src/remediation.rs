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

#[derive(Debug, Clone, PartialEq)]
pub enum RemediationError {
    MissingUserEvidenceForTasteCandidate,
    PrecisionGateNotMet { measured: f64, required: f64 },
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
        }
    }
}

impl std::error::Error for RemediationError {}

pub const PRECISION_GATE_THRESHOLD: f64 = 0.95;

pub const SEALED_REMEDIATION_SCHEMA: &str = "1.0.0";

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
    InvalidIssueId,
    EmptyProposal,
    InvalidUserEvidence,
    UnexpectedUserEvidence,
    InvalidValidatorReceipt,
    PayloadDigestMismatch,
}

fn proposal_kind(effect: RemediationEffect) -> &'static str {
    match effect {
        RemediationEffect::ProcessChange => "workflow_change_proposal",
        RemediationEffect::GuardrailAddition => "guard_proposal",
        RemediationEffect::DocumentationUpdate => "review_warning",
        RemediationEffect::ToolingFix => "evaluator_proposal",
        RemediationEffect::TasteCandidate => "taste_candidate",
    }
}

fn valid_prefixed_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit()))
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
        let issue_suffix = proposal
            .issue_id
            .strip_prefix("ii_")
            .ok_or(RemediationSealError::InvalidIssueId)?;
        if issue_suffix.len() != 64 || !issue_suffix.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(RemediationSealError::InvalidIssueId);
        }
        if proposal.text.trim().is_empty()
            || effect_boundary.trim().is_empty()
            || honesty_limit.trim().is_empty()
            || admission_policy_version.trim().is_empty()
        {
            return Err(RemediationSealError::EmptyProposal);
        }
        if let Some(receipt) = semantic_validator_receipt_id {
            let suffix = receipt
                .strip_prefix("rcpt_")
                .ok_or(RemediationSealError::InvalidValidatorReceipt)?;
            if suffix.len() != 32 || !suffix.chars().all(|c| c.is_ascii_hexdigit()) {
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
            proposal_kind: proposal_kind(proposal.effect).into(),
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
        let id_src = format!("{issue_id}\u{0}{effect:?}\u{0}{text}");
        let id = format!("rem_{}", crate::canonical::sha256_hex(id_src.as_bytes()));
        Self {
            proposal_id: id,
            issue_id: issue_id.to_string(),
            effect,
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
