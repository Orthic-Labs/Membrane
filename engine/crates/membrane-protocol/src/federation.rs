//! Versioned internal contracts for native Pull federation.
//!
//! These envelopes are additive to the five public protocol shapes.  Provider
//! payloads are kept separate from `ContextCandidateSetV1`; the coordinator
//! stamps provider identity, validates status, and performs the final merge.

use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::time::Duration;

pub const FEDERATION_REQUEST_SCHEMA_VERSION: u32 = 1;
pub const PROVIDER_OUTPUT_SCHEMA_VERSION: u32 = 1;
pub const FEDERATION_RESPONSE_SCHEMA_VERSION: u32 = 1;

fn deserialize_request_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let version = u32::deserialize(deserializer)?;
    (version == FEDERATION_REQUEST_SCHEMA_VERSION)
        .then_some(version)
        .ok_or_else(|| D::Error::custom("unsupported federation request schema version"))
}

fn deserialize_provider_output_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let version = u32::deserialize(deserializer)?;
    (version == PROVIDER_OUTPUT_SCHEMA_VERSION)
        .then_some(version)
        .ok_or_else(|| D::Error::custom("unsupported provider output schema version"))
}

fn deserialize_response_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let version = u32::deserialize(deserializer)?;
    (version == FEDERATION_RESPONSE_SCHEMA_VERSION)
        .then_some(version)
        .ok_or_else(|| D::Error::custom("unsupported federation response schema version"))
}

/// The only provider set admitted by federation V1.  Array order is the
/// canonical merge order and is independent of completion order.
pub const PROVIDER_ORDER: [&str; 9] = [
    "anchors",
    "blueprint",
    "rules",
    "live_files",
    "git",
    "audit",
    "architect",
    "skills",
    "cortex",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Anchors,
    Blueprint,
    Rules,
    LiveFiles,
    Git,
    Audit,
    Architect,
    Skills,
    Cortex,
}

impl ProviderId {
    pub const ALL: [Self; 9] = [
        Self::Anchors,
        Self::Blueprint,
        Self::Rules,
        Self::LiveFiles,
        Self::Git,
        Self::Audit,
        Self::Architect,
        Self::Skills,
        Self::Cortex,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anchors => "anchors",
            Self::Blueprint => "blueprint",
            Self::Rules => "rules",
            Self::LiveFiles => "live_files",
            Self::Git => "git",
            Self::Audit => "audit",
            Self::Architect => "architect",
            Self::Skills => "skills",
            Self::Cortex => "cortex",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.as_str() == value)
    }

    pub fn rank(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(usize::MAX)
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationProviderStatusV1 {
    Complete,
    Partial,
    Failed,
    Cancelled,
}

impl FederationProviderStatusV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationStatus {
    Complete,
    Partial,
    Failed,
    Cancelled,
}

impl FederationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Stable machine-readable reasons.  Human detail is optional and must never
/// be used for branching or canonical ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    InvalidRequest,
    InvalidRoot,
    ReleaseGenerationMismatch,
    ScopeGrantMissing,
    ScopeGrantInvalid,
    FreshnessUnavailable,
    ProviderUnavailable,
    ProviderFailed,
    ProviderTimeout,
    ProviderCancelled,
    ProviderMalformed,
    GenerationIncoherent,
    CandidateIdentityConflict,
    DeadlineExhausted,
    Cancelled,
    Internal,
}

impl ReasonCode {
    pub const ALL: [Self; 16] = [
        Self::InvalidRequest,
        Self::InvalidRoot,
        Self::ReleaseGenerationMismatch,
        Self::ScopeGrantMissing,
        Self::ScopeGrantInvalid,
        Self::FreshnessUnavailable,
        Self::ProviderUnavailable,
        Self::ProviderFailed,
        Self::ProviderTimeout,
        Self::ProviderCancelled,
        Self::ProviderMalformed,
        Self::GenerationIncoherent,
        Self::CandidateIdentityConflict,
        Self::DeadlineExhausted,
        Self::Cancelled,
        Self::Internal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidRoot => "invalid_root",
            Self::ReleaseGenerationMismatch => "release_generation_mismatch",
            Self::ScopeGrantMissing => "scope_grant_missing",
            Self::ScopeGrantInvalid => "scope_grant_invalid",
            Self::FreshnessUnavailable => "freshness_unavailable",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ProviderFailed => "provider_failed",
            Self::ProviderTimeout => "provider_timeout",
            Self::ProviderCancelled => "provider_cancelled",
            Self::ProviderMalformed => "provider_malformed",
            Self::GenerationIncoherent => "generation_incoherent",
            Self::CandidateIdentityConflict => "candidate_identity_conflict",
            Self::DeadlineExhausted => "deadline_exhausted",
            Self::Cancelled => "cancelled",
            Self::Internal => "internal",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|reason| reason.as_str() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningSeverity {
    Warning,
    Blocker,
}

impl WarningSeverity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Blocker => "blocker",
        }
    }
}

