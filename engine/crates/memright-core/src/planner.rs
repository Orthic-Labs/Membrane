//! RightContext planner: deterministic, pure, provider-neutral admission.
//!
//! Per dispatch G3A, this module owns:
//!   - ContextCandidateSet v1 shape parsing and normalization
//!   - Cross-root rejection (trustClass == remote_untrusted)
//!   - Version-authority filtering (superseded/proposed lose unless protected)
//!   - Source-class filters (memory relevance threshold; doc over memory)
//!   - Deduplication by id
//!   - Ranking by (providerScore desc, freshness desc, kind priority, id asc)
//!   - Token-budget admission (sum estimatedTokens <= max_tokens)
//!   - Bound ContextPacket v1 emission with a Budget + per-layer allocations
//!   - Content-free ContextReceipt v2 emission per candidate
//!   - Fallback contract: when freshness.stale=true or provider_capability_missing,
//!     only protected anchors + portable_text_fallback candidates are admitted
//!   - Reason codes on every receipt (admitted, rejected, budget_exhausted,
//!     cross_root, superseded_version, memory_low_relevance,
//!     provider_capability_missing, fallback_quarantine, duplicate_id,
//!     within_global_budget, freshness_authority_outranked)
//!
//! The planner performs no I/O: no filesystem reads, no Blueprint queries, no
//! Git access, no MemRight DB queries. Producers feed it via JSON; consumers
//! receive the packet and the receipts back.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

