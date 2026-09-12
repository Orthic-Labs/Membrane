//! Operation-specific contract shapes for the Membrane MCP tool surface.
//!
//! Each MCP tool operation gets its OWN independently-versioned contract:
//! a closed typed error taxonomy, a success output shape, and a single
//! `OperationResult` envelope that discriminates `kind: "success" | "error"`.
//! Independent versioning means bumping one operation's `errorVersion` (or
//! `schemaVersion`) NEVER forces a sibling to move; the cross-operation
//! registry (`OPERATIONS`) is the only place that observes the whole set.
//!
//! These Rust types are the source of truth for the
//! `schemas/operations/*.v1.schema.json` documents. The round-trip tests in
//! `tests/operations_roundtrip.rs` load each operation's schema + fixtures
//! from disk and assert:
//!
//!   1. The success fixture validates against the schema's success branch.
//!   2. The error fixture validates against the schema's error branch.
//!   3. The `OperationSpec` enum-representation deserializes each fixture
//!      into the correct variant without lossy coercion.
//!   4. The `OPERATIONS` registry matches the `operations-index.v1` fixture
//!      file (file names, schemaVersions, errorVersions, error-code lists).
//!
//! The companion TypeScript binding lives at
//! `engine/crates/membrane-protocol/bindings/operations.mjs` and
//! re-implements (1)+(2)+(4) with the same minimal JSON-Schema subset the
//! MBR-101 binding uses.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Canonical Membrane ownership axes. These names are part of the protocol
/// vocabulary, not product-specific implementation aliases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubsystemAxis {
    Pull,
    Push,
    Cortex,
    Blueprint,
    Ledger,
    Adapt,
}

impl SubsystemAxis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pull => "pull",
            Self::Push => "push",
            Self::Cortex => "cortex",
            Self::Blueprint => "blueprint",
            Self::Ledger => "ledger",
            Self::Adapt => "adapt",
        }
    }
}

/// Return protocol ownership for one operation. Hub operations intentionally
/// return `None`: Hub is an integration surface, not a semantic subsystem.
pub fn axis_for_operation(name: &str) -> Option<SubsystemAxis> {
    match name {
        "membrane_context" | "pull" => Some(SubsystemAxis::Pull),
        "membrane_source_read" => Some(SubsystemAxis::Ledger),
        "membrane_blueprint" => Some(SubsystemAxis::Blueprint),
        "membrane_knowledge_propose"
        | "membrane_memory"
        | "membrane_memory_read"
        | "membrane_knowledge_review"
        | "membrane_checkpoint_save"
        | "membrane_checkpoint_load"
        | "membrane_working_context"
        | "membrane_temporal_fact"
        | "membrane_scratchpad"
        | "membrane_feedback" => Some(SubsystemAxis::Cortex),
        "push" => Some(SubsystemAxis::Cortex),
        _ => None,
    }
}

/// The discriminator for an operation's response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultKind {
    Success,
    Error,
}

