//! Blueprint V1 wire contracts.
//!
//! These types intentionally mirror the language-neutral JSON schemas.  Open
//! extension points remain open maps; required fields remain required so a
//! partial or invented status cannot deserialize as success.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub type OpenMap = BTreeMap<String, Value>;

/// Source-bound candidate emitted by Blueprint Recall/Resolve.
///
/// This is Blueprint's native wire representation of the supported
/// `CandidateV1` JSON shape.  Keeping it here makes Blueprint the owner of
/// graph-to-candidate semantics while federation remains a lossless consumer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlueprintCandidateV1 {
    pub id: String,
    pub layer: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub source_kind: String,
    pub source_ref: String,
    pub source_hash: String,
    pub trust_class: String,
    pub instruction_policy: String,
    pub provider_score: f64,
    #[serde(default)]
    pub score_components: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    pub estimated_tokens: u32,
    pub protected: bool,
    pub exact: bool,
    pub recoverable: bool,
    pub resolver: String,
    pub text: String,
}

impl BlueprintCandidateV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.id.is_empty()
            || self.source_kind.is_empty()
            || self.source_ref.is_empty()
            || self.source_hash.is_empty()
            || self.trust_class.is_empty()
            || self.instruction_policy.is_empty()
            || self.resolver.is_empty()
        {
            return Err(ContractError::MissingRequiredField);
        }
        if !(0.0..=1.0).contains(&self.provider_score) {
            return Err(ContractError::InvalidValue);
        }
        Ok(())
    }
}

/// Recall/Resolve candidate container. `state`, coverage and omissions are
/// producer facts: federation must preserve them, never infer a complete
/// result from a non-empty candidate array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlueprintCandidateSetV1 {
    pub schema_version: u32,
    pub state: String,
    pub candidates: Vec<BlueprintCandidateV1>,
    pub candidate_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_known_count: Option<u64>,
    pub truncated: bool,
    pub coverage: String,
    pub freshness: String,
    #[serde(default)]
    pub omissions: Vec<Value>,
}

impl BlueprintCandidateSetV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != 1 {
            return Err(ContractError::UnsupportedSchema);
        }
        if self.state.is_empty() || self.coverage.is_empty() || self.freshness.is_empty()
            || self.candidate_count != self.candidates.len() as u64
        {
            return Err(ContractError::InvalidValue);
        }
        self.candidates.iter().try_for_each(BlueprintCandidateV1::validate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeGrantV1 {
    pub task_id: String,
    pub repo_root: String,
    #[serde(deserialize_with = "deserialize_present_optional")]
    pub generation_id: Option<String>,
    pub receipt_id: String,
    pub paths: Vec<String>,
    pub issued_ms: u64,
    pub ttl_ms: u64,
    pub signature: String,
}

fn deserialize_present_optional<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

impl ScopeGrantV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.task_id.is_empty() || self.repo_root.is_empty() || self.receipt_id.is_empty() {
            return Err(ContractError::MissingRequiredField);
        }
        if self.paths.is_empty() || self.paths.iter().any(|path| path.is_empty()) || self.ttl_ms == 0 {
            return Err(ContractError::InvalidValue);
        }
        if self.signature.len() != 64 || !self.signature.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
            return Err(ContractError::InvalidSignature);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryManifestV1 {
    pub schema_version: u32,
    pub generation_id: String,
    pub manifest_digest: String,
    pub provider: String,
    pub complete: bool,
    pub repo: String,
    pub counts: OpenMap,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<OpenMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<OpenMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<OpenMap>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryStatusV1 {
    pub schema_version: u32,
    pub state: String,
    pub artifacts: OpenMap,
    pub stats: OpenMap,
    pub errors: Vec<OpenMap>,
    pub warnings: Vec<OpenMap>,
    pub reasons: Vec<String>,
    pub capabilities: OpenMap,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<OpenMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<OpenMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<OpenMap>,
}

impl RepositoryStatusV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != 1 { return Err(ContractError::UnsupportedSchema); }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarrierResult { CaughtUp, GapBlocked, Timeout }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FreshnessReceiptV1 {
    pub receipt_id: String,
    pub created_ms: u64,
    pub repo_root: String,
    pub generation_id: String,
    pub source_clock: u64,
    pub applied_clock: u64,
    pub event_gap: bool,
    pub barrier_result: BarrierResult,
    pub details: OpenMap,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<OpenMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<OpenMap>,
}

/// Bounded task/symbol/file/node orientation section (BM03).
///
/// Every Architecture orientation section (anchors, source, component/owner,
/// callers/callees, tests, impact, config, governing claims, derived
/// constraints) reports one of these dispositions plus counts, so an empty
/// evaluated result is distinguishable from a section that was never run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionDisposition {
    Evaluated,
    Partial,
    Unavailable,
    NotEvaluated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrientationSectionV1 {
    pub disposition: SectionDisposition,
    pub returned_count: u64,
    /// `None` means the total is not known (must be stated explicitly, never
    /// inferred as zero); `Some(0)` means a known-empty evaluated result.
    #[serde(default)]
    pub total_known_count: Option<u64>,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub items: Vec<Value>,
}

impl OrientationSectionV1 {
    pub fn evaluated(items: Vec<Value>, total_known_count: Option<u64>, truncated: bool) -> Self {
        Self { disposition: SectionDisposition::Evaluated, returned_count: items.len() as u64, total_known_count, truncated, reason: None, items }
    }
    /// An empty but *performed* evaluation: distinct from `not_evaluated`.
    pub fn empty_evaluated() -> Self {
        Self { disposition: SectionDisposition::Evaluated, returned_count: 0, total_known_count: Some(0), truncated: false, reason: None, items: Vec::new() }
    }
    pub fn partial(items: Vec<Value>, total_known_count: Option<u64>, reason: impl Into<String>) -> Self {
        Self { disposition: SectionDisposition::Partial, returned_count: items.len() as u64, total_known_count, truncated: true, reason: Some(reason.into()), items }
    }
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self { disposition: SectionDisposition::Unavailable, returned_count: 0, total_known_count: None, truncated: false, reason: Some(reason.into()), items: Vec::new() }
    }
    /// The section was never run (e.g. unimplemented) — never render this as
    /// an empty-complete evaluated section.
    pub fn not_evaluated(reason: impl Into<String>) -> Self {
        Self { disposition: SectionDisposition::NotEvaluated, returned_count: 0, total_known_count: None, truncated: false, reason: Some(reason.into()), items: Vec::new() }
    }
    pub fn validate(&self) -> Result<(), ContractError> {
        match self.disposition {
            SectionDisposition::NotEvaluated | SectionDisposition::Unavailable if self.reason.is_none() => Err(ContractError::MissingRequiredField),
            SectionDisposition::Partial if self.reason.is_none() => Err(ContractError::MissingRequiredField),
            SectionDisposition::Evaluated if self.returned_count == 0 && self.total_known_count.is_none() => Err(ContractError::InvalidValue),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError { MissingRequiredField, InvalidValue, InvalidSignature, UnsupportedSchema }

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{self:?}") }
}
impl std::error::Error for ContractError {}
