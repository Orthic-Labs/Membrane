//! Fail-closed admission seam for MBR-1007; no transport or storage.
use membrane_protocol::{TeamPolicyReceiptV1, TeamPolicySyncV1, TEAM_POLICY_SCHEMA_VERSION};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedPolicyVerification {
    pub encrypted: bool,
    pub authorized: bool,
    pub user_origin_learning_scope_preserved: bool,
    pub current_generation: u64,
}

pub trait TeamPolicyTrustVerifier {
    fn verify(&self, policy: &TeamPolicySyncV1) -> TrustedPolicyVerification;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamPolicyAdmissionReason { Accepted, InvalidBounds, Replay, UntrustedEncryption, Unauthorized, UserScopeBroadened }

impl TeamPolicyAdmissionReason {
    fn code(self) -> &'static str {
        match self {
            Self::Accepted => "accepted", Self::InvalidBounds => "invalid_bounds", Self::Replay => "replay",
            Self::UntrustedEncryption => "untrusted_encryption", Self::Unauthorized => "unauthorized",
            Self::UserScopeBroadened => "user_scope_broadened",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamPolicyAdmission { pub receipt: TeamPolicyReceiptV1, pub reason: TeamPolicyAdmissionReason }

pub fn admit_team_policy(policy: &TeamPolicySyncV1, verifier: &dyn TeamPolicyTrustVerifier) -> TeamPolicyAdmission {
    let reason = if !policy.has_valid_bounds() { TeamPolicyAdmissionReason::InvalidBounds } else {
        let verification = verifier.verify(policy);
        if policy.generation <= verification.current_generation { TeamPolicyAdmissionReason::Replay }
        else if !verification.encrypted { TeamPolicyAdmissionReason::UntrustedEncryption }
        else if !verification.authorized { TeamPolicyAdmissionReason::Unauthorized }
        else if !verification.user_origin_learning_scope_preserved { TeamPolicyAdmissionReason::UserScopeBroadened }
        else { TeamPolicyAdmissionReason::Accepted }
    };
    let admitted = reason == TeamPolicyAdmissionReason::Accepted;
    TeamPolicyAdmission { reason, receipt: TeamPolicyReceiptV1 {
        schema_version: TEAM_POLICY_SCHEMA_VERSION, policy_id: policy.policy_id.clone(), tenant_id: policy.tenant_id.clone(),
        team_id: policy.team_id.clone(), generation: policy.generation, envelope_id: policy.envelope.envelope_id.clone(),
        ciphertext_sha256: policy.envelope.ciphertext_sha256.clone(), admitted, reason: reason.code().into(),
    }}
}
