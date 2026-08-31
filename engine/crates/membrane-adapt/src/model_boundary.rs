//! Model/proposal boundary (canon §3.4).
//!
//! All model-generated outputs are untrusted proposals. Deterministic code
//! must bind them back to admissible evidence and enforce origin, scope,
//! policy, and write contracts. No model output can manufacture a user source
//! span, a user preference, a permission grant, a verification receipt, a tool
//! result, a policy exception, or a stronger scope than supported evidence.

use serde::{Deserialize, Serialize};

use crate::authority::{Origin, PrecedenceTier};
use crate::record::{InfluenceClass, RecordClass};
use crate::scope::ScopeDimensions;
use crate::seal::{
    SemanticPayloadV1, ADMISSION_POLICY_VERSION, PROVENANCE_CONTRACT_VERSION,
    REDACTION_CONTRACT_VERSION, SEAL_CONTRACT_VERSION,
};

/// Errors that arise when untrusted model proposals are misused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelProposalError {
    UnboundEvidence,
    AuthorityEscalationAttempted,
    ScopeBeyondEvidence,
    InvalidDeterministicContext,
}

impl std::fmt::Display for ModelProposalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelProposalError::UnboundEvidence => {
                write!(f, "model proposal is not bound to qualifying evidence")
            }
            ModelProposalError::AuthorityEscalationAttempted => {
                write!(
                    f,
                    "model proposal attempted to set authority or permissions"
                )
            }
            ModelProposalError::ScopeBeyondEvidence => {
                write!(f, "model proposal declared scope broader than evidence")
            }
            ModelProposalError::InvalidDeterministicContext => {
                write!(f, "deterministic proposal binding context is invalid")
            }
        }
    }
}

impl std::error::Error for ModelProposalError {}

/// A proposed preference extraction. Carries only text hints plus evidence
/// bindings; there are deliberately NO fields for authority class, signal
/// strength, origin, or permission — the type system makes authority
/// laundering through this path impossible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelExtractionProposal {
    pub proposer_id: String,
    pub rule_text: String,
    pub category_hint: String,
    pub scope_hint: String,
    /// Evidence objects this proposal claims to be bound to. Deterministic
    /// code re-verifies each binding; the model's claim alone proves nothing.
    pub bound_evidence_ids: Vec<String>,
    /// Exact excerpt the rule was extracted from; must hash-match a selected,
    /// external-user transcript event before eligibility evaluation.
    pub bound_evidence_excerpt: String,
}

/// A proposed semantic cluster grouping. Group membership stays a PROPOSAL
/// until deterministic verification or a reviewed semantic-merge receipt
/// accepts it; uncertain grouping must abstain conservatively.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelClusterProposal {
    pub proposer_id: String,
    /// Episode IDs claimed to belong to one recurring issue.
    pub episode_ids: Vec<String>,
    pub proposed_signature: String,
    pub confidence: f64,
}

/// Proposed remediation wording. Text only — effect/authority boundaries are
/// assigned by [`crate::remediation`], never by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRemediationTextProposal {
    pub proposer_id: String,
    pub issue_id: String,
    pub remediation_kind_hint: String,
    pub text: String,
}

/// Trusted-host evidence available to deterministic proposal binding. Model
/// output cannot construct authority, scope, effect, lifecycle, or receipts
/// through this type's evidence fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedModelEvidenceV1 {
    pub event_id: String,
    pub excerpt_sha256: String,
    pub source_evidence_digest: String,
    pub origin: Origin,
    pub scope: String,
    pub scope_dimensions: ScopeDimensions,
}

/// Deterministic host policy applied after model wording is verified against
/// selected evidence. Model hints are intentionally absent from every
/// authority-bearing field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeterministicProposalBindingV1 {
    pub evidence: Vec<VerifiedModelEvidenceV1>,
    pub category: String,
    pub record_class: Option<RecordClass>,
    pub machine_binding: Option<String>,
    pub canonical_pool_sha256: String,
    pub validator_receipt_id: String,
    pub validator_receipt_sha256: String,
}

impl ModelExtractionProposal {
    /// Verify that every claimed evidence binding exists and each excerpt
    /// digest matches a selected user transcript event. Returns the list
    /// of verified binding IDs, or an error. This is where "model says so"
    /// becomes "deterministic code confirmed so" — nowhere else.
    pub fn verify_bindings(
        &self,
        selected_transcript_evidence: &[(String, String)], // (event_id, excerpt_sha256)
    ) -> Result<Vec<String>, ModelProposalError> {
        use crate::canonical::sha256_hex;
        let mut verified = Vec::new();
        for id in &self.bound_evidence_ids {
            let found = selected_transcript_evidence
                .iter()
                .find(|(eid, _)| eid == id);
            let Some((_, digest)) = found else {
                return Err(ModelProposalError::UnboundEvidence);
            };
            if &sha256_hex(self.bound_evidence_excerpt.as_bytes()) != digest {
                return Err(ModelProposalError::UnboundEvidence);
            }
            verified.push(id.clone());
        }
        if verified.is_empty() {
            return Err(ModelProposalError::UnboundEvidence);
        }
        Ok(verified)
    }

