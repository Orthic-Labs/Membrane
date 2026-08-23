//! Transport-neutral source contracts used by native federation providers.
//!
//! These traits describe owner APIs only.  They deliberately contain no
//! HTTP, MCP, process, filesystem, or storage implementation.  Composition
//! chooses an in-process or resident implementation and passes it through
//! [`SourceSet`].

use crate::error::{ProviderError, Result};
use async_trait::async_trait;
use membrane_protocol::{CandidateV1, FreshnessSnapshotV1, ScopeGrantV1};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Stable query identity shared by every source lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceQuery {
    pub request_id: String,
    pub repository_id: String,
    pub repository_root: String,
    pub task: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    #[serde(default)]
    pub anchors: Vec<String>,
}

/// Content-free warning returned by a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceWarning {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_id: Option<String>,
}

/// Source response metadata.  Generation and completeness are retained so a
/// provider cannot silently turn stale or partial source data into complete
/// output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceResponse<T> {
    pub value: T,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    pub complete: bool,
    #[serde(default)]
    pub warnings: Vec<SourceWarning>,
}

pub type SourceResult<T> = Result<SourceResponse<T>>;

/// Stable audit finding projection.  The candidate is owner-produced and is
/// not reconstructed from a dynamic import or an untrusted text payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditFinding {
    pub id: String,
    pub repository_id: String,
    pub generation: String,
    pub source_hash: String,
    pub candidate: CandidateV1,
}

/// Stable architecture decision projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionRecord {
    pub id: String,
    pub repository_id: String,
    pub generation: String,
    pub source_hash: String,
    pub rationale: String,
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
}

/// Index-only skill projection.  Full skill text is acquired by an owner API
/// after admission and is not part of this contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillCatalogEntry {
    pub id: String,
    pub repository_id: String,
    pub generation: String,
    pub source_hash: String,
    pub title: String,
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// Memory candidate projection supplied by Cortex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryCandidate {
    pub id: String,
    pub repository_id: String,
    pub generation: String,
    pub source_hash: String,
    pub candidate: CandidateV1,
}

/// A validated, immutable view of the request ScopeGrant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeGrantView {
    pub id: String,
    pub repository_id: String,
    pub repository_root: String,
    pub task_id: String,
    pub session_id: String,
    pub manifest_digest: String,
    pub blueprint_generation: String,
    #[serde(default)]
    pub permitted_edge_types: Vec<String>,
    #[serde(default)]
    pub read_paths: Vec<String>,
}

impl ScopeGrantView {
    pub fn from_grant(grant: &ScopeGrantV1) -> Self {
        Self {
            id: grant.id.clone(),
            repository_id: grant.repository_id.clone(),
            repository_root: grant.repository_root.clone(),
            task_id: grant.task_id.clone(),
            session_id: grant.session_id.clone(),
            manifest_digest: grant.manifest_digest.clone(),
            blueprint_generation: grant.blueprint_generation.clone(),
            permitted_edge_types: grant.permitted_edge_types.clone(),
            read_paths: grant
                .read_paths
                .iter()
                .map(|path| format!("{}:{}-{}", path.path, path.start_line, path.end_line))
                .collect(),
        }
    }
}

/// Typed Blueprint result.  The payload remains owner-defined while its
/// generation is explicit and bound by the caller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlueprintResult {
    pub generation: String,
    #[serde(default)]
    pub candidates: Vec<CandidateV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

#[async_trait]
pub trait AuditFindingSource: Send + Sync {
    async fn findings(&self, query: &SourceQuery) -> SourceResult<Vec<AuditFinding>>;
}

#[async_trait]
pub trait DecisionRecordSource: Send + Sync {
    async fn decisions(&self, query: &SourceQuery) -> SourceResult<Vec<DecisionRecord>>;
}

#[async_trait]
pub trait SkillCatalogSource: Send + Sync {
    async fn skills(&self, query: &SourceQuery) -> SourceResult<Vec<SkillCatalogEntry>>;
}

#[async_trait]
pub trait MemoryCandidateSource: Send + Sync {
    async fn candidates(&self, query: &SourceQuery) -> SourceResult<Vec<MemoryCandidate>>;
}

#[async_trait]
pub trait ScopeGrantSource: Send + Sync {
    async fn grant(&self, query: &SourceQuery) -> SourceResult<ScopeGrantView>;
}

#[async_trait]
pub trait FreshnessSource: Send + Sync {
    async fn freshness(&self, query: &SourceQuery) -> SourceResult<FreshnessSnapshotV1>;
}

#[async_trait]
pub trait BlueprintSource: Send + Sync {
    async fn query(&self, query: &SourceQuery) -> SourceResult<BlueprintResult>;

    async fn resolve_symbol(
        &self,
        query: &SourceQuery,
        symbol: &str,
    ) -> SourceResult<BlueprintResult>;
}

/// All typed sources available to one provider context.  Missing entries are
/// explicit and are reported as [`ProviderError::MissingSource`] by callers.
#[derive(Clone, Default)]
pub struct SourceSet {
    pub audit: Option<Arc<dyn AuditFindingSource>>,
    pub decisions: Option<Arc<dyn DecisionRecordSource>>,
    pub skills: Option<Arc<dyn SkillCatalogSource>>,
    pub memory: Option<Arc<dyn MemoryCandidateSource>>,
    pub scope_grant: Option<Arc<dyn ScopeGrantSource>>,
    pub freshness: Option<Arc<dyn FreshnessSource>>,
    pub blueprint: Option<Arc<dyn BlueprintSource>>,
}

impl std::fmt::Debug for SourceSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceSet")
            .field("audit", &self.audit.is_some())
            .field("decisions", &self.decisions.is_some())
            .field("skills", &self.skills.is_some())
            .field("memory", &self.memory.is_some())
            .field("scope_grant", &self.scope_grant.is_some())
            .field("freshness", &self.freshness.is_some())
            .field("blueprint", &self.blueprint.is_some())
            .finish()
    }
}
