//! The five typed Membrane contract shapes.
//!
//! These Rust types are the **source of truth** for the Membrane protocol.
//! Field names, optionality, and JSON casing are modelled faithfully on how the
//! shapes are actually produced and consumed:
//!
//!   * `ScopeGrantV1`            — minted/validated by `mcp/scope-grant-v1.mjs`.
//!   * `ContextCandidateSetV1`   — parsed by the admission planner
//!                                 (`crypt-core::planner`) and emitted by federation.
//!   * `ContextPacketV1`         — the bounded packet the planner emits.
//!   * `ContextReceiptV1`        — the content-free per-candidate receipt (planner
//!                                 receipt body; `schemaVersion` is the receipt
//!                                 envelope version, 1 or 2).
//!   * `KnowledgeEmissionV1`     — the proposal body persisted verbatim by
//!                                 `membrane_knowledge_propose` / `ProposalStore`.
//!
//! Serde conventions: every struct uses `rename_all = "camelCase"`; closed shapes
//! (grant, candidate-set, packet, receipt) use `deny_unknown_fields`; optional
//! fields are `default + skip_serializing_if = "Option::is_none"` so they are
//! absent from JSON when `None`; enums serialize as `snake_case` strings.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ============================================================================
// ScopeGrant v1
// ============================================================================

/// One permitted read edge a grant authorizes (an exact `path:start-end` range).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadPathV1 {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// ScopeGrantV1 — the bounded, Ed25519-signed authority to read exact source
/// ranges for one task. Minted by `mintScopeGrantV1` in `mcp/scope-grant-v1.mjs`;
/// every field name and enum value below matches that module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeGrantV1 {
    pub schema_version: u32,
    /// Unique grant id, `sgv1-<uuid>`.
    pub id: String,
    /// Issuer principal; fixed to `rightcontext-gateway` by the minter.
    pub issuer: String,
    /// Issuing client; fixed to `mcp`.
    pub client: String,
    /// Repository roots this grant is confined to.
    pub repository_ids: Vec<String>,
    /// Edge types this grant permits (e.g. `source_read`).
    pub permitted_edge_types: Vec<String>,
    pub task_id: String,
    pub session_id: String,
    /// RFC 3339 / ISO 8601 issuance instant.
    pub issued_at: String,
    /// RFC 3339 / ISO 8601 expiry instant.
    pub expires_at: String,
    /// Lifecycle status; `active` while usable.
    pub status: String,
    /// Replay-protection nonce (>= 8 hex chars).
    pub nonce: String,
    /// `sha256:<hex>` of the freshness manifest.
    pub manifest_digest: String,
    pub repository_id: String,
    pub repository_root: String,
    /// Blueprint blueprint generation the grant binds to.
    pub blueprint_generation: String,
    /// Blueprint graph freshness state (`clean`, `stale_snapshot`, ...).
    pub blueprint_freshness: String,
    /// `sha256:<hex>` digest of the canonical request object.
    pub request_hash: String,
    /// `sha256:<hex>` digest of the canonical context packet.
    pub context_packet_hash: String,
    /// Exact source ranges the grant authorizes reading.
    pub read_paths: Vec<ReadPathV1>,
    pub source_read_bytes_max: u32,
    pub unique_files_max: u32,
    pub results_max: u32,
    /// `available` when a Crypt block was admitted, else `degraded`.
    pub crypt_status: String,
    pub degraded: bool,
    /// Signature algorithm; fixed to `Ed25519`.
    pub signature_algorithm: String,
    /// `scope-grant-ed25519-v1:<hex>` key identifier.
    pub key_id: String,
    /// Base64 Ed25519 signature over the domain-separated canonical bytes.
    pub signature: String,
}

// ============================================================================
// ContextCandidateSet v1
// ============================================================================

/// Graph freshness state for the candidate set's source generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// Per-candidate freshness classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessClass {
    Current,
    CommittedSnapshot,
    DirtyOverlay,
    StaleSnapshot,
    Unknown,
}

/// Release-generation match status between expected and observed builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseGenerationStatus {
    Matched,
    Mismatch,
    Unavailable,
}

/// Identity of the dirty overlay used for one retrieval. Overlay state is
/// session-local even when the committed graph is shared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayIdentityV1 {
    #[serde(alias = "session_id")]
    pub session_id: String,
    #[serde(alias = "worktree_path")]
    pub worktree_path: String,
    #[serde(alias = "generation_id")]
    pub generation_id: String,
    #[serde(alias = "overlay_digest")]
    pub overlay_digest: String,
}