// ---- Schema mirror ----------------------------------------------------------
//
// These mirror the canonical v1 + v2 definitions in
// tools/lib/context-contracts.schema.json byte-for-byte. The planner rejects
// unknown schemaVersion and rejects v1 receipts that lack planner-observability
// fields when the caller accepts receipt v2.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateV1 {
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
    pub freshness_class: Option<FreshnessClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    pub estimated_tokens: usize,
    pub protected: bool,
    pub exact: bool,
    pub recoverable: bool,
    pub resolver: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmissionV1 {
    pub id: String,
    #[serde(default)]
    pub layer: Option<u8>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCandidateSetV1 {
    pub schema_version: u32,
    pub trace_id: String,
    pub task: String,
    pub mode: String,
    pub provider: String,
    pub freshness: FreshnessV1,
    pub provider_ceiling: ProviderCeilingV1,
    pub candidates: Vec<CandidateV1>,
    pub omissions: Vec<OmissionV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreshnessV1 {
    pub revision: String,
    pub indexed_at: String,
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_state: Option<GraphState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_release_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_release_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_generation_status: Option<ReleaseGenerationStatus>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseGenerationStatus {
    Matched,
    Mismatch,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessClass {
    Current,
    CommittedSnapshot,
    DirtyOverlay,
    StaleSnapshot,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraphState {
    Clean,
    DirtyOverlay,
    StaleSnapshot,
    MissingSnapshot,
    PartialReindex,
    ConcurrentUpdate,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCeilingV1 {
    pub max_candidates: usize,
    pub max_estimated_tokens: usize,
}

// ---- Planner input/output --------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerInput {
    pub candidate_set: ContextCandidateSetV1,
    pub max_tokens: usize,
    /// Optional lower character ceiling selected for the active model. Exact rendered character
    /// enforcement belongs to the client finalizers; the pure planner only carries the contract.
    #[serde(default)]
    pub packet_char_budget_override: Option<usize>,
    #[serde(default)]
    pub packet_char_budget_model: Option<String>,
    /// Caller's accepted receipt versions, e.g. vec![1, 2]. The v2 fields are
    /// always populated; v1 will see them omitted.
    #[serde(default = "default_accepted_versions")]
    pub accepted_receipt_versions: Vec<u32>,
    /// Optional trace id override; defaults to candidate_set.trace_id.
    #[serde(default)]
    pub trace_id_override: Option<String>,
    /// Optional ScopeGrant v1 (server-minted). When present, the planner
    /// validates accepted_receipt_versions matches the grant; when absent the
    /// planner accepts receipt v1+v2 unconditionally.
    #[serde(default)]
    pub scope_grant_present: bool,
}

fn default_accepted_versions() -> Vec<u32> {
    vec![2]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatus {
    Fresh,
    Stale,
    Unavailable,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackMode {
    None,
    NonGraphSourcesOnly,
    PortableTextOnly,
}

impl FallbackMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::NonGraphSourcesOnly => "non_graph_sources_only",
            Self::PortableTextOnly => "portable_text_only",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DegradationReason {
    None,
    BlueprintStale,
    BlueprintUnavailable,
    BlueprintCorrupt,
    ProviderCapabilityMissing,
    ReleaseGenerationMismatch,
    ReleaseGenerationUnavailable,
}

impl DegradationReason {
    fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::BlueprintStale => "blueprint_stale",
            Self::BlueprintUnavailable => "blueprint_unavailable",
            Self::BlueprintCorrupt => "blueprint_corrupt",
            Self::ProviderCapabilityMissing => "provider_capability_missing",
            Self::ReleaseGenerationMismatch => "release_generation_mismatch",
            Self::ReleaseGenerationUnavailable => "release_generation_unavailable",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPacketV1 {
    pub schema_version: u32,
    pub trace_id: String,
    pub task: String,
    pub mode: String,
    pub budget: BudgetV1,
    pub allocations: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider_accounting: BTreeMap<String, ProviderAccountingV1>,
    pub blocks: Vec<BlockV1>,
    pub omissions: Vec<OmissionV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetV1 {
    pub max_tokens: usize,
    pub admitted_tokens: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_char_budget_default: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_char_budget_override: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_char_budget_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_packet_char_budget: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_packet_char_budget: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccountingV1 {
    pub delivery_stage: DeliveryStage,
    pub selected_tokens: usize,
    pub rendered_tokens: usize,
    pub delivered_chars: usize,
    pub drop_reason: DropReason,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStage {
    Planned,
    Finalized,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryClass {
    ResolverBacked,
    MetadataOnly,
    Rendered,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    None,
    MissingResolver,
    TrustGateDropped,
    SealFailed,
    AdditionalContextCharCap,
    PacketBudgetExceeded,
    NotSelected,
    Multiple,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatusScope {
    CandidateSet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockV1 {
    pub id: String,
    pub layer: u8,
    pub provider: String,
    pub source_kind: String,
    pub source_ref: String,
    pub source_hash: String,
    pub trust_class: String,
    pub instruction_policy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_class: Option<FreshnessClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    pub priority: u8,
    pub estimated_tokens: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_stage: Option<DeliveryStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_class: Option<DeliveryClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_tokens: Option<usize>,
    /// Planner-owned transform target. `estimated_tokens` remains original
    /// candidate size; consumers may compress to this bounded allocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allotted_tokens: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered_tokens: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop_reason: Option<DropReason>,
    pub protected: bool,
    pub recoverable: bool,
    pub resolver: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextReceiptV2 {
    pub schema_version: u32,
    pub trace_id: String,
    pub id: String,
    pub layer: u8,
    pub provider: String,
    pub source_kind: String,
    pub source_ref: String,
    pub before_chars: usize,
    pub admitted_chars: usize,
    pub decision: String,
    pub reason: String,
    pub content_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_class: Option<FreshnessClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_stage: Option<DeliveryStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_class: Option<DeliveryClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_tokens: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allotted_tokens: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered_tokens: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop_reason: Option<DropReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_scope: Option<StatusScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_provider: Option<String>,
    pub provider_status: ProviderStatus,
    pub fallback_mode: FallbackMode,
    pub degradation_reason: DegradationReason,
    pub source_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_release_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_release_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_generation_status: Option<ReleaseGenerationStatus>,
}

// ---- Errors ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerError {
    UnknownSchemaVersion {
        found: u32,
        supported: Vec<u32>,
    },
    EmptyTask,
    EmptyProvider,
    ReceiptVersionUnsupported {
        accepted: Vec<u32>,
    },
    /// Caller passed max_tokens = 0 or negative; we surface as 0 for parity with v1.
    ZeroBudget,
}

impl std::fmt::Display for PlannerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlannerError::UnknownSchemaVersion { found, supported } => {
                write!(f, "unknown schemaVersion {found}; supported: {supported:?}")
            }
            PlannerError::EmptyTask => write!(f, "candidate set task is empty"),
            PlannerError::EmptyProvider => write!(f, "candidate set provider is empty"),
            PlannerError::ReceiptVersionUnsupported { accepted } => write!(
                f,
                "caller accepted receipt versions {accepted:?}; planner emits v2 only"
            ),
            PlannerError::ZeroBudget => write!(f, "max_tokens must be >= 1"),
        }
    }
}

impl std::error::Error for PlannerError {}

// ---- Planner output -------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerOutput {
    pub packet: ContextPacketV1,
    pub receipts: Vec<ContextReceiptV2>,
    pub provider_status: ProviderStatus,
    pub fallback_mode: FallbackMode,
    pub degradation_reason: DegradationReason,
    pub source_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_release_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_release_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_generation_status: Option<ReleaseGenerationStatus>,
    pub structured_event: StructuredFallbackEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredFallbackEvent {
    pub event: String,
    pub trace_id: String,
    pub provider: String,
    pub reason: String,
    pub mode: String,
}

// ---- Planner entry --------------------------------------------------------

pub fn plan(input: &PlannerInput) -> Result<PlannerOutput, PlannerError> {
    if input.candidate_set.schema_version != 1 {
        return Err(PlannerError::UnknownSchemaVersion {
            found: input.candidate_set.schema_version,
            supported: vec![1],
        });
    }
    if input.candidate_set.task.trim().is_empty() {
        return Err(PlannerError::EmptyTask);
    }
    if input.candidate_set.provider.trim().is_empty() {
        return Err(PlannerError::EmptyProvider);
    }
    if input.max_tokens == 0 {
        return Err(PlannerError::ZeroBudget);
    }
    if !input.accepted_receipt_versions.contains(&2) {
        return Err(PlannerError::ReceiptVersionUnsupported {
            accepted: input.accepted_receipt_versions.clone(),
        });
    }

    let freshness_stale = input.candidate_set.freshness.stale;
    let graph_degradation_reason = match input.candidate_set.freshness.graph_state {
        Some(GraphState::DirtyOverlay | GraphState::StaleSnapshot | GraphState::PartialReindex) => {
            Some(DegradationReason::BlueprintStale)
        }
        Some(
            GraphState::MissingSnapshot | GraphState::ConcurrentUpdate | GraphState::Indeterminate,
        ) => Some(DegradationReason::BlueprintUnavailable),
        Some(GraphState::Clean) | None => None,
    };
    let release_degradation_reason = release_generation_degradation(&input.candidate_set.freshness);
    let provider_unsupported = input.candidate_set.provider.ends_with("-missing")
        || input.candidate_set.provider.ends_with("-unsupported");
    let fallback_only =
        freshness_stale || provider_unsupported || release_degradation_reason.is_some();
    let (fallback_mode, degradation_reason) = if provider_unsupported {
        (
            FallbackMode::NonGraphSourcesOnly,
            DegradationReason::ProviderCapabilityMissing,
        )
    } else if let Some(reason) = release_degradation_reason.clone() {
        (FallbackMode::PortableTextOnly, reason)
    } else if freshness_stale {
        (
            FallbackMode::PortableTextOnly,
            DegradationReason::BlueprintStale,
        )
    } else {
        (
            FallbackMode::None,
            graph_degradation_reason
                .clone()
                .unwrap_or(DegradationReason::None),
        )
    };
    let provider_status = if provider_unsupported || release_degradation_reason.is_some() {
        ProviderStatus::Unavailable
    } else if freshness_stale {
        ProviderStatus::Stale
    } else if graph_degradation_reason.is_some() {
        ProviderStatus::Degraded
    } else {
        ProviderStatus::Fresh
    };
    let source_generation = if fallback_mode == FallbackMode::None {
        input
            .candidate_set
            .freshness
            .snapshot_id
            .clone()
            .or_else(|| Some(input.candidate_set.freshness.revision.clone()))
    } else {
        None
    };

    let candidates = &input.candidate_set.candidates;
    let mut decisions: Vec<(CandidateV1, String, String)> = Vec::with_capacity(candidates.len());

    // 1. Cross-root rejection (remote_untrusted trust_class).
    let mut kept_after_root: Vec<&CandidateV1> = Vec::new();
    for cand in candidates {
        if cand.trust_class == "remote_untrusted" {
            decisions.push((cand.clone(), "rejected".into(), "cross_root".into()));
            continue;
        }
        kept_after_root.push(cand);
    }

    // 2. Version-authority filtering: IDs ending in ":superseded" or ":proposed"
    //    lose unless protected (e.g. a protected anchor pinning the historical fact).
    let mut kept_after_authority: Vec<&CandidateV1> = Vec::new();
    for cand in kept_after_root {
        if is_version_demoted(cand) && !cand.protected {
            decisions.push((cand.clone(), "rejected".into(), "superseded_version".into()));
            continue;
        }
        kept_after_authority.push(cand);
    }

    // 3. Source-class filters.
    let mut kept_after_class: Vec<&CandidateV1> = Vec::new();
    for cand in kept_after_authority {
        let cap_missing = cand
            .score_components
            .get("provider_capability_missing")
            .copied()
            .unwrap_or(0.0);
        if cap_missing > 0.0 {
            decisions.push((
                cand.clone(),
                "rejected".into(),
                "provider_capability_missing".into(),
            ));
            continue;
        }
        if cand.source_kind == "memory" {
            let structural = cand
                .score_components
                .get("structural")
                .copied()
                .unwrap_or(0.0);
            let lexical = cand.score_components.get("lexical").copied().unwrap_or(0.0);
            if structural <= 0.0 && lexical < 0.85 && !cand.protected {
                decisions.push((
                    cand.clone(),
                    "rejected".into(),
                    "memory_low_relevance".into(),
                ));
                continue;
            }
        }
        kept_after_class.push(cand);
    }

    // 4. Deduplicate by id (keep first occurrence).
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut deduped: Vec<&CandidateV1> = Vec::new();
    for cand in kept_after_class {
        if !seen.insert(cand.id.clone()) {
            decisions.push((cand.clone(), "rejected".into(), "duplicate_id".into()));
            continue;
        }
        deduped.push(cand);
    }

    // 5. Rank by (providerScore desc, freshness desc, kind priority asc,
    //    protected desc, exact desc, id asc).
    deduped.sort_by(|a, b| {
        let af = freshness_component(a);
        let bf = freshness_component(b);
        let ak = kind_priority(a);
        let bk = kind_priority(b);
        b.provider_score
            .partial_cmp(&a.provider_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(bf.partial_cmp(&af).unwrap_or(std::cmp::Ordering::Equal))
            .then(ak.cmp(&bk))
            .then(b.protected.cmp(&a.protected))
            .then(b.exact.cmp(&a.exact))
            .then(a.id.cmp(&b.id))
    });

    // 6. Token-budget admission — TWO PASSES. Pass 1 admits per-source-class RESERVED LANES
    // (by score within the lane); pass 2 fills the remaining budget in global score order
    // (the previous single-pass behavior). Rationale: providers score on incompatible scales —
    // the git overlay carries a flat recency prior (0.6) that structurally outranks memory's
    // real relevance cosine (~0.3-0.5), so on a dirty tree the overlay floods the whole budget
    // and durable memory + skills never admit. Lanes stop the scales competing head-to-head;
    // no score is changed, and lane sizes of 0 restore the exact previous behavior. An unused
    // reservation is simply left for pass 2 (nothing is withheld).
    const RESERVED_LANES: &[(&str, usize)] = &[("memory", 800), ("skill", 300)];
    const MAX_PACKET_BLOCKS: usize = 32;
    let mut admitted_tokens = 0usize;
    let mut admitted: Vec<&CandidateV1> = Vec::new();
    let mut lane_admitted: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (lane_kind, lane_budget) in RESERVED_LANES {
        let mut lane_tokens = 0usize;
        for cand in deduped.iter().filter(|c| c.source_kind == *lane_kind) {
            if admitted.len() >= MAX_PACKET_BLOCKS {
                continue; // pass 2 emits the packet_block_limit receipt
            }
            if fallback_only && !(cand.protected || cand.source_kind == "portable_text_fallback") {
                continue; // pass 2 emits the fallback_quarantine receipt
            }
            let cost = cand.estimated_tokens;
            if lane_tokens + cost > *lane_budget || admitted_tokens + cost > input.max_tokens {
                continue; // over the lane: falls through to pass 2's general fill
            }
            lane_tokens += cost;
            admitted_tokens += cost;
            admitted.push(*cand);
            lane_admitted.insert(cand.id.clone());
            decisions.push((
                (*cand).clone(),
                "admitted".into(),
                "within_reserved_lane".into(),
            ));
        }
    }
    for cand in deduped {
        if lane_admitted.contains(&cand.id) {
            continue;
        }
        let is_anchor = cand.protected;
        let is_portable_fallback = cand.source_kind == "portable_text_fallback";
        if fallback_only && !(is_anchor || is_portable_fallback) {
            decisions.push((
                cand.clone(),
                "rejected".into(),
                "fallback_quarantine".into(),
            ));
            continue;
        }
        if admitted.len() >= MAX_PACKET_BLOCKS {
            decisions.push((cand.clone(), "rejected".into(), "packet_block_limit".into()));
            continue;
        }
        let cost = cand.estimated_tokens;
        if admitted_tokens + cost > input.max_tokens {
            decisions.push((cand.clone(), "rejected".into(), "budget_exhausted".into()));
            continue;
        }
        admitted_tokens += cost;
        admitted.push(cand);
        decisions.push((
            cand.clone(),
            "admitted".into(),
            "within_global_budget".into(),
        ));
    }

    // 7. Build packet blocks.
    let trace_id = input
        .trace_id_override
        .clone()
        .unwrap_or_else(|| input.candidate_set.trace_id.clone());

    let mut allocations: BTreeMap<String, usize> = BTreeMap::new();
    allocations.insert("3".into(), 0);
    allocations.insert("7".into(), 0);
    allocations.insert("other".into(), 0);
    let mut provider_accounting: BTreeMap<String, ProviderAccountingV1> = BTreeMap::new();
    let mut blocks: Vec<BlockV1> = Vec::new();
    let allotments = score_proportional_allotments(&admitted);
    for cand in &admitted {
        let layer_key = match cand.layer {
            3 => "3",
            7 => "7",
            _ => "other",
        };
        allocations
            .entry(layer_key.into())
            .and_modify(|v| *v += cand.estimated_tokens)
            .or_insert(cand.estimated_tokens);
        let provider = candidate_provider(cand, &input.candidate_set.provider);
        let (delivery_class, drop_reason) = planner_delivery_intent(cand);
        provider_accounting
            .entry(provider.clone())
            .and_modify(|accounting| {
                accounting.selected_tokens += cand.estimated_tokens;
                accounting.drop_reason = combine_drop_reasons(accounting.drop_reason, drop_reason);
            })
            .or_insert(ProviderAccountingV1 {
                delivery_stage: DeliveryStage::Planned,
                selected_tokens: cand.estimated_tokens,
                rendered_tokens: 0,
                delivered_chars: 0,
                drop_reason,
            });
        blocks.push(BlockV1 {
            id: cand.id.clone(),
            layer: cand.layer,
            provider,
            source_kind: cand.source_kind.clone(),
            source_ref: cand.source_ref.clone(),
            source_hash: cand.source_hash.clone(),
            trust_class: cand.trust_class.clone(),
            instruction_policy: cand.instruction_policy.clone(),
            base_commit: cand.base_commit.clone(),
            overlay_digest: cand.overlay_digest.clone(),
            freshness_class: cand.freshness_class,
            snapshot_id: cand.snapshot_id.clone(),
            priority: (cand.provider_score * 100.0).round() as u8,
            estimated_tokens: cand.estimated_tokens,
            allotted_tokens: allotments.get(&cand.id).copied(),
            delivery_stage: Some(DeliveryStage::Planned),
            delivery_class: Some(delivery_class),
            selected_tokens: Some(cand.estimated_tokens),
            rendered_tokens: Some(0),
            delivered_chars: Some(0),
            drop_reason: Some(drop_reason),
            protected: cand.protected,
            recoverable: cand.recoverable,
            resolver: cand.resolver.clone(),
            text: cand.text.clone(),
        });
    }

    const DEFAULT_PACKET_CHAR_BUDGET: usize = 30_000;
    let packet_char_budget_override = input.packet_char_budget_override.filter(|value| *value > 0);
    let configured_packet_char_budget =
        packet_char_budget_override.unwrap_or(DEFAULT_PACKET_CHAR_BUDGET);
    let packet = ContextPacketV1 {
        schema_version: 1,
        trace_id: trace_id.clone(),
        task: input.candidate_set.task.clone(),
        mode: input.candidate_set.mode.clone(),
        budget: BudgetV1 {
            max_tokens: input.max_tokens,
            admitted_tokens,
            packet_char_budget_default: Some(DEFAULT_PACKET_CHAR_BUDGET),
            packet_char_budget_override,
            packet_char_budget_model: input
                .packet_char_budget_model
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .cloned(),
            configured_packet_char_budget: Some(configured_packet_char_budget),
            // The pure planner has no rendered fragments or door prefix/header. Python/JS
            // finalization writes the effective value once exact remaining capacity is known.
            effective_packet_char_budget: None,
        },
        allocations,
        provider_accounting,
        blocks,
        omissions: input.candidate_set.omissions.clone(),
    };

    // 8. Receipts. One per decision. Receipts are content-free: text is
    //    banned by schema; the planner never serializes raw prompt or repo
    //    content into the receipt payload.
    let mut receipts = Vec::with_capacity(decisions.len());
    for (cand, decision, reason) in decisions {
        let admitted = decision == "admitted";
        let before_chars = cand.estimated_tokens.saturating_mul(4).max(1);
        let admitted_chars = if admitted { before_chars } else { 0 };
        let (delivery_class, selected_tokens, drop_reason) = if admitted {
            let (delivery_class, drop_reason) = planner_delivery_intent(&cand);
            (delivery_class, cand.estimated_tokens, drop_reason)
        } else {
            (DeliveryClass::MetadataOnly, 0, DropReason::NotSelected)
        };
        let content_sha = sha256_hex(&cand.text);
        receipts.push(ContextReceiptV2 {
            schema_version: 2,
            trace_id: trace_id.clone(),
            id: cand.id.clone(),
            layer: cand.layer,
            provider: candidate_provider(&cand, &input.candidate_set.provider),
            source_kind: cand.source_kind.clone(),
            source_ref: cand.source_ref.clone(),
            before_chars,
            admitted_chars,
            decision,
            reason,
            content_sha256: content_sha,
            base_commit: cand.base_commit.clone(),
            overlay_digest: cand.overlay_digest.clone(),
            freshness_class: cand.freshness_class,
            snapshot_id: cand.snapshot_id.clone(),
            delivery_stage: Some(DeliveryStage::Planned),
            delivery_class: Some(delivery_class),
            selected_tokens: Some(selected_tokens),
            allotted_tokens: admitted
                .then(|| allotments.get(&cand.id).copied())
                .flatten(),
            rendered_tokens: Some(0),
            delivered_chars: Some(0),
            drop_reason: Some(drop_reason),
            status_scope: Some(StatusScope::CandidateSet),
            status_provider: Some(input.candidate_set.provider.clone()),
            provider_status: provider_status.clone(),
            fallback_mode: fallback_mode.clone(),
            degradation_reason: degradation_reason.clone(),
            source_generation: source_generation.clone(),
            expected_release_generation: input
                .candidate_set
                .freshness
                .expected_release_generation
                .clone(),
            observed_release_generation: input
                .candidate_set
                .freshness
                .observed_release_generation
                .clone(),
            release_generation_status: input.candidate_set.freshness.release_generation_status,
        });
    }

    let structured_event = StructuredFallbackEvent {
        event: "context_fallback".into(),
        trace_id: trace_id.clone(),
        provider: input.candidate_set.provider.clone(),
        reason: degradation_reason.as_str().into(),
        mode: fallback_mode.as_str().into(),
    };

    Ok(PlannerOutput {
        packet,
        receipts,
        provider_status,
        fallback_mode,
        degradation_reason,
        source_generation,
        expected_release_generation: input
            .candidate_set
            .freshness
            .expected_release_generation
            .clone(),
        observed_release_generation: input
            .candidate_set
            .freshness
            .observed_release_generation
            .clone(),
        release_generation_status: input.candidate_set.freshness.release_generation_status,
        structured_event,
    })
}

fn release_generation_degradation(freshness: &FreshnessV1) -> Option<DegradationReason> {
    let expected = freshness.expected_release_generation.as_deref();
    let observed = freshness.observed_release_generation.as_deref();
    let status = freshness.release_generation_status;
    if expected.is_none() && observed.is_none() && status.is_none() {
        return None;
    }

    let expected_valid = expected.is_some_and(valid_release_generation);
    let observed_valid = observed.is_some_and(valid_release_generation);
    if expected_valid
        && observed_valid
        && expected == observed
        && status == Some(ReleaseGenerationStatus::Matched)
    {
        return None;
    }
    if status == Some(ReleaseGenerationStatus::Mismatch)
        || (expected_valid && observed_valid && expected != observed)
    {
        return Some(DegradationReason::ReleaseGenerationMismatch);
    }
    Some(DegradationReason::ReleaseGenerationUnavailable)
}

fn valid_release_generation(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

// ---- Helpers --------------------------------------------------------------

fn freshness_component(cand: &CandidateV1) -> f64 {
    cand.score_components
        .get("freshness")
        .copied()
        .unwrap_or(1.0)
}

fn is_version_demoted(cand: &CandidateV1) -> bool {
    if cand.id.ends_with(":superseded") || cand.id.ends_with(":proposed") {
        return true;
    }
    freshness_component(cand) == 0.0
}

fn kind_priority(cand: &CandidateV1) -> u8 {
    match cand.source_kind.as_str() {
        "repo_code" | "repo_code_overlay" => 0,
        "handoff_envelope" | "audit_finding" | "architect_decision" | "portable_text_fallback" => 1,
        "doc" => 2,
        "memory" => 3,
        _ => 2,
    }
}

/// Allocate each admitted source-kind lane proportionally to non-negative
/// retrieval score. Largest-remainder rounding is deterministic by id & never
/// changes a lane's total admitted budget.
fn score_proportional_allotments(candidates: &[&CandidateV1]) -> BTreeMap<String, usize> {
    let mut by_lane: BTreeMap<&str, Vec<&CandidateV1>> = BTreeMap::new();
    for candidate in candidates {
        by_lane
            .entry(candidate.source_kind.as_str())
            .or_default()
            .push(*candidate);
    }
    let mut out = BTreeMap::new();
    for lane in by_lane.values_mut() {
        lane.sort_by(|left, right| left.id.cmp(&right.id));
        let total = lane
            .iter()
            .map(|candidate| candidate.estimated_tokens)
            .sum::<usize>();
        let weights = lane
            .iter()
            .map(|candidate| candidate.provider_score.max(0.0))
            .collect::<Vec<_>>();
        let weight_total = weights.iter().sum::<f64>();
        let denominator = if weight_total > 0.0 {
            weight_total
        } else {
            lane.len() as f64
        };
        let mut allocated = 0usize;
        let mut fractions = Vec::new();
        for (index, candidate) in lane.iter().enumerate() {
            let weight = if weight_total > 0.0 {
                weights[index]
            } else {
                1.0
            };
            let exact = total as f64 * weight / denominator;
            let floor = exact.floor() as usize;
            allocated += floor;
            out.insert(candidate.id.clone(), floor);
            fractions.push((index, exact - floor as f64));
        }
        fractions.sort_by(
            |(left_index, left_fraction), (right_index, right_fraction)| {
                right_fraction
                    .partial_cmp(left_fraction)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(lane[*left_index].id.cmp(&lane[*right_index].id))
            },
        );
        for (index, _) in fractions.into_iter().take(total.saturating_sub(allocated)) {
            *out.entry(lane[index].id.clone()).or_default() += 1;
        }
    }
    out
}

fn sha256_hex(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn candidate_provider(candidate: &CandidateV1, fallback: &str) -> String {
    candidate
        .provider
        .as_deref()
        .filter(|provider| !provider.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn planner_delivery_intent(candidate: &CandidateV1) -> (DeliveryClass, DropReason) {
    if candidate.resolver.trim().is_empty() {
        (DeliveryClass::MetadataOnly, DropReason::MissingResolver)
    } else {
        (DeliveryClass::ResolverBacked, DropReason::None)
    }
}

fn combine_drop_reasons(left: DropReason, right: DropReason) -> DropReason {
    if left == right {
        left
    } else {
        DropReason::Multiple
    }
}

// ---- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, kind: &str, est: usize, score: f64, protected: bool) -> CandidateV1 {
        CandidateV1 {
            id: id.into(),
            layer: 3,
            provider: None,
            source_kind: kind.into(),
            source_ref: format!("path:{}", id),
            source_hash: "sha256:".to_string() + &"0".repeat(64),
            trust_class: "workspace_tracked".into(),
            instruction_policy: "data_only".into(),
            provider_score: score,
            score_components: BTreeMap::new(),
            base_commit: None,
            overlay_digest: None,
            freshness_class: None,
            snapshot_id: None,
            estimated_tokens: est,
            protected,
            exact: true,
            recoverable: true,
            resolver: "blueprint resolve".into(),
            text: format!("text for {id}"),
        }
    }

    fn candidate_set(candidates: Vec<CandidateV1>) -> ContextCandidateSetV1 {
        ContextCandidateSetV1 {
            schema_version: 1,
            trace_id: "trace-1".into(),
            task: "do a thing".into(),
            mode: "verify".into(),
            provider: "blueprint".into(),
            freshness: FreshnessV1 {
                revision: "rev-1".into(),
                indexed_at: "2026-07-12T00:00:00Z".into(),
                stale: false,
                graph_state: None,
                snapshot_id: None,
                base_commit: None,
                overlay_digest: None,
                expected_release_generation: None,
                observed_release_generation: None,
                release_generation_status: None,
            },
            provider_ceiling: ProviderCeilingV1 {
                max_candidates: 40,
                max_estimated_tokens: 8000,
            },
            candidates,
            omissions: vec![],
        }
    }

    #[test]
    fn score_proportional_allotments_are_deterministic_and_lane_bounded() {
        let high = candidate("memory:high", "memory", 60, 3.0, false);
        let low = candidate("memory:low", "memory", 40, 1.0, false);
        let skill = candidate("skill:one", "skill", 20, 1.0, false);
        let first = score_proportional_allotments(&[&high, &low, &skill]);
        let second = score_proportional_allotments(&[&skill, &low, &high]);
        assert_eq!(first, second);
        assert_eq!(first["memory:high"] + first["memory:low"], 100);
        assert_eq!(first["skill:one"], 20);
        assert!(first["memory:high"] > first["memory:low"]);
    }

    fn empty_planner_input(candidates: Vec<CandidateV1>) -> PlannerInput {
        PlannerInput {
            candidate_set: candidate_set(candidates),
            max_tokens: 1000,
            packet_char_budget_override: None,
            packet_char_budget_model: None,
            accepted_receipt_versions: vec![2],
            trace_id_override: None,
            scope_grant_present: false,
        }
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let mut input = empty_planner_input(vec![]);
        input.candidate_set.schema_version = 99;
        let err = plan(&input).unwrap_err();
        assert!(matches!(
            err,
            PlannerError::UnknownSchemaVersion { found: 99, supported }
                if supported == vec![1]
        ));
    }

    #[test]
    fn receipt_content_hash_names_candidate_content_not_source_reference() {
        let mut first = candidate("first", "repo_code", 20, 0.9, false);
        let mut second = candidate("second", "repo_code", 20, 0.8, false);
        first.source_ref = "shared:source-ref".into();
        second.source_ref = "shared:source-ref".into();
        first.text = "first admitted content".into();
        second.text = "second admitted content".into();

        let output = plan(&empty_planner_input(vec![first, second])).unwrap();
        assert_eq!(output.receipts.len(), 2);
        assert_ne!(
            output.receipts[0].content_sha256,
            output.receipts[1].content_sha256
        );
        assert_eq!(
            output.receipts[0].content_sha256,
            sha256_hex("first admitted content")
        );
        assert_eq!(
            output.receipts[1].content_sha256,
            sha256_hex("second admitted content")
        );
    }

    #[test]
    fn rejects_empty_task() {
        let mut input = empty_planner_input(vec![]);
        input.candidate_set.task = "".into();
        assert_eq!(plan(&input).unwrap_err(), PlannerError::EmptyTask);
    }

    #[test]
    fn rejects_zero_budget() {
        let mut input = empty_planner_input(vec![]);
        input.max_tokens = 0;
        assert_eq!(plan(&input).unwrap_err(), PlannerError::ZeroBudget);
    }

    #[test]
    fn rejects_receipt_version_1_only() {
        let mut input = empty_planner_input(vec![]);
        input.accepted_receipt_versions = vec![1];
        let err = plan(&input).unwrap_err();
        assert!(matches!(
            err,
            PlannerError::ReceiptVersionUnsupported { .. }
        ));
    }

    #[test]
    fn rejects_remote_untrusted_candidate() {
        let mut c = candidate("external", "repo_code", 100, 1.0, false);
        c.trust_class = "remote_untrusted".into();
        let input = empty_planner_input(vec![c]);
        let out = plan(&input).unwrap();
        assert_eq!(out.receipts.len(), 1);
        assert_eq!(out.receipts[0].decision, "rejected");
        assert_eq!(out.receipts[0].reason, "cross_root");
        assert_eq!(out.packet.blocks.len(), 0);
    }

    #[test]
    fn rejects_superseded_version() {
        let c = candidate(
            "rule:audit:src:superseded",
            "audit_finding",
            100,
            0.9,
            false,
        );
        let input = empty_planner_input(vec![c]);
        let out = plan(&input).unwrap();
        assert_eq!(out.receipts[0].reason, "superseded_version");
    }

    #[test]
    fn rejects_low_relevance_memory() {
        let mut c = candidate("memory:role:noise", "memory", 100, 0.7, false);
        c.score_components.insert("lexical".into(), 0.4);
        c.score_components.insert("structural".into(), 0.0);
        let input = empty_planner_input(vec![c]);
        let out = plan(&input).unwrap();
        assert_eq!(out.receipts[0].reason, "memory_low_relevance");
    }

    #[test]
    fn reserved_lane_admits_memory_under_overlay_flood() {
        // 30 overlay candidates at a flat 0.6 recency prior (200 tok each = 6000 tok demand)
        // + 3 relevant memories at 0.35 cosine. At a 4096 budget the old single-pass admission
        // starved memory entirely (budget_exhausted). The memory lane must admit them.
        let mut cands = Vec::new();
        for i in 0..30 {
            cands.push(candidate(
                &format!("overlay-{i:02}"),
                "repo_code_overlay",
                200,
                0.6,
                false,
            ));
        }
        for i in 0..3 {
            let mut m = candidate(&format!("memory:role:m{i}"), "memory", 150, 0.35, false);
            m.score_components.insert("structural".into(), 0.35);
            cands.push(m);
        }
        let mut input = empty_planner_input(cands);
        input.max_tokens = 4096;
        let out = plan(&input).unwrap();
        let lane: Vec<_> = out
            .receipts
            .iter()
            .filter(|r| r.reason == "within_reserved_lane")
            .collect();
        assert!(
            lane.iter().any(|r| r.id.starts_with("memory:role:")),
            "the memory lane must admit relevant memory under overlay flood"
        );
        // All 3 memories fit the 800-token lane (3 x 150 = 450).
        assert_eq!(
            out.receipts
                .iter()
                .filter(|r| r.id.starts_with("memory:role:") && r.decision == "admitted")
                .count(),
            3
        );
        // Overlay still fills the general budget after the lanes.
        assert!(out
            .packet
            .blocks
            .iter()
            .any(|b| b.source_kind == "repo_code_overlay"));
    }

    #[test]
    fn zero_lane_usage_when_no_lane_candidates() {
        // No memory/skill candidates -> lanes reserve nothing; general fill is unchanged.
        let c1 = candidate("a", "repo_code", 500, 1.0, false);
        let c2 = candidate("b", "repo_code", 600, 0.9, false);
        let mut input = empty_planner_input(vec![c1, c2]);
        input.max_tokens = 1200;
        let out = plan(&input).unwrap();
        assert_eq!(
            out.packet.blocks.len(),
            2,
            "unused reservations must not shrink the budget"
        );
    }

    #[test]
    fn deduplicates_by_id() {
        let c = candidate("a", "repo_code", 100, 1.0, false);
        let input = empty_planner_input(vec![c.clone(), c]);
        let out = plan(&input).unwrap();
        assert_eq!(out.receipts.len(), 2);
        // First encountered: kept as admitted. Second encountered: rejected as duplicate.
        let admitted_receipt = out
            .receipts
            .iter()
            .find(|r| r.decision == "admitted")
            .unwrap();
        let dup_receipt = out
            .receipts
            .iter()
            .find(|r| r.decision == "rejected")
            .unwrap();
        assert_eq!(admitted_receipt.reason, "within_global_budget");
        assert_eq!(dup_receipt.reason, "duplicate_id");
        assert_eq!(out.packet.blocks.len(), 1);
        assert_eq!(out.packet.blocks[0].id, "a");
    }

    #[test]
    fn admits_within_budget() {
        let c1 = candidate("a", "repo_code", 100, 1.0, false);
        let c2 = candidate("b", "repo_code", 100, 0.9, false);
        let input = empty_planner_input(vec![c1, c2]);
        let out = plan(&input).unwrap();
        assert_eq!(out.packet.budget.admitted_tokens, 200);
        assert_eq!(out.packet.blocks.len(), 2);
        assert_eq!(
            out.receipts
                .iter()
                .filter(|r| r.decision == "admitted")
                .count(),
            2
        );
    }

    #[test]
    fn caps_packet_blocks_and_rejects_excess_candidates_with_receipts() {
        let candidates = (0..40)
            .map(|index| candidate(&format!("candidate-{index:02}"), "repo_code", 1, 1.0, false))
            .collect();
        let out = plan(&empty_planner_input(candidates)).unwrap();

        assert_eq!(out.packet.blocks.len(), 32);
        assert_eq!(
            out.receipts
                .iter()
                .filter(|receipt| receipt.decision == "admitted")
                .count(),
            32
        );
        assert_eq!(
            out.receipts
                .iter()
                .filter(|receipt| {
                    receipt.decision == "rejected" && receipt.reason == "packet_block_limit"
                })
                .count(),
            8
        );
    }

    #[test]
    fn rejects_when_budget_exhausted() {
        let c1 = candidate("a", "repo_code", 500, 1.0, false);
        let c2 = candidate("b", "repo_code", 600, 0.9, false);
        let mut input = empty_planner_input(vec![c1, c2]);
        input.max_tokens = 700;
        let out = plan(&input).unwrap();
        // c1 admitted (cost 500); c2 would exceed (1100 > 700) -> budget_exhausted.
        assert_eq!(out.packet.blocks.len(), 1);
        let exhausted = out
            .receipts
            .iter()
            .find(|r| r.decision == "rejected")
            .unwrap();
        assert_eq!(exhausted.reason, "budget_exhausted");
    }

    #[test]
    fn fresh_provider_emits_fresh_provider_status_v2_fields() {
        let c = candidate("a", "repo_code", 50, 0.9, false);
        let out = plan(&empty_planner_input(vec![c])).unwrap();
        assert_eq!(out.provider_status, ProviderStatus::Fresh);
        assert_eq!(out.fallback_mode, FallbackMode::None);
        assert_eq!(out.degradation_reason, DegradationReason::None);
        assert!(out.source_generation.is_some());
        assert_eq!(out.receipts[0].schema_version, 2);
        assert_eq!(out.receipts[0].provider_status, ProviderStatus::Fresh);
    }

    #[test]
    fn stale_freshness_triggers_portable_text_only_fallback() {
        let c = candidate("anchor", "repo_code", 50, 0.9, true);
        let mut input = empty_planner_input(vec![c]);
        input.candidate_set.freshness.stale = true;
        let out = plan(&input).unwrap();
        assert_eq!(out.provider_status, ProviderStatus::Stale);
        assert_eq!(out.fallback_mode, FallbackMode::PortableTextOnly);
        assert_eq!(out.degradation_reason, DegradationReason::BlueprintStale);
        assert!(out.source_generation.is_none());
        // The protected anchor still admits even under fallback quarantine.
        assert_eq!(out.packet.blocks.len(), 1);
    }

    #[test]
    fn matching_release_generation_preserves_rich_delivery_and_stamps_receipt() {
        let mut value = serde_json::to_value(empty_planner_input(vec![candidate(
            "current",
            "repo_code",
            50,
            0.9,
            false,
        )]))
        .unwrap();
        let generation = format!("sha256:{}", "7".repeat(64));
        value["candidate_set"]["freshness"]["expectedReleaseGeneration"] =
            serde_json::Value::String(generation.clone());
        value["candidate_set"]["freshness"]["observedReleaseGeneration"] =
            serde_json::Value::String(generation.clone());
        value["candidate_set"]["freshness"]["releaseGenerationStatus"] =
            serde_json::Value::String("matched".into());
        let input: PlannerInput = serde_json::from_value(value).unwrap();

        let out = plan(&input).unwrap();

        assert_eq!(out.fallback_mode, FallbackMode::None);
        assert_eq!(out.packet.blocks.len(), 1);
        assert_eq!(
            out.expected_release_generation.as_deref(),
            Some(generation.as_str())
        );
        assert_eq!(
            out.observed_release_generation.as_deref(),
            Some(generation.as_str())
        );
        assert_eq!(
            out.release_generation_status,
            Some(ReleaseGenerationStatus::Matched)
        );
        assert_eq!(
            out.receipts[0].release_generation_status,
            Some(ReleaseGenerationStatus::Matched)
        );
    }

    #[test]
    fn release_generation_mismatch_never_delivers_rich_blocks() {
        let mut value = serde_json::to_value(empty_planner_input(vec![candidate(
            "wrong",
            "repo_code",
            50,
            0.9,
            false,
        )]))
        .unwrap();
        value["candidate_set"]["freshness"]["expectedReleaseGeneration"] =
            serde_json::Value::String(format!("sha256:{}", "7".repeat(64)));
        value["candidate_set"]["freshness"]["observedReleaseGeneration"] =
            serde_json::Value::String(format!("sha256:{}", "8".repeat(64)));
        value["candidate_set"]["freshness"]["releaseGenerationStatus"] =
            serde_json::Value::String("mismatch".into());
        let input: PlannerInput = serde_json::from_value(value).unwrap();

        let out = plan(&input).unwrap();

        assert_eq!(out.fallback_mode, FallbackMode::PortableTextOnly);
        assert_eq!(
            out.degradation_reason,
            DegradationReason::ReleaseGenerationMismatch
        );
        assert_eq!(out.structured_event.reason, "release_generation_mismatch");
        assert!(out.packet.blocks.is_empty());
    }

    #[test]
    fn partial_or_invalid_release_generation_is_unavailable_but_both_absent_is_legacy_compatible() {
        let legacy = plan(&empty_planner_input(vec![candidate(
            "legacy",
            "repo_code",
            50,
            0.9,
            false,
        )]))
        .unwrap();
        assert_eq!(legacy.fallback_mode, FallbackMode::None);
        assert_eq!(legacy.packet.blocks.len(), 1);

        let mut value = serde_json::to_value(empty_planner_input(vec![candidate(
            "partial",
            "repo_code",
            50,
            0.9,
            false,
        )]))
        .unwrap();
        value["candidate_set"]["freshness"]["expectedReleaseGeneration"] =
            serde_json::Value::String("not-a-generation".into());
        value["candidate_set"]["freshness"]["releaseGenerationStatus"] =
            serde_json::Value::String("matched".into());
        let input: PlannerInput = serde_json::from_value(value).unwrap();

        let out = plan(&input).unwrap();

        assert_eq!(out.fallback_mode, FallbackMode::PortableTextOnly);
        assert_eq!(
            out.degradation_reason,
            DegradationReason::ReleaseGenerationUnavailable
        );
        assert!(out.packet.blocks.is_empty());
    }

    #[test]
    fn dirty_overlay_is_advisory_and_provenance_reaches_blocks_and_receipts() {
        let base_commit = "a".repeat(40);
        let overlay_digest = format!("sha256:{}", "b".repeat(64));
        let snapshot_id = format!("sha256:{}", "c".repeat(64));
        let mut c = candidate("dirty", "repo_code_overlay", 50, 0.9, false);
        c.base_commit = Some(base_commit.clone());
        c.overlay_digest = Some(overlay_digest.clone());
        c.freshness_class = Some(FreshnessClass::DirtyOverlay);
        c.snapshot_id = Some(snapshot_id.clone());
        let mut input = empty_planner_input(vec![c]);
        input.candidate_set.freshness.graph_state = Some(GraphState::DirtyOverlay);
        input.candidate_set.freshness.snapshot_id = Some(snapshot_id);

        let out = plan(&input).unwrap();

        assert_eq!(out.provider_status, ProviderStatus::Degraded);
        assert_eq!(out.fallback_mode, FallbackMode::None);
        assert_eq!(out.degradation_reason, DegradationReason::BlueprintStale);
        assert_eq!(out.packet.blocks.len(), 1);
        assert_eq!(
            out.packet.blocks[0].base_commit.as_deref(),
            Some(base_commit.as_str())
        );
        assert_eq!(
            out.packet.blocks[0].freshness_class,
            Some(FreshnessClass::DirtyOverlay)
        );
        assert_eq!(
            out.receipts[0].overlay_digest.as_deref(),
            Some(overlay_digest.as_str())
        );
        assert_eq!(
            out.receipts[0].freshness_class,
            Some(FreshnessClass::DirtyOverlay)
        );
    }

    #[test]
    fn unsupported_provider_quarantines_non_anchors() {
        let c = candidate("plain", "repo_code", 50, 0.9, false);
        let mut input = empty_planner_input(vec![c]);
        input.candidate_set.provider = "blueprint-unsupported".into();
        let out = plan(&input).unwrap();
        assert_eq!(out.provider_status, ProviderStatus::Unavailable);
        assert_eq!(out.fallback_mode, FallbackMode::NonGraphSourcesOnly);
        // Non-anchor non-portable-fallback candidate is quarantined.
        let q = out
            .receipts
            .iter()
            .find(|r| r.decision == "rejected")
            .unwrap();
        assert_eq!(q.reason, "fallback_quarantine");
    }

    #[test]
    fn unsupported_provider_admits_portable_text_fallback_anchor() {
        let anchor = candidate("manifest", "portable_text_fallback", 50, 0.5, false);
        let c = candidate("plain", "repo_code", 50, 0.9, false);
        let mut input = empty_planner_input(vec![anchor, c]);
        input.candidate_set.provider = "blueprint-unsupported".into();
        let out = plan(&input).unwrap();
        let admitted: Vec<_> = out
            .receipts
            .iter()
            .filter(|r| r.decision == "admitted")
            .collect();
        assert_eq!(admitted.len(), 1);
        assert_eq!(admitted[0].id, "manifest");
    }

    #[test]
    fn receipts_have_no_text_field() {
        let c = candidate("a", "repo_code", 50, 0.9, false);
        let out = plan(&empty_planner_input(vec![c])).unwrap();
        for r in &out.receipts {
            let serialised = serde_json::to_string(r).unwrap();
            assert!(!serialised.contains("\"text\":"));
            assert!(!serialised.contains("\"raw"));
        }
    }

    #[test]
    fn allocations_populated_by_layer() {
        let c3 = candidate("c3", "repo_code", 100, 1.0, false);
        let mut c7 = candidate("c7", "memory", 50, 0.5, false);
        c7.layer = 7;
        c7.score_components.insert("structural".into(), 0.5);
        let out = plan(&empty_planner_input(vec![c3, c7])).unwrap();
        assert_eq!(out.packet.allocations["3"], 100);
        assert_eq!(out.packet.allocations["7"], 50);
    }

    #[test]
    fn structured_fallback_event_always_present() {
        let c = candidate("a", "repo_code", 50, 0.9, false);
        let out = plan(&empty_planner_input(vec![c])).unwrap();
        assert_eq!(out.structured_event.event, "context_fallback");
        assert_eq!(out.structured_event.trace_id, "trace-1");
        assert_eq!(out.structured_event.provider, "blueprint");
    }

    #[test]
    fn ranking_prefers_higher_provider_score() {
        let lo = candidate("lo", "repo_code", 100, 0.5, false);
        let hi = candidate("hi", "repo_code", 100, 0.95, false);
        let out = plan(&empty_planner_input(vec![lo, hi])).unwrap();
        assert_eq!(out.packet.blocks[0].id, "hi");
        assert_eq!(out.packet.blocks[1].id, "lo");
    }

    #[test]
    fn shas_are_deterministic() {
        let c = candidate("a", "repo_code", 100, 0.9, false);
        let o1 = plan(&empty_planner_input(vec![c.clone()])).unwrap();
        let o2 = plan(&empty_planner_input(vec![c])).unwrap();
        let r1 = serde_json::to_string(&o1.receipts[0]).unwrap();
        let r2 = serde_json::to_string(&o2.receipts[0]).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn candidate_provider_is_preserved_with_candidate_set_fallback() {
        let mut input_value = serde_json::to_value(empty_planner_input(vec![
            candidate("a", "repo_code", 100, 0.9, false),
            candidate("b", "repo_code", 50, 0.8, false),
            candidate("legacy", "repo_code", 25, 0.7, false),
        ]))
        .unwrap();
        input_value["candidate_set"]["candidates"][0]["provider"] =
            serde_json::Value::String("blueprint".into());
        input_value["candidate_set"]["candidates"][1]["provider"] =
            serde_json::Value::String("memright".into());
        let input: PlannerInput = serde_json::from_value(input_value).unwrap();

        let out = plan(&input).unwrap();
        let block_providers: BTreeMap<_, _> = out
            .packet
            .blocks
            .iter()
            .map(|block| (block.id.as_str(), block.provider.as_str()))
            .collect();
        assert_eq!(block_providers["a"], "blueprint");
        assert_eq!(block_providers["b"], "memright");
        assert_eq!(block_providers["legacy"], "blueprint");
        let receipt_providers: BTreeMap<_, _> = out
            .receipts
            .iter()
            .map(|receipt| (receipt.id.as_str(), receipt.provider.as_str()))
            .collect();
        assert_eq!(receipt_providers, block_providers);
    }

    #[test]
    fn planner_records_selected_delivery_without_claiming_rendering() {
        let resolved = candidate("resolved", "repo_code", 100, 0.9, false);
        let mut metadata = candidate("metadata", "repo_code", 40, 0.8, false);
        metadata.resolver.clear();
        let mut input_value =
            serde_json::to_value(empty_planner_input(vec![resolved, metadata])).unwrap();
        input_value["candidate_set"]["candidates"][0]["provider"] =
            serde_json::Value::String("blueprint".into());
        input_value["candidate_set"]["candidates"][1]["provider"] =
            serde_json::Value::String("memright".into());
        let input: PlannerInput = serde_json::from_value(input_value).unwrap();

        let out = plan(&input).unwrap();
        let blocks = serde_json::to_value(&out.packet.blocks).unwrap();
        assert_eq!(blocks[0]["deliveryStage"], "planned");
        assert_eq!(blocks[0]["deliveryClass"], "resolver_backed");
        assert_eq!(blocks[0]["selectedTokens"], 100);
        assert_eq!(blocks[0]["renderedTokens"], 0);
        assert_eq!(blocks[0]["deliveredChars"], 0);
        assert_eq!(blocks[0]["dropReason"], "none");
        assert_eq!(blocks[1]["deliveryClass"], "metadata_only");
        assert_eq!(blocks[1]["deliveryStage"], "planned");
        assert_eq!(blocks[1]["selectedTokens"], 40);
        assert_eq!(blocks[1]["renderedTokens"], 0);
        assert_eq!(blocks[1]["deliveredChars"], 0);
        assert_eq!(blocks[1]["dropReason"], "missing_resolver");

        let receipts = serde_json::to_value(&out.receipts).unwrap();
        assert_eq!(receipts[0]["deliveryStage"], "planned");
        assert_eq!(receipts[0]["deliveryClass"], "resolver_backed");
        assert_eq!(receipts[0]["selectedTokens"], 100);
        assert_eq!(receipts[0]["dropReason"], "none");
        assert_eq!(receipts[1]["deliveryClass"], "metadata_only");
        assert_eq!(receipts[1]["deliveryStage"], "planned");
        assert_eq!(receipts[1]["selectedTokens"], 40);
        assert_eq!(receipts[1]["renderedTokens"], 0);
        assert_eq!(receipts[1]["deliveredChars"], 0);
        assert_eq!(receipts[1]["dropReason"], "missing_resolver");

        let packet = serde_json::to_value(&out.packet).unwrap();
        assert_eq!(
            packet["providerAccounting"]["blueprint"]["deliveryStage"],
            "planned"
        );
        assert_eq!(
            packet["providerAccounting"]["blueprint"]["selectedTokens"],
            100
        );
        assert_eq!(
            packet["providerAccounting"]["memright"]["selectedTokens"],
            40
        );
        assert_eq!(
            packet["providerAccounting"]["memright"]["renderedTokens"],
            0
        );
        assert_eq!(
            packet["providerAccounting"]["memright"]["deliveredChars"],
            0
        );
        assert_eq!(
            packet["providerAccounting"]["memright"]["dropReason"],
            "missing_resolver"
        );
    }

    #[test]
    fn rejected_receipt_is_not_selected_for_delivery() {
        let mut input = empty_planner_input(vec![
            candidate("selected", "repo_code", 500, 1.0, false),
            candidate("rejected", "repo_code", 600, 0.9, false),
        ]);
        input.max_tokens = 700;
        let out = plan(&input).unwrap();
        let receipt = out
            .receipts
            .iter()
            .find(|receipt| receipt.id == "rejected")
            .unwrap();
        let receipt = serde_json::to_value(receipt).unwrap();

        assert_eq!(receipt["decision"], "rejected");
        assert_eq!(receipt["admittedChars"], 0);
        assert_eq!(receipt["selectedTokens"], 0);
        assert_eq!(receipt["deliveryClass"], "metadata_only");
        assert_eq!(receipt["renderedTokens"], 0);
        assert_eq!(receipt["deliveredChars"], 0);
        assert_eq!(receipt["dropReason"], "not_selected");
    }

    #[test]
    fn receipt_status_is_scoped_to_candidate_set_provider() {
        let mut input_value = serde_json::to_value(empty_planner_input(vec![candidate(
            "memory",
            "repo_code",
            50,
            0.9,
            false,
        )]))
        .unwrap();
        input_value["candidate_set"]["candidates"][0]["provider"] =
            serde_json::Value::String("memright".into());
        let input: PlannerInput = serde_json::from_value(input_value).unwrap();
        let out = plan(&input).unwrap();
        let receipt = serde_json::to_value(&out.receipts[0]).unwrap();

        assert_eq!(receipt["provider"], "memright");
        assert_eq!(receipt["statusScope"], "candidate_set");
        assert_eq!(receipt["statusProvider"], "blueprint");
    }

    #[test]
    fn provider_accounting_combines_mixed_outcomes_deterministically() {
        let resolved = candidate("resolved", "repo_code", 100, 0.9, false);
        let mut metadata = candidate("metadata", "repo_code", 40, 0.8, false);
        metadata.resolver = "   ".into();
        let input = empty_planner_input(vec![resolved, metadata]);

        let out = plan(&input).unwrap();
        let accounting = &out.packet.provider_accounting["blueprint"];
        assert_eq!(accounting.selected_tokens, 140);
        assert_eq!(accounting.drop_reason, DropReason::Multiple);

        let reversed = empty_planner_input(vec![
            {
                let mut candidate = candidate("metadata", "repo_code", 40, 0.8, false);
                candidate.resolver = "   ".into();
                candidate
            },
            candidate("resolved", "repo_code", 100, 0.9, false),
        ]);
        let reversed_out = plan(&reversed).unwrap();
        assert_eq!(
            reversed_out.packet.provider_accounting["blueprint"].drop_reason,
            DropReason::Multiple
        );
    }

    #[test]
    fn legacy_packet_deserializes_and_reserializes_without_delivery_extensions() {
        let legacy = serde_json::json!({
            "schemaVersion": 1,
            "traceId": "11111111-1111-4111-8111-111111111111",
            "task": "Find the request handler and its storage path",
            "mode": "verify",
            "budget": {"maxTokens": 2000, "admittedTokens": 120},
            "allocations": {"3": 120, "7": 0, "other": 0},
            "blocks": [{
                "id": "symbol:route-handler",
                "layer": 3,
                "provider": "blueprint",
                "sourceKind": "repo_code",
                "sourceRef": "src/routes.ts:10-24",
                "sourceHash": format!("sha256:{}", "a".repeat(64)),
                "trustClass": "workspace_tracked",
                "instructionPolicy": "data_only",
                "priority": 80,
                "estimatedTokens": 120,
                "protected": false,
                "recoverable": true,
                "resolver": "blueprint resolve symbol:route-handler",
                "text": "Bounded source-backed candidate"
            }],
            "omissions": [{"id": "memory:other", "layer": 7, "reason": "global_budget"}]
        });

        let packet: ContextPacketV1 = serde_json::from_value(legacy).unwrap();
        let serialized = serde_json::to_value(packet).unwrap();
        assert!(serialized.get("providerAccounting").is_none());
        let block = &serialized["blocks"][0];
        for field in [
            "deliveryStage",
            "deliveryClass",
            "selectedTokens",
            "renderedTokens",
            "deliveredChars",
            "dropReason",
        ] {
            assert!(block.get(field).is_none(), "legacy block emitted {field}");
        }
    }

    #[test]
    fn packet_budget_override_is_carried_in_the_planned_packet_contract() {
        let base = empty_planner_input(vec![candidate("a", "repo_code", 10, 0.9, false)]);
        let mut value = serde_json::to_value(base).unwrap();
        value["packet_char_budget_override"] = serde_json::json!(321);
        value["packet_char_budget_model"] = serde_json::json!("test-model");
        let input: PlannerInput = serde_json::from_value(value).unwrap();

        let packet = serde_json::to_value(plan(&input).unwrap().packet).unwrap();

        assert_eq!(packet["budget"]["packetCharBudgetDefault"], 30_000);
        assert_eq!(packet["budget"]["packetCharBudgetOverride"], 321);
        assert_eq!(packet["budget"]["packetCharBudgetModel"], "test-model");
        assert_eq!(packet["budget"]["configuredPacketCharBudget"], 321);
        assert_eq!(
            packet["budget"]["effectivePacketCharBudget"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn legacy_receipt_deserializes_and_reserializes_without_delivery_extensions() {
        let legacy = serde_json::json!({
            "schemaVersion": 2,
            "traceId": "11111111-1111-4111-8111-111111111111",
            "id": "symbol:src/service.ts::OrderService.placeOrder",
            "layer": 3,
            "provider": "blueprint",
            "sourceKind": "repo_code",
            "sourceRef": "src/service.ts:6-9",
            "beforeChars": 720,
            "admittedChars": 480,
            "decision": "admitted",
            "reason": "within_global_budget",
            "contentSha256": "a".repeat(64),
            "providerStatus": "fresh",
            "fallbackMode": "none",
            "degradationReason": "none",
            "sourceGeneration": "blueprint-rev-2cd3e52"
        });

        let receipt: ContextReceiptV2 = serde_json::from_value(legacy).unwrap();
        let serialized = serde_json::to_value(receipt).unwrap();
        for field in [
            "deliveryStage",
            "deliveryClass",
            "selectedTokens",
            "renderedTokens",
            "deliveredChars",
            "dropReason",
            "statusScope",
            "statusProvider",
        ] {
            assert!(
                serialized.get(field).is_none(),
                "legacy receipt emitted {field}"
            );
        }
    }

    #[test]
    fn planner_emits_complete_delivery_extensions() {
        let out = plan(&empty_planner_input(vec![candidate(
            "current",
            "repo_code",
            100,
            0.9,
            false,
        )]))
        .unwrap();
        let packet = serde_json::to_value(&out.packet).unwrap();
        assert!(!packet["providerAccounting"].as_object().unwrap().is_empty());
        let block = &packet["blocks"][0];
        for field in [
            "deliveryStage",
            "deliveryClass",
            "selectedTokens",
            "renderedTokens",
            "deliveredChars",
            "dropReason",
        ] {
            assert!(block.get(field).is_some(), "planned block omitted {field}");
        }
        let receipt = serde_json::to_value(&out.receipts[0]).unwrap();
        for field in [
            "deliveryStage",
            "deliveryClass",
            "selectedTokens",
            "renderedTokens",
            "deliveredChars",
            "dropReason",
            "statusScope",
            "statusProvider",
        ] {
            assert!(
                receipt.get(field).is_some(),
                "planned receipt omitted {field}"
            );
        }
    }
}