/// Serialized request boundary. `deadline_ms` is a relative budget; no
/// process-local timing primitive is serialized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FederationRequestV1 {
    #[serde(deserialize_with = "deserialize_request_version")]
    pub schema_version: u32,
    pub request_id: String,
    #[serde(default)]
    pub trace_id: String,
    pub task: String,
    #[serde(alias = "repoRoot", alias = "repo")]
    pub repository_root: String,
    pub client: String,
    #[serde(alias = "session")]
    pub session_id: String,
    pub deadline_ms: u64,
    pub max_tokens: u32,
    #[serde(default)]
    pub anchors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_grant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blueprint_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills_generation: Option<String>,
    /// Request envelopes are explicitly extensible for compatible additions.
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

impl FederationRequestV1 {
    pub fn validate(&self) -> Result<(), FederationValidationError> {
        if self.schema_version != FEDERATION_REQUEST_SCHEMA_VERSION {
            return Err(FederationValidationError::SchemaVersion);
        }
        for (name, value) in [
            ("requestId", self.request_id.as_str()),
            ("task", self.task.as_str()),
            ("repositoryRoot", self.repository_root.as_str()),
            ("client", self.client.as_str()),
            ("sessionId", self.session_id.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(FederationValidationError::MissingField(name));
            }
        }
        if !is_absolute_root(&self.repository_root) {
            return Err(FederationValidationError::InvalidRoot);
        }
        if self.deadline_ms == 0 || self.max_tokens == 0 {
            return Err(FederationValidationError::InvalidBudget);
        }
        if self.anchors.iter().any(|anchor| anchor.trim().is_empty()) {
            return Err(FederationValidationError::InvalidAnchor);
        }
        if let Some(digest) = self.manifest_digest.as_deref() {
            if !valid_digest(digest) {
                return Err(FederationValidationError::InvalidManifestDigest);
            }
        }
        Ok(())
    }

    pub fn deadline_budget(&self) -> Result<DeadlineBudget, FederationValidationError> {
        self.validate()?;
        Ok(DeadlineBudget::from_millis(self.deadline_ms))
    }
}

