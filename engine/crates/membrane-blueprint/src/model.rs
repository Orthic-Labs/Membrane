//! Native Blueprint request/response model.
//!
//! This module is deliberately storage agnostic.  A one-shot caller and a
//! resident owner use the same value types; neither caller receives a SQLite
//! handle or owns generation publication.

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::path::PathBuf;

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_DEADLINE_MS: u64 = 2_000;
pub const MIN_DEADLINE_MS: u64 = 10;
pub const MAX_DEADLINE_MS: u64 = 30_000;
pub const MAX_BUILD_DEADLINE_MS: u64 = 300_000;
pub const MAX_ONE_SHOT_REQUEST_BYTES: usize = 65_536;
pub const MAX_DAEMON_FRAME_BYTES: usize = 16_384;
pub const MAX_RESPONSE_BYTES: usize = 16_384;
pub const DEFAULT_CANDIDATE_CAP: usize = 64;
pub const MAX_CANDIDATE_CAP: usize = 256;
pub const DEFAULT_PATH_CAP: usize = 256;
pub const MAX_PATH_CAP: usize = 4_096;
pub const MAX_PATH_LENGTH: usize = 4_096;

/// Every operation exposed by the native owner.  The names are the wire
/// vocabulary used by the former Blueprint protocol; unknown names fail
/// closed instead of being interpreted as a successful operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    Init,
    Update,
    Doctor,
    Repair,
    Export,
    Build,
    Refresh,
    Status,
    Search,
    Resolve,
    Recall,
    Expand,
    Impact,
    Path,
    Architecture,
    DocumentTruth,
    SnapshotGet,
    SnapshotList,
    Changes,
    Federate,
    Phase2Plan,
    Phase2Verify,
    Phase2Seal,
    DbOpen,
    DbStatus,
    DbMigrate,
    FreshnessObserve,
    FreshnessBarrier,
    FreshnessReconcile,
    FindingsGet,
    FindingsExplain,
    FindingsEvidencePack,
    FindingsBaselineCapture,
    FindingsBaselineList,
    FindingsSarif,
}

impl Serialize for Operation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer { serializer.serialize_str(self.as_str()) }
}

impl<'de> Deserialize<'de> for Operation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| de::Error::custom(format!("unknown Blueprint operation: {value}")))
    }
}