/// Freshness provenance shared by every candidate in the set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_identity: Option<OverlayIdentityV1>,
}

/// The provider's self-declared admission ceiling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderCeilingV1 {
    pub max_candidates: u32,
    pub max_estimated_tokens: u32,
}

/// One candidate piece of context offered for admission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    /// Lane-local relevance signal in [0, 1]; NOT a cross-provider probability.
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
    pub estimated_tokens: u32,
    pub protected: bool,
    pub exact: bool,
    pub recoverable: bool,
    pub resolver: String,
    pub text: String,
}

/// A candidate the provider dropped before submission, with a reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OmissionV1 {
    pub id: String,
    #[serde(default)]
    pub layer: Option<u8>,
    pub reason: String,
}

/// ContextCandidateSetV1 — the federated set of candidates fed to admission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

// ============================================================================
// ContextPacket v1
// ============================================================================

/// Where in the delivery pipeline a block/provider stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStage {
    Planned,
    Finalized,
}

/// How a block's content reaches the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryClass {
    ResolverBacked,
    MetadataOnly,
    Rendered,
}

/// MBR-608: explicit lane every block occupies under the single
/// cross-provider attention budget. Lanes are mutually exclusive per block;
/// the cross-provider total is the sum of the per-lane totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetLaneKind {
    /// Host-loaded content with a matching host receipt (MBR-010). Zero bytes
    /// / tokens; the host already has the data.
    Native,
    /// Inline text the renderer serialized into the prompt.
    Rendered,
    /// Resolver-backed reference; the agent retrieves on demand.
    ResolverBacked,
    /// Metadata without content. Zero tokens; zero bytes.
    MetadataOnly,
}

impl BudgetLaneKind {
    /// Stable, lowercase label shared with the JS renderer and the
    /// receipt code. The label is the canonical string contract.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Rendered => "rendered",
            Self::ResolverBacked => "resolver_backed",
            Self::MetadataOnly => "metadata_only",
        }
    }

    /// True when the lane consumes tokens. `Native`, `ResolverBacked`, and
    /// `MetadataOnly` are zero-token lanes; only `Rendered` adds to the
    /// cross-provider selected/delivered totals.
    pub fn consumes_tokens(&self) -> bool {
        matches!(self, Self::Rendered)
    }
}

impl std::fmt::Display for BudgetLaneKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The fixed, ordered set of lanes. The order is the contract order; both
/// the Rust and TypeScript reconciliations iterate this exact sequence so the
/// receipts never reorder.
pub const BUDGET_LANE_KINDS: &[BudgetLaneKind] = &[
    BudgetLaneKind::Native,
    BudgetLaneKind::Rendered,
    BudgetLaneKind::ResolverBacked,
    BudgetLaneKind::MetadataOnly,
];

/// MBR-608: how the content reached the agent at the renderer's level of
/// detail. Carries the same wire-level meaning as the JS
/// `mcp/context-renderer-lib.cjs` `deliveryMode` enum but is the canonical
/// Rust type for IPC and the receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    Native,
    Inline,
    Reference,
}

/// Why content was (or would be) dropped from the final packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// The packet's token/char budget ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetV1 {
    pub max_tokens: u32,
    pub admitted_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_char_budget_default: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_char_budget_override: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_char_budget_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_packet_char_budget: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_packet_char_budget: Option<u32>,
}

/// Per-provider accounting of what survived to delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderAccountingV1 {
    pub delivery_stage: DeliveryStage,
    pub selected_tokens: u32,
    pub rendered_tokens: u32,
    pub delivered_chars: u32,
    pub drop_reason: DropReason,
}

/// MBR-608: one row of the receipt-side reconciliation. Each row is one
/// lane under the single cross-provider attention budget. The list always
/// iterates in the canonical lane order: `native`, `rendered`,
/// `resolver_backed`, `metadata_only`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetReconciliationRowV1 {
    pub lane: BudgetLaneKind,
    pub selected_tokens: u32,
    pub delivered_tokens: u32,
    pub delivered_chars: u32,
    pub blocks: u32,
}

