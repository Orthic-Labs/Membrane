//! Native port of `blueprint/src/lib/application/errors.mjs`.
//!
//! Error metadata grounded in the codes thrown by lib/application/** and the
//! reasonCode values emitted by lib/admission.mjs. Lookup is additive: legacy
//! fields (code/message/details) are preserved untouched and retryable plus
//! remediation are attached when the code is known.
//!
//! `retryable`      -- whether re-running the operation can succeed without user edits.
//! `summary`        -- actionable one-line explanation of the failure.
//! `next_operation` -- the concrete next command/action, or `None` for no-op codes.
//!
//! NOTE ON SCOPE: this is a standalone faithful port of the *code -> metadata*
//! lookup table and `BlueprintError`/`fail` shape from the legacy module. It
//! is intentionally independent of `crate::api::BlueprintError`, which is
//! the unrelated native API-boundary error type used by engine.rs/query.rs/
//! findings.rs and covers a different (mostly disjoint) code surface -- see
//! `crate::api::BlueprintError` for that one. Nothing in this lane's scope
//! wires this legacy-shaped error type into the native dispatch path; it
//! exists to prove behavioral parity of the standalone lib/application/
//! errors.mjs module, one of this lane's 12 assigned legacy files.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy)]
pub struct ErrorMetadata {
    pub retryable: bool,
    pub summary: Option<&'static str>,
    pub next_operation: Option<&'static str>,
}

const fn meta(retryable: bool, summary: Option<&'static str>, next_operation: Option<&'static str>) -> ErrorMetadata {
    ErrorMetadata { retryable, summary, next_operation }
}

static ERROR_METADATA: LazyLock<BTreeMap<&'static str, ErrorMetadata>> = LazyLock::new(|| {
    BTreeMap::from([
        // --- lib/application/** thrown codes ---
        ("request_cancelled", meta(false, None, None)),
        ("root_not_enrolled", meta(false, Some("The requested root is not enrolled; enroll it before querying."), Some("blueprint init"))),
        ("graph_missing", meta(true, Some("No graph store exists for this repository; build the graph to enable queries."), Some("blueprint build"))),
        ("schema_mismatch", meta(true, Some("The sealed Blueprint generation does not match the current schema; rebuild it."), Some("blueprint build"))),
        ("service_unavailable", meta(false, Some("The requested Blueprint service operation is unavailable."), None)),
        ("stale_blocked", meta(true, Some("Recall against the current generation, or pass allowStale to accept known-stale evidence."), Some("blueprint_recall"))),
        ("generation_mismatch", meta(true, Some("The graph advanced after recall; recall to obtain a current receipt."), Some("blueprint_recall"))),
        ("anchor_not_found", meta(false, Some("No graph node matches the anchor; use an exact node id, name, path, or file:/symbol: reference."), Some("re-run with a valid anchor"))),
        ("anchor_ambiguous", meta(false, Some("The anchor matches multiple nodes; narrow it to a qualified name or exact node id."), Some("re-run with a more specific anchor"))),
        ("node_not_found", meta(false, Some("No graph node matches the nodeId in the current generation; verify the identifier."), Some("re-run with a valid nodeId"))),
        ("root_escape", meta(false, None, None)),
        ("query_required", meta(false, Some("A non-empty query is required; pass query or task."), Some("re-run with a non-empty query"))),
        ("anchor_required", meta(false, Some("An anchor is required; pass a path, symbol, node id, or query."), Some("re-run with an anchor"))),
        // --- lib/admission.mjs reasonCode values ---
        ("missing_graph", meta(true, Some("No complete graph generation is available for recall; build the graph first."), Some("blueprint build"))),
        ("missing_generation", meta(true, Some("No sealed generation exists; build the graph before querying."), Some("blueprint build"))),
        ("receipt_reuse", meta(false, None, None)),
        ("recalled", meta(false, None, None)),
        ("recalled_stale", meta(true, Some("Recall established under stale graph state; rebuild to refresh the generation."), Some("blueprint build"))),
        ("stale_generation_withheld", meta(true, Some("Stale-source enumeration is incomplete, so every source-backed row is withheld; rebuild to serve evidence again."), Some("blueprint build"))),
        ("recalled_changed_since_generation", meta(true, Some("Recall served under a generation that predates current worktree changes; suppressed sources are named on the receipt."), Some("blueprint build"))),
        ("no_candidates", meta(false, Some("Recall resolved no evidence paths for this task."), None)),
        ("recalled_indeterminate", meta(true, Some("Recall established under indeterminate graph state; rebuild to get a determinate generation."), Some("blueprint build"))),
        ("missing_receipt_id", meta(false, Some("expand/revoke requires a receiptId; call recall first and pass the returned receiptId."), Some("blueprint recall"))),
        ("receipt_not_found", meta(false, Some("No receipt matches the receiptId; call recall to establish one."), Some("blueprint recall"))),
        ("receipt_revoked", meta(false, Some("Receipt is revoked; recall with force=true or a new session/task."), Some("blueprint recall"))),
        ("absolute_path_rejected", meta(false, Some("expand rejects absolute self-approved paths; pass a relative repo path or graph query."), Some("re-run expand with a relative path"))),
        ("missing_expand_query", meta(false, Some("expand requires a path, symbol, or query grounded in the graph."), Some("re-run expand with path, symbol, or query"))),
        ("generation_changed", meta(true, Some("Graph generation changed after recall; recall against the current generation before expanding."), Some("blueprint recall"))),
        ("expanded", meta(false, None, None)),
        ("no_receipt", meta(false, Some("No recall receipt found for the active session/task/repo; call recall."), Some("blueprint recall"))),
        ("receipt_active", meta(false, None, None)),
        ("already_revoked", meta(false, None, None)),
        ("revoked", meta(false, Some("Recall receipt revoked; call recall before further recall-dependent work."), Some("blueprint recall"))),
    ])
});

/// Remediation attached to a `BlueprintError` when the code is known and no
/// explicit `details.remediation` was supplied.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Remediation {
    pub summary: String,
    pub next_operation: String,
    pub arguments: serde_json::Value,
}

/// Faithful port of legacy `class BlueprintError extends Error`.
#[derive(Debug, Clone)]
pub struct BlueprintError {
    pub name: &'static str,
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
    pub retryable: bool,
    pub remediation: Option<Remediation>,
}

impl BlueprintError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, details: Option<serde_json::Value>) -> Self {
        let code = code.into();
        let details = details.unwrap_or_else(|| serde_json::json!({}));
        let meta = ERROR_METADATA.get(code.as_str());
        let retryable = meta.map(|m| m.retryable).unwrap_or(false);
        let explicit_remediation = details.get("remediation").filter(|v| !v.is_null());
        let remediation = if let Some(explicit) = explicit_remediation {
            serde_json::from_value(explicit.clone()).ok()
        } else {
            meta.and_then(|m| m.next_operation.map(|next_operation| Remediation {
                summary: m.summary.unwrap_or_default().to_owned(),
                next_operation: next_operation.to_owned(),
                arguments: serde_json::json!({}),
            }))
        };
        Self { name: "BlueprintError", code, message: message.into(), details, retryable, remediation }
    }
}

impl fmt::Display for BlueprintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for BlueprintError {}

/// Mirrors `export function fail(code, message, details) { throw new BlueprintError(...); }`.
pub fn fail(code: impl Into<String>, message: impl Into<String>, details: Option<serde_json::Value>) -> Result<std::convert::Infallible, BlueprintError> {
    Err(BlueprintError::new(code, message, details))
}
