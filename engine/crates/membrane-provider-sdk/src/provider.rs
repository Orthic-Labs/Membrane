//! The `Provider` trait — the typed Membrane contract every external adapter
//! implements.
//!
//! An adapter that wants to serve one or more Membrane MCP operations
//! (the `membrane_*` set declared in `membrane-protocol::operations`)
//! implements `Provider`. The trait has three required methods:
//!
//!   1. `initialize` — set up the provider with a JSON configuration
//!      document. Called once, before any other method.
//!   2. `list_capabilities` — return every operation name the provider
//!      claims to support. The names MUST be a subset of
//!      `membrane_protocol::operations_slice()`.
//!   3. `handle_operation` — process one request and return one response.
//!      The request/response envelopes are `serde_json::Value` so the SDK
//!      stays neutral about the per-operation data shape; each provider is
//!      responsible for validating its own request against the per-operation
//!      JSON Schema.
//!
//! The conformance harness in `conformance` asserts that the provider's
//! response is identical (canonical-bytes-equal) to the fixture's expected
//! response for every fixture in the supplied set.

use crate::error::Result;
pub use membrane_protocol::{
    ProviderIdentityV1, ProviderObservationV1, ProviderReadinessStateV1, ProviderReadinessV1,
    ProviderTestQueryV1, PROVIDER_READINESS_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};

/// A single declared capability: the operation name plus the operation's
/// per-operation contract versions. The `schemaVersion` and `errorVersion`
/// are exposed so adapters can pin to the same independent versions the
/// `membrane-protocol::operations` registry uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityV1 {
    /// Operation name, e.g. `membrane_context`.
    pub name: String,
    /// Independent contract version of the operation.
    pub schema_version: u32,
    /// Independent error-taxonomy version of the operation.
    pub error_version: u32,
}

/// The typed Membrane provider contract.
///
/// The three required methods are the only surface area an adapter must
/// implement. Adapters MAY add helper methods (for example, a Cortex
/// `MemoryStore` accessor) but those helpers are not part of the SDK.
pub trait Provider: Send + Sync {
    /// Initialize the provider with a configuration document.
    ///
    /// Called exactly once, before any other method. The provider MUST be
    /// ready to serve `handle_operation` after a successful `initialize`.
    /// A second call MUST be idempotent or return
    /// `ProviderError::Uninitialized`-class error.
    fn initialize(&mut self, config: &serde_json::Value) -> Result<()>;

    /// Return one authoritative, typed observation; process existence is not readiness.
    fn readiness(&self) -> ProviderReadinessV1;

    /// Every operation this provider claims to support.
    ///
    /// The names MUST be a subset of
    /// `membrane_protocol::operations_slice()`. Returning a name that is
    /// not in the registry is permitted (the registry is the only
    /// authority), but the conformance harness will flag the mismatch.
    fn list_capabilities(&self) -> Vec<CapabilityV1>;

    /// Handle one request and return one response.
    ///
    /// `operation` is the operation name (e.g. `membrane_context`).
    /// `request` is the canonical-JSON request envelope the caller sent.
    /// The returned `serde_json::Value` is the canonical-JSON response
    /// envelope, which MUST be one of:
    ///
    ///   * `{"kind": "success", "data": <operation-specific>}`, or
    ///   * `{"kind": "error", "code": "...", "message": "...",
    ///      "retryable": <bool>, "details": <optional>}`.
    ///
    /// If the provider cannot process the request at all, it returns
    /// `Err(ProviderError::...)` instead of producing a typed error
    /// envelope — the harness distinguishes the two cases.
    fn handle_operation(
        &self,
        operation: &str,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value>;
}
