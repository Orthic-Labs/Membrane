//! Model/proposal boundary (canon §3.4).
//!
//! All model-generated outputs are untrusted proposals. Deterministic code
//! must bind them back to admissible evidence and enforce origin, scope,
//! policy, and write contracts. No model output can manufacture a user source
//! span, a user preference, a permission grant, a verification receipt, a tool
//! result, a policy exception, or a stronger scope than supported evidence.

use serde::{Deserialize, Serialize};

/// Errors that arise when untrusted model proposals are misused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelProposalError {
    UnboundEvidence,
    AuthorityEscalationAttempted,
    ScopeBeyondEvidence,
}

impl std::fmt::Display for ModelProposalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelProposalError::UnboundEvidence => {
                write!(f, "model proposal is not bound to qualifying evidence")
            }
            ModelProposalError::AuthorityEscalationAttempted => {
                write!(f, "model proposal attempted to set authority or permissions")
            }
            ModelProposalError::ScopeBeyondEvidence => {
                write!(f, "model proposal declared scope broader than evidence")
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
    /// Exact excerpt the rule was extracted from; must hash-match an
    /// authenticated user-act evidence object before eligibility evaluation.
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

impl ModelExtractionProposal {
    /// Verify that every claimed evidence binding exists and each excerpt
    /// digest matches an authenticated user evidence object. Returns the list
    /// of verified binding IDs, or an error. This is where "model says so"
    /// becomes "deterministic code confirmed so" — nowhere else.
    pub fn verify_bindings(
        &self,
        authenticated_evidence: &[(String, String)], // (evidence_id, excerpt_sha256)
    ) -> Result<Vec<String>, ModelProposalError> {
        use crate::canonical::sha256_hex;
        let mut verified = Vec::new();
        for id in &self.bound_evidence_ids {
            let found = authenticated_evidence.iter().find(|(eid, _)| eid == id);
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
        for forbidden in ["authority", "permission", "origin", "signal_strength", "precedence"] {
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
        let real: Vec<(String, String)> =
            vec![("ev-real".into(), sha256_hex(b"some other text"))];
        assert_eq!(
            p.verify_bindings(&real),
            Err(ModelProposalError::UnboundEvidence)
        );
        let matching: Vec<(String, String)> =
            vec![("ev-fake".into(), sha256_hex(b"always run tests"))];
        assert!(p.verify_bindings(&matching).is_ok());
    }
}