impl Operation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Init => "init", Self::Update => "update", Self::Doctor => "doctor",
            Self::Repair => "repair", Self::Export => "export", Self::Build => "build",
            Self::Refresh => "refresh", Self::Status => "status", Self::Search => "search",
            Self::Resolve => "resolve", Self::Recall => "recall", Self::Expand => "expand",
            Self::Impact => "impact", Self::Path => "path", Self::Architecture => "architecture",
            Self::DocumentTruth => "documentTruth", Self::SnapshotGet => "snapshot_get",
            Self::SnapshotList => "snapshot_list", Self::Changes => "changes", Self::Federate => "federate",
            Self::Phase2Plan => "phase2_plan", Self::Phase2Verify => "phase2_verify", Self::Phase2Seal => "phase2_seal",
            Self::DbOpen => "db_open", Self::DbStatus => "db_status", Self::DbMigrate => "db_migrate",
            Self::FreshnessObserve => "freshness_observe", Self::FreshnessBarrier => "freshness_barrier",
            Self::FreshnessReconcile => "freshness_reconcile", Self::FindingsGet => "findings.get",
            Self::FindingsExplain => "findings.explain", Self::FindingsEvidencePack => "findings.evidence_pack",
            Self::FindingsBaselineCapture => "findings.baseline.capture", Self::FindingsBaselineList => "findings.baseline.list",
            Self::FindingsSarif => "findings.sarif",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "init" => Some(Self::Init), "update" => Some(Self::Update), "doctor" => Some(Self::Doctor),
            "repair" => Some(Self::Repair), "export" => Some(Self::Export), "build" => Some(Self::Build),
            "refresh" => Some(Self::Refresh), "status" => Some(Self::Status), "search" => Some(Self::Search),
            "resolve" => Some(Self::Resolve), "recall" => Some(Self::Recall), "expand" => Some(Self::Expand),
            "impact" => Some(Self::Impact), "path" => Some(Self::Path), "architecture" => Some(Self::Architecture),
            "documentTruth" | "document_truth" => Some(Self::DocumentTruth), "snapshot_get" => Some(Self::SnapshotGet),
            "snapshot_list" => Some(Self::SnapshotList), "changes" => Some(Self::Changes), "federate" => Some(Self::Federate),
            "phase2_plan" => Some(Self::Phase2Plan), "phase2_verify" => Some(Self::Phase2Verify), "phase2_seal" => Some(Self::Phase2Seal),
            "db_open" => Some(Self::DbOpen), "db_status" => Some(Self::DbStatus), "db_migrate" => Some(Self::DbMigrate),
            "freshness_observe" => Some(Self::FreshnessObserve), "freshness_barrier" => Some(Self::FreshnessBarrier),
            "freshness_reconcile" => Some(Self::FreshnessReconcile), "findings.get" => Some(Self::FindingsGet),
            "findings.explain" => Some(Self::FindingsExplain), "findings.evidence_pack" => Some(Self::FindingsEvidencePack),
            "findings.baseline.capture" => Some(Self::FindingsBaselineCapture), "findings.baseline.list" => Some(Self::FindingsBaselineList),
            "findings.sarif" => Some(Self::FindingsSarif), _ => None,
        }
    }

    pub const fn is_build(self) -> bool { matches!(self, Self::Build) }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryScope {
    pub repo_root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationBinding {
    pub repo_root: PathBuf,
    pub generation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessState { Fresh, ChangedSinceGeneration, Unknown, Unavailable }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FreshnessBinding {
    pub state: FreshnessState,
    pub source_clock: u64,
    pub applied_clock: u64,
    pub event_gap: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestScope {
    pub repository: RepositoryScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<GenerationBinding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub generation_id: String,
    #[serde(default)]
    pub evidence: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphEdge {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub target: Option<String>,
    pub generation_id: String,
    #[serde(default)]
    pub evidence: Vec<Value>,
}

/// BM04 contract: bounded unresolved/targetless impact-frontier classification.
///
/// Traversal MUST carry every edge reached within its declared bounds into
/// classification -- including edges whose `target` is `None` (a dynamic or
/// unresolved call site) -- rather than silently dropping them before a
/// class is assigned. Dropping a `target: None` edge before classification
/// collapses "unresolved dynamic surface" into "not observed", which is a
/// distinct and stronger (false) claim.
///
/// Exact structural resolution of a symbol (an edge whose provenance carries
/// `ConfidenceTier::ExactResolution`) proves only that a structural
/// dependency edge exists. It is never sufficient, on its own, to claim
/// behavioral impact or comprehensive test coverage of the caller/callee
/// relationship; runtime behavior and test reachability are separate claims
/// that this classification does not make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactFrontierClass {
    /// A resolved edge (concrete `target`) backed by exact-resolution
    /// provenance: a known structural dependency, not a proof of behavior.
    KnownStructuralDependency,
    /// A resolved edge backed by lower-confidence (lexical/heuristic)
    /// provenance: a possible, unconfirmed impact.
    PossibleImpact,
    /// An edge with no resolvable `target` (dynamic dispatch, reflection,
    /// unresolved call, or similar): the frontier is real but its
    /// destination is not known. Must still be carried through traversal,
    /// never filtered out before classification.
    UnresolvedDynamicSurface,
    /// No edge reached this frontier within the declared traversal bounds:
    /// absence of evidence, not evidence of absence. Distinct from an empty
    /// classification produced by a bug that dropped edges pre-classification.
    NotObserved,
}

impl ImpactFrontierClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KnownStructuralDependency => "known_structural_dependency",
            Self::PossibleImpact => "possible_impact",
            Self::UnresolvedDynamicSurface => "unresolved_dynamic_surface",
            Self::NotObserved => "not_observed",
        }
    }

    /// Classify one traversed edge into the BM04 frontier taxonomy.
    ///
    /// `confidence_tier` is the edge's provenance confidence tier label as
    /// recorded on its evidence (see [`crate::graph::ConfidenceTier`]); pass
    /// `None` when the edge carries no resolvable provenance tier. This
    /// function must be called for every edge the traversal reaches within
    /// its bounds -- including `target: None` edges -- before any edge is
    /// discarded, so an unresolved/targetless frontier is classified rather
    /// than silently filtered.
    pub fn classify(target: Option<&str>, confidence_tier: Option<&str>) -> Self {
        if target.is_none() {
            return Self::UnresolvedDynamicSurface;
        }
        match confidence_tier {
            Some("EXACT_RESOLUTION") | Some("exact_resolution") => Self::KnownStructuralDependency,
            Some("UNRESOLVED") | Some("unresolved") | None => Self::UnresolvedDynamicSurface,
            Some(_) => Self::PossibleImpact,
        }
    }
}
