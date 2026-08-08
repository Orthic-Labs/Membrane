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
