//! Content-free team policy synchronization contract (MBR-1007).
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const TEAM_POLICY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamPolicyScopeV1 { Tenant, Team, User, LocalRoot }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EncryptedReplicationEnvelopeV1 {
    pub envelope_id: String,
    pub tenant_id: String,
    pub team_id: String,
    pub generation: u64,
    pub ciphertext_sha256: String,
    pub key_id: String,
    pub replay_nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TeamPolicySyncV1 {
    pub schema_version: u32,
    pub policy_id: String,
    pub tenant_id: String,
    pub team_id: String,
    pub user_id: String,
    pub generation: u64,
    pub scopes: Vec<TeamPolicyScopeV1>,
    pub admin_policy_sha256: String,
    pub offboarded_user_ids: Vec<String>,
    pub key_rotation_id: String,
    pub audit_export_id: String,
    pub envelope: EncryptedReplicationEnvelopeV1,
}

impl TeamPolicySyncV1 {
    pub fn has_valid_bounds(&self) -> bool {
        self.schema_version == TEAM_POLICY_SCHEMA_VERSION
            && safe_id(&self.policy_id) && safe_id(&self.tenant_id) && safe_id(&self.team_id)
            && safe_id(&self.user_id) && self.generation > 0 && safe_id(&self.key_rotation_id)
            && safe_id(&self.audit_export_id) && valid_digest(&self.admin_policy_sha256)
            && self.envelope.tenant_id == self.tenant_id && self.envelope.team_id == self.team_id
            && self.envelope.generation == self.generation && safe_id(&self.envelope.envelope_id)
            && safe_id(&self.envelope.key_id) && safe_nonce(&self.envelope.replay_nonce)
            && valid_digest(&self.envelope.ciphertext_sha256)
            && self.scopes.len() == 3
            && self.scopes.iter().any(|scope| matches!(scope, TeamPolicyScopeV1::Tenant))
            && self.scopes.iter().any(|scope| matches!(scope, TeamPolicyScopeV1::Team))
            && self.scopes.iter().any(|scope| matches!(scope, TeamPolicyScopeV1::User))
            && !self.scopes.iter().any(|scope| matches!(scope, TeamPolicyScopeV1::LocalRoot))
            && self.offboarded_user_ids.len() <= 1_024
            && self.offboarded_user_ids.iter().all(|id| safe_id(id))
            && self.offboarded_user_ids.iter().map(String::as_str).collect::<BTreeSet<_>>().len() == self.offboarded_user_ids.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TeamPolicyReceiptV1 {
    pub schema_version: u32,
    pub policy_id: String,
    pub tenant_id: String,
    pub team_id: String,
    pub generation: u64,
    pub envelope_id: String,
    pub ciphertext_sha256: String,
    pub admitted: bool,
    pub reason: String,
}

fn safe_id(value: &str) -> bool { !value.is_empty() && value.len() <= 160 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte)) }
fn safe_nonce(value: &str) -> bool { value.len() >= 16 && value.len() <= 160 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte)) }
fn valid_digest(value: &str) -> bool {
    value.len() == 71 && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Local, content-free record of whether team policy sync is enabled on this installation
/// (MBR-1007). Sync is off by default: the only well-formed value with `enabled == false` is
/// [`TeamSyncOptInV1::disabled`], every field empty. Nothing in this type can be produced from
/// wire data — [`TeamPolicySyncV1`] never carries an opt-in flag, so a value that arrived over
/// the wire cannot flip a local installation into synced state. Setting `enabled == true`
/// requires an explicit local caller to construct one with a real tenant/team/key id, which
/// `membrane-runtime`'s admission path then binds every incoming policy against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TeamSyncOptInV1 {
    pub schema_version: u32,
    pub tenant_id: String,
    pub team_id: String,
    pub key_id: String,
    pub enabled: bool,
    pub enabled_at: String,
}

impl TeamSyncOptInV1 {
    /// The default, off-by-default state: no tenant, team, or key bound, sync disabled.
    pub fn disabled() -> Self {
        Self {
            schema_version: TEAM_POLICY_SCHEMA_VERSION,
            tenant_id: String::new(),
            team_id: String::new(),
            key_id: String::new(),
            enabled: false,
            enabled_at: String::new(),
        }
    }