/// Installed resident-controller lease protocol.  It is intentionally not an
/// MCP operation: it binds a local authenticated controller lifetime, while
/// ordinary MCP/CLI requests retain their bounded explicit identity.
pub const RESIDENT_HOLDER_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidentHolderOperationV1 {
    Acquire,
    Renew,
    Release,
    Status,
    SubscribeLoss,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResidentControllerIdentityV1 {
    pub installation_id: String,
    pub cortex_store_id: String,
    pub release_generation: String,
    pub startup_generation: u64,
    pub stable_current: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResidentHolderCredentialV1 {
    pub holder_kind: String,
    pub holder_id: String,
    pub credential_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResidentHolderRequestV1 {
    pub schema_version: u32,
    pub operation: ResidentHolderOperationV1,
    pub controller: ResidentControllerIdentityV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<ResidentHolderCredentialV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_unix_ms: Option<u64>,
    pub observed_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss_cursor: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResidentHolderStatusV1 {
    pub controller_active: bool,
    /// True only after controller's full resident service set is healthy.
    pub services_ready: bool,
    /// Typed reason when a holder exists but resident work must stay withheld.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub services_unavailable_reason: Option<ResidentServicesUnavailableV1>,
    pub hub_holders: u32,
    pub coderight_daemon_holders: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidentServicesUnavailableV1 {
    BlueprintWatcherUnavailable,
    CatalogUnavailable,
    StoreUnavailable,
    Draining,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidentHolderLossKindV1 {
    FinalRelease,
    LeaseExpired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResidentHolderLossV1 {
    pub sequence: u64,
    pub kind: ResidentHolderLossKindV1,
    pub controller: ResidentControllerIdentityV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResidentHolderResponseV1 {
    pub schema_version: u32,
    pub operation: ResidentHolderOperationV1,
    pub controller: ResidentControllerIdentityV1,
    pub status: ResidentHolderStatusV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss: Option<ResidentHolderLossV1>,
}

// ---------------------------------------------------------------------------
// Lease protocol v2
// ---------------------------------------------------------------------------
//
// v2 is additive alongside the v1 resident-holder shapes above: it never
// replaces `ResidentHolderRequestV1` / `ResidentHolderResponseV1` /
// `ResidentControllerIdentityV1` / `ResidentHolderStatusV1` /
// `ResidentHolderLossV1` (the five preserved public V1 shapes). v2 introduces
// a server-MACed acquire permit plus a 128-bit-incarnation lease handle with
// idempotent replay and tombstoned closure.
pub const LEASE_PROTOCOL_SCHEMA_VERSION_V2: u32 = 2;

/// Maximum lifetime of an acquire permit, in milliseconds. A permit is a
/// server-MACed capability binding `generation` + `holder` + `incarnation`
/// that the caller redeems exactly once to open a lease incarnation.
pub const ACQUIRE_PERMIT_MAX_TTL_MS: u64 = 10_000;

/// Maximum server-computed lease TTL, in milliseconds. Every renew re-derives
/// a fresh TTL bounded by this ceiling; expiry is monotonic and never moves
/// backward relative to a prior grant for the same incarnation.
pub const LEASE_MAX_TTL_MS: u64 = 60_000;

/// Same ceiling applied to the idempotent logical-operation deadline and to
/// closed-incarnation tombstone retention.
pub const LOGICAL_OPERATION_MAX_DEADLINE_MS: u64 = 60_000;

/// Global cap on concurrently open (non-tombstoned) lease incarnations. A new
/// `acquire` at capacity is rejected outright; an in-flight acquire reserves
/// a tombstone slot up front so a subsequent `release` can never fail for
/// capacity reasons.
pub const LEASE_GLOBAL_CAPACITY: u32 = 64;

/// A 128-bit incarnation identifier. Serialized as lowercase hex (32 chars)
/// so it round-trips byte-identically across the wire and through JSON
/// fixtures; compared for equality as the raw `u128`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeaseIncarnationIdV2(pub u128);

impl LeaseIncarnationIdV2 {
    pub fn to_hex(self) -> String {
        format!("{:032x}", self.0)
    }

    pub fn from_hex(value: &str) -> Option<Self> {
        u128::from_str_radix(value, 16).ok().map(Self)
    }
}

impl Serialize for LeaseIncarnationIdV2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for LeaseIncarnationIdV2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::from_hex(&raw).ok_or_else(|| {
            serde::de::Error::custom("lease incarnation id must be 32 lowercase hex chars")
        })
    }
}

/// Server-MACed acquire permit. Valid for at most
/// [`ACQUIRE_PERMIT_MAX_TTL_MS`] from `issued_at_unix_ms`; redemption binds
/// exactly the `generation` + `holder` + `incarnation` triple the server
/// signed. A delayed (post-expiry) or already-redeemed permit is rejected as
/// stale and cannot mint a second incarnation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcquirePermitV2 {
    pub schema_version: u32,
    pub generation: u64,
    pub holder: ResidentHolderCredentialV1,
    pub incarnation: LeaseIncarnationIdV2,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    /// Base64 server MAC over `(generation, holder, incarnation, issued_at,
    /// expires_at)`; opaque to the client.
    pub mac: String,
}

/// A held lease incarnation. `sequence` is monotonic per incarnation and
/// strictly increases on every accepted renew; `expiry_unix_ms` is monotonic
/// non-decreasing and `authoritative_wall_time_unix_ms` is server time that
/// never rewinds relative to a prior response for the same incarnation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseHandleV2 {
    pub schema_version: u32,
    pub incarnation: LeaseIncarnationIdV2,
    pub operation_id: String,
    pub sequence: u64,
    pub ttl_ms: u64,
    pub expiry_unix_ms: u64,
    pub authoritative_wall_time_unix_ms: u64,
}

/// Outcome recorded for the first accepted logical operation under a given
/// `operation_id`. A fresh nonce carrying an identical canonical body before
/// `deadline_unix_ms` retransmits this same record; a changed body against a
/// live record is a conflict; a record observed after its deadline returns
/// `LeaseOperationRecordStateV2::OutcomeUnknown` and requires reconcile plus
/// a new permit/incarnation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseOperationRecordV2 {
    pub operation_id: String,
    pub incarnation: LeaseIncarnationIdV2,
    /// Digest of the canonical request body (e.g. lowercase hex sha256),
    /// bounded in length by the caller's canonicalization layer.
    pub canonical_body_digest: String,
    pub outcome: OperationResult,
    pub deadline_unix_ms: u64,
}

/// State returned when a caller replays or probes a logical operation
/// record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseOperationRecordStateV2 {
    /// Identical canonical body observed before the stored deadline: return
    /// the stored outcome unchanged.
    Retransmit,
    /// A different canonical body was submitted for the same `operation_id`
    /// while the prior record is still live.
    Conflict,
    /// The record's deadline has passed; the outcome is unknown and the
    /// caller must reconcile and acquire a new permit/incarnation.
    OutcomeUnknown,
}

/// Tombstone retained for a closed incarnation through the lesser of the
/// permit deadline and the logical-operation deadline, capped at
/// [`LOGICAL_OPERATION_MAX_DEADLINE_MS`]. While the tombstone is live, no
/// acquire, renew, or release may resurrect or otherwise touch the closed
/// incarnation; an unexpired tombstone for incarnation A is never overwritten
/// by a later incarnation B's tombstone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseTombstoneV2 {
    pub incarnation: LeaseIncarnationIdV2,
    pub generation: u64,
    pub closed_at_unix_ms: u64,
    pub retain_until_unix_ms: u64,
}

/// Typed reasons a v2 lease request is rejected as stale/expired rather than
/// applied. Distinguishes ordinary busy/backoff cases from replay-safety
/// rejections so callers never mistake a fenced retry for progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseRejectionV2 {
    /// Global capacity ([`LEASE_GLOBAL_CAPACITY`]) reached; only new
    /// `acquire` is rejected for capacity, never `release`.
    CapacityExceeded,
    /// The permit or lease has passed its own TTL/expiry.
    Expired,
    /// The request targets a tombstoned (already-closed) incarnation.
    Tombstoned,
    /// The request's generation/sequence is behind the authoritative state
    /// (a delayed duplicate of an earlier acquire/renew/release).
    Stale,
    /// The permit's MAC does not verify against the claimed fields.
    PermitInvalid,
}

