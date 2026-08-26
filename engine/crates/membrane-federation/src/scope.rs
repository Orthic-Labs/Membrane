//! Fail-closed ScopeGrant validation and immutable request binding.
//!
//! Grant lookup and signature verification are injected.  This module never
//! opens grant storage, calls HTTP loopback, or invents a default grant.

use membrane_protocol::{ReadPathV1, ScopeGrantV1};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SCOPE_GRANT_SCHEMA_VERSION: u32 = 1;
pub const SCOPE_GRANT_ISSUER: &str = "membrane-gateway";
pub const SCOPE_GRANT_ALGORITHM: &str = "Ed25519";
pub const SCOPE_GRANT_DOMAIN: &str = "rightstudio.scope-grant.v1\0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeGrantBinding {
    pub client: String,
    pub task_id: String,
    pub session_id: String,
    pub repository_id: String,
    pub repository_root: String,
    pub worktree_root: String,
    pub nonce: String,
    pub manifest_digest: Option<String>,
    pub provider_permissions: BTreeSet<String>,
    pub source_permissions: BTreeSet<String>,
    pub action_permissions: BTreeSet<String>,
    pub persistence_permissions: BTreeSet<String>,
    pub egress_permissions: BTreeSet<String>,
    pub policy_epoch: String,
}

impl ScopeGrantBinding {
    pub fn new(
        client: impl Into<String>,
        task_id: impl Into<String>,
        session_id: impl Into<String>,
        repository_id: impl Into<String>,
        repository_root: impl Into<String>,
        worktree_root: impl Into<String>,
        nonce: impl Into<String>,
        manifest_digest: Option<String>,
        policy_epoch: impl Into<String>,
    ) -> Result<Self, ScopeGrantError> {
        let value = Self {
            client: client.into(),
            task_id: task_id.into(),
            session_id: session_id.into(),
            repository_id: repository_id.into(),
            repository_root: repository_root.into(),
            worktree_root: worktree_root.into(),
            nonce: nonce.into(),
            manifest_digest,
            provider_permissions: BTreeSet::new(),
            source_permissions: BTreeSet::new(),
            action_permissions: BTreeSet::new(),
            persistence_permissions: BTreeSet::new(),
            egress_permissions: BTreeSet::new(),
            policy_epoch: policy_epoch.into(),
        };
        value.validate_shape()?;
        Ok(value)
    }

    pub fn validate_shape(&self) -> Result<(), ScopeGrantError> {
        for value in [
            &self.client,
            &self.task_id,
            &self.session_id,
            &self.repository_id,
            &self.repository_root,
            &self.worktree_root,
            &self.nonce,
            &self.policy_epoch,
        ] {
            if value.trim().is_empty() {
                return Err(ScopeGrantError::Malformed("binding identity is empty"));
            }
        }
        Ok(())
    }
}

/// Signature verifier supplied by the owner of the resident key material.
/// A missing verifier is equivalent to a failed verification.
pub trait ScopeGrantVerifier {
    fn verify(&self, signing_bytes: &[u8], signature: &str, key_id: &str) -> bool;
}

pub trait ScopeGrantSource {
    fn lookup(&self, grant_id: &str) -> Result<Option<ScopeGrantV1>, ScopeGrantError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeGrantErrorCode {
    Missing,
    Unavailable,
    Malformed,
    SignatureInvalid,
    Expired,
    Revoked,
    Mismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopeGrantError {
    Missing,
    Unavailable(String),
    Malformed(&'static str),
    SignatureInvalid,
    Expired,
    Revoked,
    Mismatch(&'static str),
}

impl ScopeGrantError {
    pub const fn code(&self) -> ScopeGrantErrorCode {
        match self {
            Self::Missing => ScopeGrantErrorCode::Missing,
            Self::Unavailable(_) => ScopeGrantErrorCode::Unavailable,
            Self::Malformed(_) => ScopeGrantErrorCode::Malformed,
            Self::SignatureInvalid => ScopeGrantErrorCode::SignatureInvalid,
            Self::Expired => ScopeGrantErrorCode::Expired,
            Self::Revoked => ScopeGrantErrorCode::Revoked,
            Self::Mismatch(_) => ScopeGrantErrorCode::Mismatch,
        }
    }
}

impl fmt::Display for ScopeGrantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("scope_grant_missing"),
            Self::Unavailable(detail) => write!(f, "scope_grant_unavailable:{detail}"),
            Self::Malformed(detail) => write!(f, "scope_grant_malformed:{detail}"),
            Self::SignatureInvalid => f.write_str("scope_grant_signature_invalid"),
            Self::Expired => f.write_str("scope_grant_expired"),
            Self::Revoked => f.write_str("scope_grant_revoked"),
            Self::Mismatch(detail) => write!(f, "scope_grant_mismatch:{detail}"),
        }
    }
}

impl std::error::Error for ScopeGrantError {}

/// Narrow, immutable view passed to providers after all grant checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidatedScopeGrantView {
    pub id: String,
    pub repository_id: String,
    pub repository_root: String,
    pub worktree_root: String,
    pub task_id: String,
    pub session_id: String,
    pub client: String,
    pub nonce: String,
    pub manifest_digest: String,
    pub blueprint_generation: String,
    pub blueprint_freshness: String,
    pub read_paths: Vec<ReadPathV1>,
    pub permitted_edge_types: Vec<String>,
    pub provider_permissions: BTreeSet<String>,
    pub source_permissions: BTreeSet<String>,
    pub action_permissions: BTreeSet<String>,
    pub persistence_permissions: BTreeSet<String>,
    pub egress_permissions: BTreeSet<String>,
    pub policy_epoch: String,
}

