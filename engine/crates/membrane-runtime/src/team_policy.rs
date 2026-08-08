//! Fail-closed admission seam for MBR-1007; no transport or storage.
use membrane_protocol::{
    TeamPolicyReceiptV1, TeamPolicySyncV1, TeamSyncOptInV1, TEAM_POLICY_SCHEMA_VERSION,
};

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
pub enum TeamPolicyAdmissionReason {
    Accepted,
    InvalidBounds,
    Replay,
    UntrustedEncryption,
    Unauthorized,
    UserScopeBroadened,
    /// This installation has not explicitly opted in to team sync (or the opt-in record itself
    /// is malformed). Team sync is off by default; nothing admits until a local caller enables it.
    SyncNotOptedIn,
    /// The policy's tenant/team does not match the tenant/team this installation opted in to.
    /// Opting in to one team can never be broadened into admitting another team's policy.
    TenantOrTeamMismatch,
    /// The envelope's `key_id` does not match the key this installation opted in with. A key
    /// rotation on the sending side must be paired with an explicit local re-opt-in; it is never
    /// silently followed.
    KeyMismatch,
}

impl TeamPolicyAdmissionReason {
    fn code(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::InvalidBounds => "invalid_bounds",
            Self::Replay => "replay",
            Self::UntrustedEncryption => "untrusted_encryption",
            Self::Unauthorized => "unauthorized",
            Self::UserScopeBroadened => "user_scope_broadened",
            Self::SyncNotOptedIn => "sync_not_opted_in",
            Self::TenantOrTeamMismatch => "tenant_or_team_mismatch",
            Self::KeyMismatch => "key_mismatch",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamPolicyAdmission {
    pub receipt: TeamPolicyReceiptV1,
    pub reason: TeamPolicyAdmissionReason,
}

pub fn admit_team_policy(
    policy: &TeamPolicySyncV1,
    verifier: &dyn TeamPolicyTrustVerifier,
) -> TeamPolicyAdmission {
    let reason = if !policy.has_valid_bounds() {
        TeamPolicyAdmissionReason::InvalidBounds
    } else {
        let verification = verifier.verify(policy);
        if policy.generation <= verification.current_generation {
            TeamPolicyAdmissionReason::Replay
        } else if !verification.encrypted {
            TeamPolicyAdmissionReason::UntrustedEncryption
        } else if !verification.authorized {
            TeamPolicyAdmissionReason::Unauthorized
        } else if !verification.user_origin_learning_scope_preserved {
            TeamPolicyAdmissionReason::UserScopeBroadened
        } else {
            TeamPolicyAdmissionReason::Accepted
        }
    };
    let admitted = reason == TeamPolicyAdmissionReason::Accepted;
    TeamPolicyAdmission {
        reason,
        receipt: TeamPolicyReceiptV1 {
            schema_version: TEAM_POLICY_SCHEMA_VERSION,
            policy_id: policy.policy_id.clone(),
            tenant_id: policy.tenant_id.clone(),
            team_id: policy.team_id.clone(),
            generation: policy.generation,
            envelope_id: policy.envelope.envelope_id.clone(),
            ciphertext_sha256: policy.envelope.ciphertext_sha256.clone(),
            admitted,
            reason: reason.code().into(),
        },
    }
}

fn rejection_receipt(
    policy: &TeamPolicySyncV1,
    reason: TeamPolicyAdmissionReason,
) -> TeamPolicyAdmission {
    TeamPolicyAdmission {
        reason,
        receipt: TeamPolicyReceiptV1 {
            schema_version: TEAM_POLICY_SCHEMA_VERSION,
            policy_id: policy.policy_id.clone(),
            tenant_id: policy.tenant_id.clone(),
            team_id: policy.team_id.clone(),
            generation: policy.generation,
            envelope_id: policy.envelope.envelope_id.clone(),
            ciphertext_sha256: policy.envelope.ciphertext_sha256.clone(),
            admitted: false,
            reason: reason.code().into(),
        },
    }
}

/// The only entry point that should ever be wired to a real transport (MBR-1007). It adds three
/// fail-closed gates in front of [`admit_team_policy`], each checked *before* any trusted
/// encryption/authorization verification runs, so a missing or mismatched local opt-in never
/// even reaches the crypto/authority path:
///
/// 1. **Opt-in.** `opt_in.enabled` must be `true` and structurally valid
///    ([`TeamSyncOptInV1::has_valid_bounds`]). Team sync ships off; nothing this function does
///    can turn it on. [`TeamPolicyAdmissionReason::SyncNotOptedIn`] otherwise.
/// 2. **Scope.** The policy's `tenant_id`/`team_id` must equal the opted-in tenant/team.
///    [`TeamPolicyAdmissionReason::TenantOrTeamMismatch`] otherwise.
/// 3. **Key.** The envelope's `key_id` must equal the opted-in `key_id`. A rotated key on the
///    sending side is only followed after an explicit local re-opt-in.
///    [`TeamPolicyAdmissionReason::KeyMismatch`] otherwise.
///
/// Only once all three pass does this delegate to [`admit_team_policy`], which still owns bounds
/// validation, replay-by-generation, trusted encryption/authorization, and user-origin learning
/// scope preservation. None of those existing guarantees are weakened or bypassed here.
pub fn admit_team_policy_with_opt_in(
    policy: &TeamPolicySyncV1,
    opt_in: &TeamSyncOptInV1,
    verifier: &dyn TeamPolicyTrustVerifier,
) -> TeamPolicyAdmission {
    if !opt_in.enabled || !opt_in.has_valid_bounds() {
        return rejection_receipt(policy, TeamPolicyAdmissionReason::SyncNotOptedIn);
    }
    if opt_in.tenant_id != policy.tenant_id || opt_in.team_id != policy.team_id {
        return rejection_receipt(policy, TeamPolicyAdmissionReason::TenantOrTeamMismatch);
    }
    if opt_in.key_id != policy.envelope.key_id {
        return rejection_receipt(policy, TeamPolicyAdmissionReason::KeyMismatch);
    }
    admit_team_policy(policy, verifier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::{EncryptedReplicationEnvelopeV1, TeamPolicyScopeV1};

    fn envelope(key_id: &str) -> EncryptedReplicationEnvelopeV1 {
        EncryptedReplicationEnvelopeV1 {
            envelope_id: "env-1".into(),
            tenant_id: "tenant-1".into(),
            team_id: "team-1".into(),
            generation: 5,
            ciphertext_sha256: format!("sha256:{}", "a".repeat(64)),
            key_id: key_id.into(),
            replay_nonce: "0123456789abcdef".into(),
        }
    }

    fn policy(generation: u64, key_id: &str) -> TeamPolicySyncV1 {
        TeamPolicySyncV1 {
            schema_version: TEAM_POLICY_SCHEMA_VERSION,
            policy_id: "policy-1".into(),
            tenant_id: "tenant-1".into(),
            team_id: "team-1".into(),
            user_id: "user-1".into(),
            generation,
            scopes: vec![
                TeamPolicyScopeV1::Tenant,
                TeamPolicyScopeV1::Team,
                TeamPolicyScopeV1::User,
            ],
            admin_policy_sha256: format!("sha256:{}", "b".repeat(64)),
            offboarded_user_ids: vec![],
            key_rotation_id: "rot-1".into(),
            audit_export_id: "audit-1".into(),
            envelope: EncryptedReplicationEnvelopeV1 {
                generation,
                ..envelope(key_id)
            },
        }
    }

    fn opted_in(key_id: &str) -> TeamSyncOptInV1 {
        TeamSyncOptInV1 {
            schema_version: TEAM_POLICY_SCHEMA_VERSION,
            tenant_id: "tenant-1".into(),
            team_id: "team-1".into(),
            key_id: key_id.into(),
            enabled: true,
            enabled_at: "2026-08-08T00:00:00Z".into(),
        }
    }

    struct TrustedVerifier;
    impl TeamPolicyTrustVerifier for TrustedVerifier {
        fn verify(&self, _policy: &TeamPolicySyncV1) -> TrustedPolicyVerification {
            TrustedPolicyVerification {
                encrypted: true,
                authorized: true,
                user_origin_learning_scope_preserved: true,
                current_generation: 0,
            }
        }
    }

    struct UntrustedVerifier;
    impl TeamPolicyTrustVerifier for UntrustedVerifier {
        fn verify(&self, _policy: &TeamPolicySyncV1) -> TrustedPolicyVerification {
            TrustedPolicyVerification {
                encrypted: false,
                authorized: false,
                user_origin_learning_scope_preserved: true,
                current_generation: 0,
            }
        }
    }

    #[test]
    fn default_off_installation_never_admits_even_a_perfectly_trusted_policy() {
        let admission = admit_team_policy_with_opt_in(
            &policy(1, "key-1"),
            &TeamSyncOptInV1::disabled(),
            &TrustedVerifier,
        );
        assert_eq!(admission.reason, TeamPolicyAdmissionReason::SyncNotOptedIn);
        assert!(!admission.receipt.admitted);
    }

    #[test]
    fn opting_in_to_one_team_never_admits_a_different_team_policy() {
        let mut other_team = policy(1, "key-1");
        other_team.team_id = "team-2".into();
        other_team.envelope.team_id = "team-2".into();
        let admission =
            admit_team_policy_with_opt_in(&other_team, &opted_in("key-1"), &TrustedVerifier);
        assert_eq!(
            admission.reason,
            TeamPolicyAdmissionReason::TenantOrTeamMismatch
        );
        assert!(!admission.receipt.admitted);
    }

    #[test]
    fn a_rotated_key_is_never_silently_followed() {
        let admission = admit_team_policy_with_opt_in(
            &policy(1, "key-rotated"),
            &opted_in("key-1"),
            &TrustedVerifier,
        );
        assert_eq!(admission.reason, TeamPolicyAdmissionReason::KeyMismatch);
        assert!(!admission.receipt.admitted);
    }

    #[test]
    fn opted_in_matching_trusted_policy_is_accepted() {
        let admission = admit_team_policy_with_opt_in(
            &policy(1, "key-1"),
            &opted_in("key-1"),
            &TrustedVerifier,
        );
        assert_eq!(admission.reason, TeamPolicyAdmissionReason::Accepted);
        assert!(admission.receipt.admitted);
    }

    #[test]
    fn opt_in_gate_never_overrides_untrusted_encryption() {
        let admission = admit_team_policy_with_opt_in(
            &policy(1, "key-1"),
            &opted_in("key-1"),
            &UntrustedVerifier,
        );
        assert_eq!(
            admission.reason,
            TeamPolicyAdmissionReason::UntrustedEncryption
        );
        assert!(!admission.receipt.admitted);
    }

    #[test]
    fn malformed_opt_in_record_fails_closed_even_if_marked_enabled() {
        let mut malformed = opted_in("key-1");
        malformed.tenant_id = String::new(); // enabled=true but missing tenant: not well-formed
        let admission =
            admit_team_policy_with_opt_in(&policy(1, "key-1"), &malformed, &TrustedVerifier);
        assert_eq!(admission.reason, TeamPolicyAdmissionReason::SyncNotOptedIn);
    }
}