/// The success output of a Membrane MCP operation.
///
/// We intentionally do not model the per-operation data shape here — the
/// canonical `data` payload is validated against the per-operation JSON
/// Schema's `#/$defs/success` branch. The Rust side just preserves whatever
/// `data` value the caller (or the fixture) carried, so the round-trip
/// assertion stays byte-identical to the fixture's canonical form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SuccessResult {
    pub kind: ResultKind,
    pub data: serde_json::Value,
}

/// The typed error envelope of a Membrane MCP operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErrorResult {
    pub kind: ResultKind,
    /// Closed, stable error code drawn from this operation's taxonomy.
    pub code: String,
    /// Human-readable message (one line).
    pub message: String,
    /// Whether a caller may retry the same input.
    pub retryable: bool,
    /// Optional operation-specific details. The contract is closed at the
    /// `kind`/`code`/`message`/`retryable` layer; `details` is an open map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// The discriminated union every MCP tool returns: either a success payload
/// or a typed error envelope. Persisted and re-emitted byte-for-byte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum OperationResult {
    Success(SuccessResult),
    Error(ErrorResult),
}

/// One operation response envelope.
///
/// `schemaVersion` is the INDEPENDENT contract version of this operation.
/// `errorVersion` is the INDEPENDENT error-taxonomy version. The two advance
/// separately: tightening an error code set does not move the contract's
/// `schemaVersion`; adding a new output field does not move `errorVersion`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationEnvelope {
    pub schema_version: u32,
    pub operation: String,
    pub error_version: u32,
    pub result: OperationResult,
}

