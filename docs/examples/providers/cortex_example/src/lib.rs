//! Reference Cortex adapter built on `membrane-provider-sdk`.
//!
//! This crate is a **minimal but real** example of how a Cortex-shaped
//! adapter is written against the Membrane provider SDK. It implements
//! `membrane_provider_sdk::Provider` for two operations:
//!
//!   * `membrane_knowledge_propose` — records a knowledge proposal and
//!                                     returns its durable proposal id.
//!   * `membrane_checkpoint_save`   — saves a checkpoint and returns
//!                                     its durable checkpoint id.
//!
//! The shapes mirror the per-operation JSON Schemas and golden fixtures
//! under `engine/crates/membrane-protocol/operations/`. This adapter is
//! deterministic: given a request envelope, it always returns the SAME
//! response (the one the corresponding `membrane-testkit` fixture
//! declares). The conformance test in `membrane-provider-sdk/tests/`
//! exercises this property via `run_conformance`.
//!
//! The adapter is intentionally pure (no I/O, no clock, no randomness).
//! Real Cortex adapters will write to the durable `MemoryStore`, but those
//! writes are out of scope for the conformance adapter — the SDK is the
//! contract layer, not the implementation layer.

use membrane_provider_sdk::provider::{
    ProviderIdentityV1, ProviderObservationV1, ProviderReadinessStateV1, ProviderReadinessV1,
    ProviderTestQueryV1, PROVIDER_READINESS_SCHEMA_VERSION,
};
use membrane_provider_sdk::{
    run_conformance, CapabilityV1, ConformanceReport, Fixture, Provider, ProviderError, Result,
};
use membrane_testkit::cortex_fixtures;
use serde_json::{json, Value};

/// The set of operations the reference Cortex adapter advertises.
pub const CORTEX_OPERATIONS: &[&str] = &["membrane_knowledge_propose", "membrane_checkpoint_save"];

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

    fn readiness(&self) -> ProviderReadinessV1 {
        let identity = ProviderIdentityV1 {
            provider_id: "cortex-example".into(),
            installation_id: "example-installation".into(),
            service_id: "cortex-example-service".into(),
            release_generation: "example-v1".into(),
            data_root_digest: "sha256:example-root".into(),
        };
        if !self.ready {
            return ProviderReadinessV1::unknown(identity, "not_initialized");
        }
        ProviderReadinessV1 {
            schema_version: PROVIDER_READINESS_SCHEMA_VERSION,
            state: ProviderReadinessStateV1::Ready,
            identity,
            observation: Some(ProviderObservationV1 {
                observed_at_unix_ms: 1,
                fresh_for_ms: 60_000,
            }),
            test_query: Some(ProviderTestQueryV1 {
                name: "checkpoint-smoke".into(),
                succeeded: true,
            }),
            reason: "authoritative_test_passed".into(),
        }
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
            "membrane_knowledge_propose" => Ok(handle_membrane_knowledge_propose(request)),
            "membrane_checkpoint_save" => Ok(handle_membrane_checkpoint_save(request)),
            other => Err(ProviderError::UnknownOperation(other.to_string())),
        }
    }
}

/// `membrane_knowledge_propose` — store a knowledge proposal and return
/// the durable proposal id. The fixture pins the proposal id; a real
/// adapter would mint it from the durable store.
fn handle_membrane_knowledge_propose(request: &Value) -> Value {
    let task_envelope = request
        .get("taskEnvelope")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let turn_envelope = request
        .get("turnEnvelope")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let source_ref = request
        .pointer("/proposal/sourceRefs/0")
        .and_then(Value::as_str)
        .unwrap_or("");
    json!({
        "kind": "success",
        "data": {
            "ok": true,
            "proposalId": "prop-mbr-104-cortex",
            "status": "stored",
            "durable": true,
            "taskEnvelope": task_envelope,
            "turnEnvelope": turn_envelope,
            "binding": {
                "schema": "membrane.binding.v1",
                "kind": "source_ref",
                "value": source_ref
            }
        }
    })
}

/// `membrane_checkpoint_save` — save a checkpoint and return the
/// durable checkpoint id.
fn handle_membrane_checkpoint_save(request: &Value) -> Value {
    let task_envelope = request
        .get("taskEnvelope")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let turn_envelope = request
        .get("turnEnvelope")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let checkpoint_id = request
        .pointer("/checkpoint/id")
        .and_then(Value::as_str)
        .unwrap_or("ckpt-mbr-104-cortex");
    json!({
        "kind": "success",
        "data": {
            "ok": true,
            "checkpointId": checkpoint_id,
            "status": "saved",
            "durable": true,
            "bytes": 22,
            "taskEnvelope": task_envelope,
            "turnEnvelope": turn_envelope
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
