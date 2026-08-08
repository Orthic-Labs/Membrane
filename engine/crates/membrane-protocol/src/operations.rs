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

/// The discriminator for an operation's response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultKind {
    Success,
    Error,
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
/// to expose the SAME 10 operations in the SAME order so the index-round-trip
/// test can pin a single digest.
pub fn operations() -> Vec<OperationIndexEntry> {
    vec![
        entry(
            "membrane_context",
            1,
            1,
            "schemas/operations/membrane-context.v1.schema.json",
            "operations/operations/membrane-context.v1.golden.json",
            "operations/operations/membrane-context.v1.error.golden.json",
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
            "operations/operations/membrane-source-read.v1.golden.json",
            "operations/operations/membrane-source-read.v1.error.golden.json",
            &[
                "source_read_unavailable",
                "source_read_hash_mismatch",
                "source_read_anchor_missing",
                "source_read_scope_denied",
                "source_read_envelope_invalid",
            ],
        ),
        entry(
            "membrane_cortex",
            1,
            1,
            "schemas/operations/membrane-cortex.v1.schema.json",
            "operations/operations/membrane-cortex.v1.golden.json",
            "operations/operations/membrane-cortex.v1.error.golden.json",
            &[
                "cortex_unavailable",
                "cortex_envelope_invalid",
                "cortex_caller_scope_binding_denied",
                "cortex_caller_not_authorized",
                "cortex_cross_root_denied",
                "cortex_batch_invalid",
            ],
        ),
        entry(
            "membrane_knowledge_propose",
            1,
            1,
            "schemas/operations/membrane-knowledge-propose.v1.schema.json",
            "operations/operations/membrane-knowledge-propose.v1.golden.json",
            "operations/operations/membrane-knowledge-propose.v1.error.golden.json",
            &[
                "proposal_emission_text_required",
                "proposal_payload_too_large",
                "proposal_rate_limited",
                "proposal_binding_unresolvable",
                "proposal_durable_write_failed",
                "proposal_scope_denied",
            ],
        ),
        entry(
            "membrane_checkpoint_save",
            1,
            1,
            "schemas/operations/membrane-checkpoint-save.v1.schema.json",
            "operations/operations/membrane-checkpoint-save.v1.golden.json",
            "operations/operations/membrane-checkpoint-save.v1.error.golden.json",
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
            "operations/operations/membrane-checkpoint-load.v1.golden.json",
            "operations/operations/membrane-checkpoint-load.v1.error.golden.json",
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
            "operations/operations/membrane-working-context.v1.golden.json",
            "operations/operations/membrane-working-context.v1.error.golden.json",
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
            1,
            1,
            "schemas/operations/membrane-temporal-fact.v1.schema.json",
            "operations/operations/membrane-temporal-fact.v1.golden.json",
            "operations/operations/membrane-temporal-fact.v1.error.golden.json",
            &[
                "temporal_fact_payload_too_large",
                "temporal_fact_scope_denied",
                "temporal_fact_scope_mismatch",
                "temporal_fact_query_invalid",
                "temporal_fact_operation_invalid",
                "temporal_fact_envelope_invalid",
            ],
        ),
        entry(
            "membrane_scratchpad",
            1,
            1,
            "schemas/operations/membrane-scratchpad.v1.schema.json",
            "operations/operations/membrane-scratchpad.v1.golden.json",
            "operations/operations/membrane-scratchpad.v1.error.golden.json",
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
            "operations/operations/membrane-feedback.v1.golden.json",
            "operations/operations/membrane-feedback.v1.error.golden.json",
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
            "hub.capabilities",
            1,
            1,
            "schemas/operations/hub-capabilities.v1.schema.json",
            "operations/operations/hub-capabilities.v1.golden.json",
            "operations/operations/hub-capabilities.v1.error.golden.json",
            &["hub_unavailable"],
        ),
        entry(
            "hub.snapshot",
            1,
            1,
            "schemas/operations/hub-snapshot.v1.schema.json",
            "operations/operations/hub-snapshot.v1.golden.json",
            "operations/operations/hub-snapshot.v1.error.golden.json",
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
    OperationSpec { id: "hub.capabilities", help: "Read-only Hub capability manifest.", parameters: &[] },
    OperationSpec { id: "hub.snapshot", help: "Read-only Hub status snapshot.", parameters: &[] },
];

/// Helper: get the canonical index entries (initializes the cache on first call).
pub fn operations_slice() -> &'static [OperationIndexEntry] {
    OPERATION_INDEX_ENTRIES.get_or_init(operations).as_slice()
}