impl ValidatedScopeGrantView {
    pub fn lookup_and_validate<S: ScopeGrantSource + ?Sized, V: ScopeGrantVerifier + ?Sized>(
        source: &S,
        verifier: &V,
        grant_id: &str,
        binding: &ScopeGrantBinding,
    ) -> Result<Self, ScopeGrantError> {
        let grant = source
            .lookup(grant_id)
            .map_err(|error| ScopeGrantError::Unavailable(error.to_string()))?
            .ok_or(ScopeGrantError::Missing)?;
        Self::validate(&grant, verifier, binding)
    }

    pub fn validate<V: ScopeGrantVerifier + ?Sized>(
        grant: &ScopeGrantV1,
        verifier: &V,
        binding: &ScopeGrantBinding,
    ) -> Result<Self, ScopeGrantError> {
        binding.validate_shape()?;
        validate_schema(grant)?;
        if grant.status == "revoked" {
            return Err(ScopeGrantError::Revoked);
        }
        if grant.status != "active" {
            return Err(ScopeGrantError::Expired);
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ScopeGrantError::Unavailable("clock".to_owned()))?;
        parse_rfc3339_seconds(&grant.issued_at)
            .ok_or(ScopeGrantError::Malformed("issued_at is invalid"))?;
        if parse_rfc3339_seconds(&grant.expires_at)
            .ok_or(ScopeGrantError::Malformed("expires_at is invalid"))?
            <= now.as_secs()
        {
            return Err(ScopeGrantError::Expired);
        }
        if !verifier.verify(&signing_bytes(grant), &grant.signature, &grant.key_id) {
            return Err(ScopeGrantError::SignatureInvalid);
        }
        if grant.client != binding.client
            || grant.task_id != binding.task_id
            || grant.session_id != binding.session_id
            || grant.repository_id != binding.repository_id
            || grant.repository_root != binding.repository_root
        {
            return Err(ScopeGrantError::Mismatch("request identity"));
        }
        if !grant
            .repository_ids
            .iter()
            .any(|id| id == &binding.repository_root)
        {
            return Err(ScopeGrantError::Mismatch("repository root"));
        }
        if let Some(expected) = binding.manifest_digest.as_deref() {
            if grant.manifest_digest != expected {
                return Err(ScopeGrantError::Mismatch("manifest digest"));
            }
        }
        if grant.nonce != binding.nonce {
            return Err(ScopeGrantError::Mismatch("nonce"));
        }
        Ok(Self {
            id: grant.id.clone(),
            repository_id: grant.repository_id.clone(),
            repository_root: grant.repository_root.clone(),
            worktree_root: binding.worktree_root.clone(),
            task_id: grant.task_id.clone(),
            session_id: grant.session_id.clone(),
            client: grant.client.clone(),
            nonce: grant.nonce.clone(),
            manifest_digest: grant.manifest_digest.clone(),
            blueprint_generation: grant.blueprint_generation.clone(),
            blueprint_freshness: grant.blueprint_freshness.clone(),
            read_paths: grant.read_paths.clone(),
            permitted_edge_types: grant.permitted_edge_types.clone(),
            provider_permissions: binding.provider_permissions.clone(),
            source_permissions: binding.source_permissions.clone(),
            action_permissions: binding.action_permissions.clone(),
            persistence_permissions: binding.persistence_permissions.clone(),
            egress_permissions: binding.egress_permissions.clone(),
            policy_epoch: binding.policy_epoch.clone(),
        })
    }

    pub fn permits(&self, provider: &str, source: &str, action: &str) -> bool {
        self.provider_permissions.contains(provider)
            && self.source_permissions.contains(source)
            && self.action_permissions.contains(action)
    }
}

