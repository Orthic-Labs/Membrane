//! Reference Cortex adapter built on `membrane-provider-sdk`.
//!
//! This crate is a **minimal but real** example of how a Cortex-shaped
//! adapter is written against the Membrane provider SDK. It implements
//! `membrane_provider_sdk::Provider` for two operations:
//!
//!   * `membrane_context`     — returns a workspace context with an active
//!                              scope grant.
//!   * `membrane_source_read` — returns a source-read envelope for one
//!                              anchor section.
//!
//! The shapes mirror the per-operation JSON Schemas and golden fixtures
//! under `engine/crates/membrane-protocol/operations/`. This adapter is
//! deterministic: given a request envelope, it always returns the SAME
//! response (the one the corresponding `membrane-testkit` fixture
//! declares). The conformance test in `membrane-provider-sdk/tests/`
//! exercises this property via `run_conformance`.
//!
//! The adapter is intentionally pure (no I/O, no clock, no randomness).
//! Real Cortex adapters will read the cortex graph and the scope-grant
//! store, but those reads are out of scope for the conformance adapter
//! — the SDK is the contract layer, not the implementation layer.

use membrane_provider_sdk::{
    run_conformance, CapabilityV1, ConformanceReport, Fixture, Provider, ProviderError, Result,
};
use membrane_testkit::cortex_fixtures;
use serde_json::{json, Value};

/// The set of operations the reference Cortex adapter advertises.
pub const CORTEX_OPERATIONS: &[&str] = &["membrane_context", "membrane_source_read"];

/// The reference Cortex adapter.
///
/// The struct is `pub` so the conformance test in
/// `membrane-provider-sdk/tests/conformance.rs` can construct it
/// directly.
pub struct CortexExample {
    /// `true` after a successful `initialize`.
    ready: bool,
}

impl CortexExample {
    /// Construct an un-initialized adapter. Call `initialize` before any
    /// other method.
    pub fn new() -> Self {
        Self { ready: false }
    }
}

impl Default for CortexExample {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for CortexExample {
    fn initialize(&mut self, _config: &Value) -> Result<()> {
        self.ready = true;
        Ok(())
    }

    fn list_capabilities(&self) -> Vec<CapabilityV1> {
        CORTEX_OPERATIONS
            .iter()
            .map(|name| CapabilityV1 {
                name: (*name).to_string(),
                schema_version: 1,
                error_version: 1,
            })
            .collect()
    }

    fn handle_operation(&self, operation: &str, request: &Value) -> Result<Value> {
        if !self.ready {
            return Err(ProviderError::Uninitialized("cortex_example".into()));
        }
        match operation {
            "membrane_context" => Ok(handle_membrane_context(request)),
            "membrane_source_read" => Ok(handle_membrane_source_read(request)),
            other => Err(ProviderError::UnknownOperation(other.to_string())),
        }
    }
}

/// `membrane_context` — workspace context with an active scope grant.
fn handle_membrane_context(request: &Value) -> Value {
    // Pull the task/turn/client envelopes out of the request and re-emit
    // them in the response. The reference adapter is deterministic; the
    // shape mirrors operations/operations/membrane-context.v1.golden.json.
    let task_envelope = request
        .get("taskEnvelope")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let turn_envelope = request
        .get("turnEnvelope")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let client_envelope = request
        .get("clientEnvelope")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let scope = request
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("workspace");
    let session_id = turn_envelope
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("session-mbr-104-cortex");

    json!({
        "kind": "success",
        "data": {
            "ok": true,
            "scope": scope,
            "packet": { "status": "available" },
            "scopeGrant": {
                "id": "sgv1-mbr-104-cortex",
                "status": "active"
            },
            "taskEnvelope": task_envelope,
            "turnEnvelope": turn_envelope,
            "clientEnvelope": client_envelope,
            "overlay": {
                "schema": "orthic.overlay-identity.v1",
                "sessionId": session_id,
                "worktreePath": "/Volumes/D/claude/.worktrees/membrane-book1"
            },
            "repos": [
                { "repoId": "membrane", "basis": "admitted", "candidates": 1, "omissions": [] }
            ],
            "totalCandidates": 1,
            "totalOmissions": 0,
            "deadlineExceeded": false
        }
    })
}

/// `membrane_source_read` — one anchor section.
fn handle_membrane_source_read(request: &Value) -> Value {
    let source_ref = request
        .get("sourceRef")
        .and_then(Value::as_str)
        .unwrap_or("");
    let anchor_id = request
        .get("anchorId")
        .and_then(Value::as_str)
        .unwrap_or("anchor-mbr-104-cortex");
    json!({
        "kind": "success",
        "data": {
            "ok": true,
            "contentSha256": "sha256:000000000000000000000000000000000000000000000000000000000000a401",
            "section": {
                "anchorId": anchor_id,
                "startLine": 1,
                "endLine": 60
            },
            "sourceRef": source_ref
        }
    })
}

/// Run the reference Cortex adapter against the canonical cortex fixture
/// set from `membrane-testkit`. Convenience for downstream callers
/// (e.g. the conformance test in `membrane-provider-sdk/tests/`).
pub fn run_cortex_conformance(provider: &CortexExample) -> ConformanceReport {
    let fixtures: Vec<Fixture> = cortex_fixtures();
    run_conformance(provider, &fixtures)
}