    /// A disabled record must carry no identifiers (nothing left over from a prior opt-in that
    /// could be mistaken for live scope); an enabled record must carry a complete, well-formed
    /// tenant/team/key/timestamp so admission never has to guess a missing field.
    pub fn has_valid_bounds(&self) -> bool {
        if self.schema_version != TEAM_POLICY_SCHEMA_VERSION {
            return false;
        }
        if !self.enabled {
            return self.tenant_id.is_empty()
                && self.team_id.is_empty()
                && self.key_id.is_empty()
                && self.enabled_at.is_empty();
        }
        safe_id(&self.tenant_id)
            && safe_id(&self.team_id)
            && safe_id(&self.key_id)
            && safe_id(&self.enabled_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_envelope() -> EncryptedReplicationEnvelopeV1 {
        EncryptedReplicationEnvelopeV1 {
            envelope_id: "env-1".into(),
            tenant_id: "tenant-1".into(),
            team_id: "team-1".into(),
            generation: 1,
            ciphertext_sha256: format!("sha256:{}", "a".repeat(64)),
            key_id: "key-1".into(),
            replay_nonce: "0123456789abcdef".into(),
        }
    }

    fn valid_policy() -> TeamPolicySyncV1 {
        TeamPolicySyncV1 {
            schema_version: TEAM_POLICY_SCHEMA_VERSION,
            policy_id: "policy-1".into(),
            tenant_id: "tenant-1".into(),
            team_id: "team-1".into(),
            user_id: "user-1".into(),
            generation: 1,
            scopes: vec![
                TeamPolicyScopeV1::Tenant,
                TeamPolicyScopeV1::Team,
                TeamPolicyScopeV1::User,
            ],
            admin_policy_sha256: format!("sha256:{}", "b".repeat(64)),
            offboarded_user_ids: vec!["user-2".into()],
            key_rotation_id: "rot-1".into(),
            audit_export_id: "audit-1".into(),
            envelope: valid_envelope(),
        }
    }

    #[test]
    fn a_well_formed_policy_has_valid_bounds() {
        assert!(valid_policy().has_valid_bounds());
    }

    #[test]
    fn local_root_scope_is_never_valid() {
        let mut policy = valid_policy();
        policy.scopes.push(TeamPolicyScopeV1::LocalRoot);
        assert!(!policy.has_valid_bounds());
    }

    #[test]
    fn duplicate_offboarded_user_ids_are_rejected() {
        let mut policy = valid_policy();
        policy.offboarded_user_ids = vec!["dup".into(), "dup".into()];
        assert!(!policy.has_valid_bounds());
    }

    #[test]
    fn envelope_scope_must_match_the_policy_scope() {
        let mut policy = valid_policy();
        policy.envelope.tenant_id = "other-tenant".into();
        assert!(!policy.has_valid_bounds());
    }

    #[test]
    fn disabled_opt_in_default_has_valid_bounds() {
        assert!(TeamSyncOptInV1::disabled().has_valid_bounds());
    }

    #[test]
    fn a_disabled_opt_in_carrying_leftover_identifiers_is_invalid() {
        let mut opt_in = TeamSyncOptInV1::disabled();
        opt_in.tenant_id = "tenant-1".into();
        assert!(!opt_in.has_valid_bounds());
    }

    #[test]
    fn an_enabled_opt_in_requires_every_identifier() {
        let opt_in = TeamSyncOptInV1 {
            enabled: true,
            tenant_id: "tenant-1".into(),
            team_id: "team-1".into(),
            key_id: "key-1".into(),
            enabled_at: "2026-08-08T00:00:00Z".into(),
            ..TeamSyncOptInV1::disabled()
        };
        assert!(opt_in.has_valid_bounds());

        let mut missing_key = opt_in.clone();
        missing_key.key_id = String::new();
        assert!(!missing_key.has_valid_bounds());
    }
}
