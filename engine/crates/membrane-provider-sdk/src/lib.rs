//! # membrane-provider-sdk
//!
//! The Membrane provider SDK. Every external adapter that wants to serve
//! one or more Membrane MCP operations (the `membrane_*` set declared in
//! `membrane-protocol::operations`) implements [`Provider`] and is
//! validated against the canonical fixture corpus in `membrane-testkit`
//! via [`run_conformance`].
//!
//! The SDK is intentionally tiny. It re-exports:
//!
//!   * [`Provider`] — the trait every adapter implements
//!     ([`provider::Provider`]).
//!   * [`CapabilityV1`] — the per-operation capability envelope
//!     ([`provider::CapabilityV1`]).
//!   * [`Fixture`] — the canonical conformance fixture shape
//!     ([`conformance::Fixture`]).
//!   * [`run_conformance`] / [`ConformanceReport`] / [`FixtureFailure`] —
//!     the conformance harness ([`conformance`]).
//!   * [`ProviderError`] / [`Result`] — the typed error vocabulary
//!     ([`error`]).
//!
//! ## Usage
//!
//! ```no_run
//! use membrane_provider_sdk::{
//!     run_conformance, CapabilityV1, Fixture, Provider, ProviderError, Result,
//! };
//! use serde_json::json;
//!
//! struct EchoProvider {
//!     ops: Vec<&'static str>,
//!     ready: bool,
//! }
//!
//! impl Provider for EchoProvider {
//!     fn initialize(&mut self, _config: &serde_json::Value) -> Result<()> {
//!         self.ready = true;
//!         Ok(())
//!     }
//!     fn list_capabilities(&self) -> Vec<CapabilityV1> {
//!         self.ops
//!             .iter()
//!             .map(|n| CapabilityV1 {
//!                 name: (*n).to_string(),
//!                 schema_version: 1,
//!                 error_version: 1,
//!             })
//!             .collect()
//!     }
//!     fn handle_operation(
//!         &self,
//!         _operation: &str,
//!         _request: &serde_json::Value,
//!     ) -> Result<serde_json::Value> {
//!         if !self.ready {
//!             return Err(ProviderError::Uninitialized("echo".into()));
//!         }
//!         Ok(json!({ "kind": "success", "data": { "ok": true } }))
//!     }
//! }
//! ```
//!
//! The `membrane-testkit` crate supplies the golden Cortex and Crypt
//! fixture sets, and the `examples/providers/{cortex,crypt}_example`
//! crates show two reference adapters. See `docs/providers/README.md` for
//! the full adapter-authoring guide.

pub mod batch;
pub mod conformance;
pub mod error;
pub mod provider;

pub use batch::{
    CodeBatchItemV1, CodeBatchReceiptV1, MAX_BATCH_BYTES, MAX_BATCH_DEADLINE_MS, MAX_BATCH_ITEMS,
    MAX_BATCH_TOKENS,
};
pub use conformance::{run_conformance, ConformanceReport, Fixture, FixtureFailure};
pub use error::{ProviderError, Result};
pub use provider::{CapabilityV1, Provider};

/// Re-export the membrane-protocol operations slice accessor so adapter
/// authors can pin their `CapabilityV1` to the same registry the runtime
/// and the MCP tool surface use.
pub use membrane_protocol::operations_slice;

/// Re-export the membrane-protocol canonical-JSON helpers so adapters can
/// byte-compare their own response against the fixture's expected
/// response in their own unit tests.
pub use membrane_protocol::canonical::{canonical_json_of, canonicalize, digest_str};