    /// Convert verified model wording into sealed semantics using trusted-host
    /// policy only. Category/scope hints never cross this boundary.
    pub fn bind_deterministically(
        &self,
        binding: &DeterministicProposalBindingV1,
    ) -> Result<SemanticPayloadV1, ModelProposalError> {
        use std::collections::{BTreeMap, BTreeSet};

        if self.proposer_id.trim().is_empty()
            || self.rule_text.trim().is_empty()
            || binding.category.trim().is_empty()
            || binding.canonical_pool_sha256.trim().is_empty()
            || binding.validator_receipt_id.trim().is_empty()
            || binding.validator_receipt_sha256.trim().is_empty()
        {
            return Err(ModelProposalError::InvalidDeterministicContext);
        }
        let first_evidence = binding
            .evidence
            .first()
            .ok_or(ModelProposalError::UnboundEvidence)?;
        if first_evidence.scope.trim().is_empty() {
            return Err(ModelProposalError::InvalidDeterministicContext);
        }
        let raw_dimensions = first_evidence
            .scope_dimensions
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        let scope_dimensions = ScopeDimensions::normalize(&raw_dimensions)
            .map_err(|_| ModelProposalError::InvalidDeterministicContext)?;
        let selected = binding
            .evidence
            .iter()
            .map(|value| (value.event_id.clone(), value.excerpt_sha256.clone()))
            .collect::<Vec<_>>();
        let evidence_ids = binding
            .evidence
            .iter()
            .map(|value| value.event_id.as_str())
            .collect::<BTreeSet<_>>();
        if evidence_ids.len() != binding.evidence.len()
            || binding.evidence.iter().any(|value| {
                value.scope != first_evidence.scope
                    || value.scope_dimensions != first_evidence.scope_dimensions
            })
        {
            return Err(ModelProposalError::ScopeBeyondEvidence);
        }
        let verified_ids = self.verify_bindings(&selected)?;
        let unique_ids = verified_ids.iter().collect::<BTreeSet<_>>();
        if unique_ids.len() != verified_ids.len() {
            return Err(ModelProposalError::InvalidDeterministicContext);
        }
        let mut source_evidence_digests = Vec::with_capacity(verified_ids.len());
        for event_id in verified_ids {
            let evidence = binding
                .evidence
                .iter()
                .find(|value| value.event_id == event_id)
                .ok_or(ModelProposalError::UnboundEvidence)?;
            if evidence.source_evidence_digest.trim().is_empty() {
                return Err(ModelProposalError::InvalidDeterministicContext);
            }
            if !crate::authority::evaluate_origin(evidence.origin, &self.bound_evidence_excerpt)
                .admitted
            {
                return Err(ModelProposalError::AuthorityEscalationAttempted);
            }
            source_evidence_digests.push(evidence.source_evidence_digest.clone());
        }
        source_evidence_digests.sort();
        source_evidence_digests.dedup();

        Ok(SemanticPayloadV1 {
            seal_contract_version: SEAL_CONTRACT_VERSION.into(),
            record_kind: "preference".into(),
            category: crate::canonical::normalize_text(&binding.category),
            canonical_text: crate::canonical::normalize_text(&self.rule_text),
            scope: first_evidence.scope.trim().into(),
            scope_dimensions,
            authority_tier: PrecedenceTier::ProvisionalCandidate,
            authority_effect: crate::authority::classify_authority_effect(&self.rule_text),
            influence_class: InfluenceClass::Provisional,
            record_class: binding.record_class,
            machine_binding: binding.machine_binding.clone(),
            source_evidence_digests,
            canonical_pool_sha256: binding.canonical_pool_sha256.clone(),
            admission_policy_version: ADMISSION_POLICY_VERSION.into(),
            validator_receipt_id: binding.validator_receipt_id.clone(),
            validator_receipt_sha256: binding.validator_receipt_sha256.clone(),
            redaction_contract_version: REDACTION_CONTRACT_VERSION.into(),
            provenance_contract_version: PROVENANCE_CONTRACT_VERSION.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::sha256_hex;

    #[test]
    fn proposal_type_has_no_authority_fields() {
        // Serialization of a proposal contains no authority-bearing keys:
        // the type system makes laundering through this path unrepresentable.
        let p = ModelExtractionProposal {
            proposer_id: "m".into(),
            rule_text: "always x".into(),
            category_hint: "workflow".into(),
            scope_hint: "s".into(),
            bound_evidence_ids: vec!["ev".into()],
            bound_evidence_excerpt: "always x".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        for forbidden in [
            "authority",
            "permission",
            "origin",
            "signal_strength",
            "precedence",
        ] {
            assert!(!json.contains(forbidden), "proposal leaked {forbidden}");
        }
    }

    #[test]
    fn unverified_bindings_are_rejected() {
        let p = ModelExtractionProposal {
            proposer_id: "m".into(),
            rule_text: "always run tests".into(),
            category_hint: "verification".into(),
            scope_hint: "s".into(),
            bound_evidence_ids: vec!["ev-fake".into()],
            bound_evidence_excerpt: "always run tests".into(),
        };
        let real: Vec<(String, String)> = vec![("ev-real".into(), sha256_hex(b"some other text"))];
        assert_eq!(
            p.verify_bindings(&real),
            Err(ModelProposalError::UnboundEvidence)
        );
        let matching: Vec<(String, String)> =
            vec![("ev-fake".into(), sha256_hex(b"always run tests"))];
        assert!(p.verify_bindings(&matching).is_ok());
    }
}