fn is_absolute_root(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with('\\')
        || (bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\'))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FederationValidationError {
    #[error("unsupported federation request schema version")]
    SchemaVersion,
    #[error("required federation request field is missing: {0}")]
    MissingField(&'static str),
    #[error("repository root is not absolute")]
    InvalidRoot,
    #[error("deadline and token budgets must be positive")]
    InvalidBudget,
    #[error("anchor must not be empty")]
    InvalidAnchor,
    #[error("manifest digest is not sha256:<64 hex>")]
    InvalidManifestDigest,
    #[error("unsupported provider output schema version")]
    ProviderOutputSchemaVersion,
    #[error("unsupported federation response schema version")]
    FederationResponseSchemaVersion,
}

/// Internal monotonic budget.  It deliberately has no `Serialize` impl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadlineBudget(Duration);

impl DeadlineBudget {
    pub const fn from_millis(milliseconds: u64) -> Self {
        Self(Duration::from_millis(milliseconds))
    }
    pub const fn as_duration(self) -> Duration {
        self.0
    }
    pub fn remaining_after(self, elapsed: Duration) -> Duration {
        self.0.saturating_sub(elapsed)
    }
    pub fn is_exhausted(self, elapsed: Duration) -> bool {
        self.remaining_after(elapsed).is_zero()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FreshnessSnapshotV1 {
    pub graph_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_digest: Option<String>,
    pub stale: bool,
}

/// Provider input is internal and therefore carries a budget rather than a
/// serialized deadline. Source handles are opaque names owned by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderContextV1 {
    pub request_id: String,
    pub trace_id: String,
    pub repository_root: String,
    pub repository_id: String,
    pub task: String,
    pub session_id: String,
    pub client: String,
    pub anchors: Vec<String>,
    pub scope_grant_id: Option<String>,
    pub release_generation: Option<String>,
    pub freshness: FreshnessSnapshotV1,
    pub deadline: DeadlineBudget,
    pub cancelled: bool,
    pub sources: BTreeMap<String, String>,
}

/// Federation reuses the canonical public candidate shape.  Provider and
/// generation provenance belong to their enclosing provider output; there is
/// deliberately no second candidate schema.
pub type FederationCandidateV1 = crate::CandidateV1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderWarningV1 {
    pub provider: ProviderId,
    pub reason: ReasonCode,
    pub severity: WarningSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    /// Optional content-free diagnostic detail; never candidate/prompt text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderOmissionV1 {
    pub provider: ProviderId,
    pub reason: ReasonCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderDiagnosticsV1 {
    pub provider: ProviderId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOutputV1 {
    #[serde(deserialize_with = "deserialize_provider_output_version")]
    pub schema_version: u32,
    pub provider: ProviderId,
    pub status: FederationProviderStatusV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    #[serde(default)]
    pub candidates: Vec<FederationCandidateV1>,
    #[serde(default)]
    pub warnings: Vec<ProviderWarningV1>,
    #[serde(default)]
    pub omissions: Vec<ProviderOmissionV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<ProviderDiagnosticsV1>,
    /// Only this envelope is extensible; unknown compatible fields are kept.
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

impl ProviderOutputV1 {
    pub fn validate(&self) -> Result<(), FederationValidationError> {
        if self.schema_version != PROVIDER_OUTPUT_SCHEMA_VERSION {
            return Err(FederationValidationError::ProviderOutputSchemaVersion);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FederationDiagnosticsV1 {
    #[serde(default)]
    pub providers: Vec<ProviderDiagnosticsV1>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FederationErrorV1 {
    pub code: ReasonCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FederationResponseV1 {
    #[serde(deserialize_with = "deserialize_response_version")]
    pub schema_version: u32,
    pub request_id: String,
    pub trace_id: String,
    pub status: FederationStatus,
    #[serde(default)]
    pub providers: Vec<ProviderOutputV1>,
    #[serde(default)]
    pub candidates: Vec<FederationCandidateV1>,
    #[serde(default)]
    pub warnings: Vec<ProviderWarningV1>,
    #[serde(default)]
    pub omissions: Vec<ProviderOmissionV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<FederationDiagnosticsV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FederationErrorV1>,
    /// Response envelopes explicitly retain compatible extension fields.
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

impl FederationResponseV1 {
    pub fn validate(&self) -> Result<(), FederationValidationError> {
        if self.schema_version != FEDERATION_RESPONSE_SCHEMA_VERSION {
            return Err(FederationValidationError::FederationResponseSchemaVersion);
        }
        Ok(())
    }
}

fn compare_provider(left: ProviderId, right: ProviderId) -> Ordering {
    left.rank().cmp(&right.rank())
}

pub fn sort_provider_ids(values: &mut [ProviderId]) {
    values.sort_by(|left, right| compare_provider(*left, *right));
}

fn candidate_provider_rank(value: Option<&str>) -> usize {
    value
        .and_then(ProviderId::parse)
        .map(ProviderId::rank)
        .unwrap_or(usize::MAX)
}

pub fn sort_provider_outputs(values: &mut [ProviderOutputV1]) {
    for value in values.iter_mut() {
        sort_candidates(&mut value.candidates);
        sort_warnings(&mut value.warnings);
        sort_omissions(&mut value.omissions);
    }
    values.sort_by(|left, right| compare_provider(left.provider, right.provider));
}

pub fn sort_candidates(values: &mut [FederationCandidateV1]) {
    values.sort_by(|left, right| {
        candidate_provider_rank(left.provider.as_deref())
            .cmp(&candidate_provider_rank(right.provider.as_deref()))
            .then_with(|| right.provider_score.total_cmp(&left.provider_score))
            .then_with(|| right.protected.cmp(&left.protected))
            .then_with(|| right.exact.cmp(&left.exact))
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.source_hash.cmp(&right.source_hash))
            .then_with(|| left.source_ref.cmp(&right.source_ref))
            .then_with(|| left.text.cmp(&right.text))
            .then_with(|| {
                crate::canonical::canonical_json_of(left)
                    .cmp(&crate::canonical::canonical_json_of(right))
            })
    });
}

pub fn sort_warnings(values: &mut [ProviderWarningV1]) {
    values.sort_by(|left, right| {
        compare_provider(left.provider, right.provider)
            .then_with(|| left.reason.as_str().cmp(right.reason.as_str()))
            .then_with(|| left.detail_id.cmp(&right.detail_id))
    });
}

pub fn sort_omissions(values: &mut [ProviderOmissionV1]) {
    values.sort_by(|left, right| {
        compare_provider(left.provider, right.provider)
            .then_with(|| left.reason.as_str().cmp(right.reason.as_str()))
            .then_with(|| left.detail_id.cmp(&right.detail_id))
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    });
}

impl FederationResponseV1 {
    pub fn canonicalize_collections(&mut self) {
        sort_provider_outputs(&mut self.providers);
        sort_candidates(&mut self.candidates);
        sort_warnings(&mut self.warnings);
        sort_omissions(&mut self.omissions);
    }
}

pub type CandidateV1 = FederationCandidateV1;
pub type Candidate = FederationCandidateV1;
pub type FederationCandidate = FederationCandidateV1;
pub type ProviderIdV1 = ProviderId;
pub type ProviderKind = ProviderId;
pub type ProviderKindV1 = ProviderId;
pub type FederationProviderStatus = FederationProviderStatusV1;
pub type ProviderStatusV1 = FederationProviderStatusV1;
pub type ReasonCodeV1 = ReasonCode;
pub type FederationWarningV1 = ProviderWarningV1;
pub type FederationWarning = ProviderWarningV1;
pub type FederationOmissionV1 = ProviderOmissionV1;
pub type FederationOmission = ProviderOmissionV1;
pub type FederationError = FederationErrorV1;
pub type ProviderContext = ProviderContextV1;
pub type ProviderOutput = ProviderOutputV1;
pub type FederationRequest = FederationRequestV1;
pub type FederationResponse = FederationResponseV1;