fn validate_schema(grant: &ScopeGrantV1) -> Result<(), ScopeGrantError> {
    if grant.schema_version != SCOPE_GRANT_SCHEMA_VERSION {
        return Err(ScopeGrantError::Malformed("schema_version"));
    }
    if !grant.id.starts_with("sgv1-")
        || grant.issuer != SCOPE_GRANT_ISSUER
        || grant.client.trim().is_empty()
        || grant.repository_ids.is_empty()
        || grant.task_id.trim().is_empty()
        || grant.session_id.trim().is_empty()
        || grant.nonce.len() < 8
        || !grant.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        || grant.signature_algorithm != SCOPE_GRANT_ALGORITHM
    {
        return Err(ScopeGrantError::Malformed("identity or issuer"));
    }
    if !valid_digest(&grant.manifest_digest)
        || !valid_digest(&grant.request_hash)
        || !valid_digest(&grant.context_packet_hash)
    {
        return Err(ScopeGrantError::Malformed("digest"));
    }
    if !grant
        .permitted_edge_types
        .iter()
        .any(|edge| edge == "source_read")
    {
        return Err(ScopeGrantError::Malformed("source_read permission"));
    }
    if grant.repository_root.trim().is_empty()
        || grant.blueprint_generation.trim().is_empty()
        || !matches!(
            grant.blueprint_freshness.as_str(),
            "clean"
                | "dirty_overlay"
                | "stale_snapshot"
                | "missing_snapshot"
                | "partial_reindex"
                | "concurrent_update"
                | "indeterminate"
        )
        || !matches!(grant.cortex_status.as_str(), "available" | "degraded")
        || !valid_key_id(&grant.key_id)
        || !valid_base64ish(&grant.signature)
    {
        return Err(ScopeGrantError::Malformed("required field"));
    }
    if grant.read_paths.iter().any(|path| !valid_read_path(path)) {
        return Err(ScopeGrantError::Malformed("read path"));
    }
    Ok(())
}

fn valid_read_path(path: &ReadPathV1) -> bool {
    !path.path.is_empty()
        && !path.path.starts_with('/')
        && !path.path.contains('\\')
        && !path
            .path
            .split('/')
            .any(|part| part == ".." || part.is_empty())
        && path.start_line > 0
        && path.end_line >= path.start_line
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_key_id(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("scope-grant-ed25519-v1:") else {
        return false;
    };
    hex.len() == 32 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_base64ish(value: &str) -> bool {
    !value.is_empty()
        && value.len() % 4 == 0
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

/// Canonical signing bytes match `mcp/scope-grant-v1.mjs`: domain prefix plus
/// canonical JSON for the grant with `signature` removed.
pub fn signing_bytes(grant: &ScopeGrantV1) -> Vec<u8> {
    let unsigned = UnsignedScopeGrant::from(grant);
    let canonical = membrane_protocol::canonical_json_of(&unsigned);
    [SCOPE_GRANT_DOMAIN.as_bytes(), canonical.as_bytes()].concat()
}

/// Explicit unsigned projection avoids a JSON-map dependency in the
/// federation crate and keeps signing fields closed to the protocol shape.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnsignedScopeGrant<'a> {
    schema_version: u32,
    id: &'a str,
    issuer: &'a str,
    client: &'a str,
    repository_ids: &'a [String],
    permitted_edge_types: &'a [String],
    task_id: &'a str,
    session_id: &'a str,
    issued_at: &'a str,
    expires_at: &'a str,
    status: &'a str,
    nonce: &'a str,
    manifest_digest: &'a str,
    repository_id: &'a str,
    repository_root: &'a str,
    blueprint_generation: &'a str,
    blueprint_freshness: &'a str,
    request_hash: &'a str,
    context_packet_hash: &'a str,
    read_paths: &'a [ReadPathV1],
    source_read_bytes_max: u32,
    unique_files_max: u32,
    results_max: u32,
    cortex_status: &'a str,
    degraded: bool,
    signature_algorithm: &'a str,
    key_id: &'a str,
}

impl<'a> From<&'a ScopeGrantV1> for UnsignedScopeGrant<'a> {
    fn from(grant: &'a ScopeGrantV1) -> Self {
        Self {
            schema_version: grant.schema_version,
            id: &grant.id,
            issuer: &grant.issuer,
            client: &grant.client,
            repository_ids: &grant.repository_ids,
            permitted_edge_types: &grant.permitted_edge_types,
            task_id: &grant.task_id,
            session_id: &grant.session_id,
            issued_at: &grant.issued_at,
            expires_at: &grant.expires_at,
            status: &grant.status,
            nonce: &grant.nonce,
            manifest_digest: &grant.manifest_digest,
            repository_id: &grant.repository_id,
            repository_root: &grant.repository_root,
            blueprint_generation: &grant.blueprint_generation,
            blueprint_freshness: &grant.blueprint_freshness,
            request_hash: &grant.request_hash,
            context_packet_hash: &grant.context_packet_hash,
            read_paths: &grant.read_paths,
            source_read_bytes_max: grant.source_read_bytes_max,
            unique_files_max: grant.unique_files_max,
            results_max: grant.results_max,
            cortex_status: &grant.cortex_status,
            degraded: grant.degraded,
            signature_algorithm: &grant.signature_algorithm,
            key_id: &grant.key_id,
        }
    }
}

fn parse_rfc3339_seconds(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' || !value.ends_with('Z') {
        return None;
    }
    let number = |start: usize, end: usize| value.get(start..end)?.parse::<u64>().ok();
    let year = number(0, 4)? as i64;
    let month = number(5, 7)? as i64;
    let day = number(8, 10)? as i64;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    if !(1..=12).contains(&month) || day == 0 || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    Some((days * 86_400) as u64 + hour * 3_600 + minute * 60 + second.min(59))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}