/// MBR-608: the single receipt-side reconciliation. Every receipt carries
/// one. The per-lane totals are the sum of the per-block lane totals; the
/// alert id is the FIRST failure (deterministic order).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetReconciliationV1 {
    pub schema_version: u32,
    pub max_tokens: u32,
    pub total_selected_tokens: u32,
    pub total_delivered_tokens: u32,
    pub total_delivered_chars: u32,
    pub lanes: Vec<BudgetReconciliationRowV1>,
    pub unclassified_blocks: u32,
    pub balanced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert: Option<String>,
}

/// One admitted block of context in the packet the agent receives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    pub estimated_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_stage: Option<DeliveryStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_class: Option<DeliveryClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_tokens: Option<u32>,
    /// Planner-owned transform target; consumers may compress to this bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allotted_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_chars: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop_reason: Option<DropReason>,
    pub protected: bool,
    pub recoverable: bool,
    pub resolver: String,
    /// MBR-608: how the content reached the agent (native / inline / reference).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_mode: Option<DeliveryMode>,
    /// MBR-608: the explicit lane this block occupies under the single
    /// cross-provider budget. The renderer stamps this when the block
    /// finalizes; the receipt reconciliation sums per-block lanes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<BudgetLaneKind>,
    pub text: String,
}

/// ContextPacketV1 — the bounded packet admitted under one token budget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextPacketV1 {
    pub schema_version: u32,
    pub trace_id: String,
    pub task: String,
    pub mode: String,
    pub budget: BudgetV1,
    pub allocations: BTreeMap<String, u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider_accounting: BTreeMap<String, ProviderAccountingV1>,
    pub blocks: Vec<BlockV1>,
    pub omissions: Vec<OmissionV1>,
    /// MBR-608: the single cross-provider budget reconciliation. Every
    /// receipt carries one of these.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciliation: Option<BudgetReconciliationV1>,
}

// ============================================================================
// ContextReceipt (content-free per-candidate receipt)
// ============================================================================

/// Planner-observed provider health for the admission pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatus {
    Fresh,
    Stale,
    Unavailable,
    Degraded,
}

/// The fallback contract applied when freshness/provider degrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackMode {
    None,
    NonGraphSourcesOnly,
    PortableTextOnly,
}

/// Why the packet was degraded, if it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// The scope a status field applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusScope {
    CandidateSet,
}

/// ContextReceiptV1 — the content-free record, per candidate, of what entered
/// context and what did not, and why. Carries no raw content (only a SHA-256 of
/// it). This is the receipt *body*; `schema_version` is the receipt envelope
/// version (1 or 2). The planner always populates the v2 observability fields; a
/// v1 consumer simply ignores them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextReceiptV1 {
    pub schema_version: u32,
    pub trace_id: String,
    pub id: String,
    pub layer: u8,
    pub provider: String,
    pub source_kind: String,
    pub source_ref: String,
    pub before_chars: u32,
    pub admitted_chars: u32,
    /// `admitted` | `rejected` | `budget_exhausted` | ...
    pub decision: String,
    /// Machine reason code, e.g. `within_global_budget`, `duplicate_id`.
    pub reason: String,
    /// `sha256:<hex>` of the candidate content (content-free provenance).
    pub content_sha256: String,
    /// IDs absorbed into this winner during deduplication.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deduplicated_from: Vec<String>,
    /// Winning candidate ID when this candidate lost dedup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deduplicated_to: Option<String>,
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
    pub selected_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allotted_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_chars: Option<u32>,
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

// ============================================================================
// KnowledgeEmission v1
// ============================================================================

/// KnowledgeEmissionV1 — a bounded, typed proposal to persist durable memory.
/// Submitted via `membrane_knowledge_propose`; the body is persisted verbatim by
/// `ProposalStore`. `text` is the canonical content field (the server reads
/// `emission.text ?? emission.content` and re-stores with `text`). Provenance is
/// open by design so future metadata does not break the contract, but
/// `schema_version`, `kind`, and `text` are always required.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEmissionV1 {
    pub schema_version: u32,
    /// The emission discriminator, e.g. `fact`, `decision`, `preference`, `lesson`.
    pub kind: String,
    /// The durable content (canonical field).
    pub text: String,
    /// Free-form provenance / metadata (source refs, confidence, tags). Open map.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provenance: BTreeMap<String, serde_json::Value>,
}