/// One registry-owned CLI parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationParameter {
    pub name: &'static str,
    pub default: Option<&'static str>,
    pub help: &'static str,
}

/// The CLI-facing projection of one operation in the canonical registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSpec {
    pub id: &'static str,
    pub help: &'static str,
    pub parameters: &'static [OperationParameter],
}

/// One operation's index entry. Mirrors
/// `schemas/operations/operations-index.v1.schema.json#/properties/operations/items`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationIndexEntry {
    pub name: String,
    pub schema_version: u32,
    pub error_version: u32,
    pub schema_path: String,
    pub success_fixture: String,
    pub error_fixture: String,
    /// Sorted list of every typed error code this operation defines. We
    /// store the index as `Vec<String>` (preserving the on-disk ordering)
    /// but assertions compare against a `BTreeSet` for closed-set
    /// equivalence.
    pub error_codes: Vec<String>,
}

/// The cross-operation registry. This is the only place that observes the
/// whole set of operations together; everything else is per-operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationsIndex {
    pub schema_version: u32,
    pub index_version: u32,
    pub operations: Vec<OperationIndexEntry>,
}

impl OperationsIndex {
    /// Every operation name, in registry order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.operations.iter().map(|entry| entry.name.as_str())
    }

    /// The closed error-code set for one operation (or empty if unknown).
    pub fn error_codes_for(&self, name: &str) -> BTreeSet<String> {
        self.operations
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.error_codes.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// Build a single [`OperationIndexEntry`] with `String` fields. A tiny
/// helper so the `operations()` builder below stays readable.
fn entry(
    name: &str,
    schema_version: u32,
    error_version: u32,
    schema_path: &str,
    success_fixture: &str,
    error_fixture: &str,
    error_codes: &[&str],
) -> OperationIndexEntry {
    OperationIndexEntry {
        name: name.to_string(),
        schema_version,
        error_version,
        schema_path: schema_path.to_string(),
        success_fixture: success_fixture.to_string(),
        error_fixture: error_fixture.to_string(),
        error_codes: error_codes.iter().map(|code| (*code).to_string()).collect(),
    }
}

/// The canonical cross-operation registry, in stable order. The TS binding
/// (`bindings/operations.mjs`) mirrors this list; both sides are required
/// to expose the SAME operations in the SAME order so the index-round-trip
/// test can pin a single digest.
pub fn operations() -> Vec<OperationIndexEntry> {
    vec![
        entry(
            "membrane_context",
            1,
            1,
            "schemas/operations/membrane-context.v1.schema.json",
            "schemas/registry/operations/membrane-context.v1.golden.json",
            "schemas/registry/operations/membrane-context.v1.error.golden.json",
            &[
                "context_unavailable",
                "context_scope_denied",
                "context_workspace_no_repos",
                "context_workspace_abstained",
                "context_deadline_exceeded",
                "context_caller_scope_binding_denied",
                "context_caller_not_authorized",
                "context_cross_root_denied",
                "context_target_denied",
                "context_envelope_invalid",
            ],
        ),
        entry(
            "membrane_source_read",
            1,
            1,
            "schemas/operations/membrane-source-read.v1.schema.json",
            "schemas/registry/operations/membrane-source-read.v1.golden.json",
            "schemas/registry/operations/membrane-source-read.v1.error.golden.json",
            &[
                "source_read_unavailable",
                "source_read_hash_mismatch",
                "source_read_anchor_missing",
                "source_read_scope_denied",
                "source_read_envelope_invalid",
            ],
        ),
        entry(
            "membrane_blueprint",
            1,
            1,
            "schemas/operations/membrane-blueprint.v1.schema.json",
            "schemas/registry/operations/membrane-blueprint.v1.golden.json",
            "schemas/registry/operations/membrane-blueprint.v1.error.golden.json",
            &[
                "blueprint_unavailable",
                "blueprint_envelope_invalid",
                "blueprint_caller_scope_binding_denied",
                "blueprint_caller_not_authorized",
                "blueprint_cross_root_denied",
                "blueprint_batch_invalid",
            ],
        ),
        entry(
            "membrane_knowledge_propose",
            2,
            2,
            "schemas/operations/membrane-knowledge-propose.v2.schema.json",
            "schemas/registry/operations/membrane-knowledge-propose.v2.golden.json",
            "schemas/registry/operations/membrane-knowledge-propose.v2.error.golden.json",
            &[
                "proposal_emission_text_required",
                "proposal_payload_too_large",
                "proposal_rate_limited",
                "proposal_binding_unresolvable",
                "proposal_durable_write_failed",
                "proposal_scope_denied",
                "producer_denied",
                "dlp_denied",
                "epistemic_denied",
                "identity_conflict",
                "caller_required",
                "installation_grant_denied",
                "repository_scope_chain_denied",
                "caller_scope_binding_denied",
                "caller_not_authorized",
                "cross_root_binding_denied",
                "authorization_revoked",
                "context_unavailable",
                "cortex_storage_unavailable",
                "proposal_review_unknown",
                "proposal_already_decided",
                "cortex_review_unavailable",
                "cortex_review_binding_denied",
                "cortex_review_invalid",
                "cortex_review_denied",
                "cortex_review_replay_conflict",
                "cortex_review_version_conflict",
                "cortex_control_version_conflict",
                "memory_unavailable",
                "memory_version_conflict",
                "memory_envelope_invalid",
                "memory_ineligible",
                "checkpoint_scope_denied",
                "temporal_admission_requires_policy",
                "proposal_envelope_invalid",
            ],
        ),
        entry(
            "membrane_checkpoint_save",
            1,
            1,
            "schemas/operations/membrane-checkpoint-save.v1.schema.json",
            "schemas/registry/operations/membrane-checkpoint-save.v1.golden.json",
            "schemas/registry/operations/membrane-checkpoint-save.v1.error.golden.json",
            &[
                "checkpoint_payload_too_large",
                "checkpoint_rate_limited",
                "checkpoint_scope_denied",
                "checkpoint_save_unavailable",
                "checkpoint_envelope_invalid",
            ],
        ),
        entry(
            "membrane_checkpoint_load",
            1,
            1,
            "schemas/operations/membrane-checkpoint-load.v1.schema.json",
            "schemas/registry/operations/membrane-checkpoint-load.v1.golden.json",
            "schemas/registry/operations/membrane-checkpoint-load.v1.error.golden.json",
            &[
                "checkpoint_not_found",
                "checkpoint_expired",
                "checkpoint_scope_denied",
                "checkpoint_load_unavailable",
                "checkpoint_envelope_invalid",
            ],
        ),
        entry(
            "membrane_working_context",
            1,
            1,
            "schemas/operations/membrane-working-context.v1.schema.json",
            "schemas/registry/operations/membrane-working-context.v1.golden.json",
            "schemas/registry/operations/membrane-working-context.v1.error.golden.json",
            &[
                "working_context_payload_too_large",
                "working_context_rate_limited",
                "working_context_scope_required",
                "working_context_id_required",
                "working_context_operation_invalid",
                "working_context_scope_denied",
                "working_context_envelope_invalid",
            ],
        ),
        entry(
            "membrane_temporal_fact",
            2,
            2,
            "schemas/operations/membrane-temporal-fact.v2.schema.json",
            "schemas/registry/operations/membrane-temporal-fact.v2.golden.json",
            "schemas/registry/operations/membrane-temporal-fact.v2.error.golden.json",
            &[
                "temporal_fact_payload_too_large",
                "temporal_fact_scope_denied",
                "temporal_fact_scope_mismatch",
                "temporal_fact_query_invalid",
                "temporal_fact_operation_invalid",
                "temporal_fact_envelope_invalid",
                "caller_required",
                "installation_grant_denied",
                "repository_scope_chain_denied",
                "caller_scope_binding_denied",
                "caller_not_authorized",
                "cross_root_binding_denied",
                "authorization_revoked",
                "context_unavailable",
                "cortex_storage_unavailable",
                "proposal_payload_too_large",
                "proposal_emission_text_required",
                "proposal_scope_denied",
                "dlp_denied",
                "proposal_review_unknown",
                "proposal_already_decided",
                "cortex_review_unavailable",
                "cortex_review_binding_denied",
                "cortex_review_invalid",
                "cortex_review_denied",
                "cortex_review_replay_conflict",
                "cortex_review_version_conflict",
                "cortex_control_version_conflict",
                "memory_unavailable",
                "memory_version_conflict",
                "memory_envelope_invalid",
                "memory_ineligible",
                "checkpoint_scope_denied",
                "temporal_admission_requires_policy",
                "temporal_fact_invalid",
            ],
        ),
        entry(
            "membrane_scratchpad",
            1,
            1,
            "schemas/operations/membrane-scratchpad.v1.schema.json",
            "schemas/registry/operations/membrane-scratchpad.v1.golden.json",
            "schemas/registry/operations/membrane-scratchpad.v1.error.golden.json",
            &[
                "scratchpad_payload_too_large",
                "scratchpad_scope_required",
                "scratchpad_scope_denied",
                "scratchpad_operation_invalid",
                "scratchpad_envelope_invalid",
            ],
        ),
        entry(
            "membrane_feedback",
            1,
            1,
            "schemas/operations/membrane-feedback.v1.schema.json",
            "schemas/registry/operations/membrane-feedback.v1.golden.json",
            "schemas/registry/operations/membrane-feedback.v1.error.golden.json",
            &[
                "feedback_invalid",
                "feedback_invalid_verdict_ref",
                "feedback_payload_too_large",
                "feedback_rate_limited",
                "feedback_binding_unresolvable",
                "feedback_durable_write_failed",
                "feedback_independent_readback_mismatch",
            ],
        ),
        entry(
            "membrane_memory",
            1,
            1,
            "schemas/operations/membrane-memory.v1.schema.json",
            "schemas/registry/operations/membrane-memory.v1.golden.json",
            "schemas/registry/operations/membrane-memory.v1.error.golden.json",
            &[
                "caller_required",
                "installation_grant_denied",
                "repository_scope_chain_denied",
                "caller_scope_binding_denied",
                "caller_not_authorized",
                "cross_root_binding_denied",
                "authorization_revoked",
                "context_unavailable",
                "cortex_storage_unavailable",
                "proposal_payload_too_large",
                "proposal_emission_text_required",
                "proposal_scope_denied",
                "dlp_denied",
                "proposal_review_unknown",
                "proposal_already_decided",
                "cortex_review_unavailable",
                "cortex_review_binding_denied",
                "cortex_review_invalid",
                "cortex_review_denied",
                "cortex_review_replay_conflict",
                "cortex_review_version_conflict",
                "cortex_control_version_conflict",
                "memory_unavailable",
                "memory_version_conflict",
                "memory_envelope_invalid",
                "memory_ineligible",
                "memory_recipe_invalid",
                "memory_recipe_unsupported",
                "checkpoint_scope_denied",
                "temporal_admission_requires_policy",
            ],
        ),
        entry(
            "membrane_memory_read",
            1,
            1,
            "schemas/operations/membrane-memory-read.v1.schema.json",
            "schemas/registry/operations/membrane-memory-read.v1.golden.json",
            "schemas/registry/operations/membrane-memory-read.v1.error.golden.json",
            &[
                "caller_required",
                "installation_grant_denied",
                "repository_scope_chain_denied",
                "caller_scope_binding_denied",
                "caller_not_authorized",
                "cross_root_binding_denied",
                "authorization_revoked",
                "context_unavailable",
                "cortex_storage_unavailable",
                "memory_unavailable",
                "memory_version_conflict",
                "memory_envelope_invalid",
                "memory_ineligible",
            ],
        ),
        entry(
            "membrane_knowledge_review",
            1,
            1,
            "schemas/operations/membrane-knowledge-review.v1.schema.json",
            "schemas/registry/operations/membrane-knowledge-review.v1.golden.json",
            "schemas/registry/operations/membrane-knowledge-review.v1.error.golden.json",
            &[
                "caller_required",
                "installation_grant_denied",
                "repository_scope_chain_denied",
                "caller_scope_binding_denied",
                "caller_not_authorized",
                "cross_root_binding_denied",
                "authorization_revoked",
                "context_unavailable",
                "cortex_storage_unavailable",
                "proposal_payload_too_large",
                "proposal_emission_text_required",
                "proposal_scope_denied",
                "proposal_review_unknown",
                "proposal_already_decided",
                "cortex_review_unavailable",
                "cortex_review_binding_denied",
                "cortex_review_invalid",
                "cortex_review_denied",
                "cortex_review_replay_conflict",
                "cortex_review_version_conflict",
                "cortex_control_version_conflict",
                "memory_unavailable",
                "memory_version_conflict",
                "memory_envelope_invalid",
                "memory_ineligible",
                "checkpoint_scope_denied",
                "temporal_admission_requires_policy",
            ],
        ),
        entry(
            "hub.capabilities",
            1,
            1,
            "schemas/operations/hub-capabilities.v1.schema.json",
            "schemas/registry/operations/hub-capabilities.v1.golden.json",
            "schemas/registry/operations/hub-capabilities.v1.error.golden.json",
            &["hub_unavailable"],
        ),
        entry(
            "hub.snapshot",
            1,
            1,
            "schemas/operations/hub-snapshot.v1.schema.json",
            "schemas/registry/operations/hub-snapshot.v1.golden.json",
            "schemas/registry/operations/hub-snapshot.v1.error.golden.json",
            &["hub_unavailable"],
        ),
    ]
}

/// Cached index entries retained for schema/fixture compatibility.
static OPERATION_INDEX_ENTRIES: std::sync::OnceLock<Vec<OperationIndexEntry>> =
    std::sync::OnceLock::new();

/// The typed operation registry consumed by generated CLI surfaces.
pub static OPERATIONS: &[OperationSpec] = &[
    OperationSpec {
        id: "membrane_context",
        help: "Federated context packet for one exact caller binding.",
        parameters: &[],
    },
    OperationSpec {
        id: "membrane_source_read",
        help: "Hash-bound DocReadV1 section fetch for one exact caller binding.",
        parameters: &[],
    },
    OperationSpec {
        id: "membrane_blueprint",
        help: "Read repository-truth evidence from Blueprint for one exact caller binding.",
        parameters: &[],
    },
    OperationSpec {
        id: "membrane_knowledge_propose",
        help: "Submit a bounded typed KnowledgeEmission proposal for quarantine review.",
        parameters: &[],
    },
    OperationSpec {
        id: "membrane_checkpoint_save",
        help: "Save an A0 session checkpoint for one exact caller binding; never durable knowledge.",
        parameters: &[],
    },
    OperationSpec {
        id: "membrane_checkpoint_load",
        help: "Load an unexpired A0 session checkpoint for one exact caller binding.",
        parameters: &[],
    },
    OperationSpec {
        id: "membrane_working_context",
        help: "Save, load, or close bounded session/task working context; durability must be explicit.",
        parameters: &[],
    },
    OperationSpec {
        id: "membrane_temporal_fact",
        help: "Record or query provenance-bound temporal facts with explicit single-valued predicate policy.",
        parameters: &[],
    },
    OperationSpec {
        id: "membrane_scratchpad",
        help: "Save, load, or clear ephemeral non-searchable session/task scratchpad state.",
        parameters: &[],
    },
    OperationSpec {
        id: "membrane_feedback",
        help: "Record bounded receipt-bound outcome feedback for quarantine review.",
        parameters: &[],
    },
    OperationSpec { id: "membrane_memory", help: "Resolve exact bounded memory or inspect and promote pending knowledge.", parameters: &[] },
    OperationSpec { id: "membrane_memory_read", help: "Resolve exact bounded Cortex memory through the read-only compatibility operation.", parameters: &[] },
    OperationSpec { id: "membrane_knowledge_review", help: "Apply an independently signed, exact-target reviewed effect.", parameters: &[] },
    OperationSpec { id: "hub.capabilities", help: "Read-only Hub capability manifest.", parameters: &[] },
    OperationSpec { id: "hub.snapshot", help: "Read-only Hub status snapshot.", parameters: &[] },
];

/// Helper: get the canonical index entries (initializes the cache on first call).
pub fn operations_slice() -> &'static [OperationIndexEntry] {
    OPERATION_INDEX_ENTRIES.get_or_init(operations).as_slice()
}

/// Native response versions preserve independent operation contracts.
pub fn operation_versions(name: &str) -> (u32, u32) {
    operations_slice()
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| (entry.schema_version, entry.error_version))
        .unwrap_or((1, 1))
}
