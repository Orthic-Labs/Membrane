//! # membrane-protocol
//!
//! The **contract source of truth** for the Membrane protocol's five typed
//! shapes:
//!
//!   | Shape                  | Rust type                | Schema (v1)                        |
//!   |------------------------|--------------------------|------------------------------------|
//!   | ScopeGrant             | [`ScopeGrantV1`]         | `schemas/scope-grant.v1.schema.json` |
//!   | ContextCandidateSet    | [`ContextCandidateSetV1`]| `schemas/context-candidate-set.v1.schema.json` |
//!   | ContextPacket          | [`ContextPacketV1`]      | `schemas/context-packet.v1.schema.json` |
//!   | ContextReceipt         | [`ContextReceiptV1`]     | `schemas/context-receipt.v1.schema.json` |
//!   | KnowledgeEmission      | [`KnowledgeEmissionV1`]  | `schemas/knowledge-emission.v1.schema.json` |
//!
//! The Rust types in [`types`] are authoritative. The JSON Schemas under
//! `schemas/` are derived from them, and golden fixtures under `operations/`
//! are the shared canonical instances. The crate's tests prove — and the
//! TypeScript binding under `bindings/` independently re-proves — that each
//! golden fixture:
//!
//!   1. validates against its JSON Schema,
//!   2. deserializes + re-serializes to the **identical canonical bytes**, and
//!   3. yields the **same `sha256:` digest** in Rust and in TypeScript.
//!
//! See `docs/protocol/source-of-truth.md` for the full contract story.

pub mod canonical;
pub mod heartbeat;
pub mod installation;
pub mod lease;
pub mod operations;
pub mod types;

pub use canonical::{canonical_json_of, canonicalize, digest_str};
pub use heartbeat::{AdapterHeartbeatV1, AdapterStatus, MechanismReceiptV1, ADAPTER_HEARTBEAT_SCHEMA_VERSION};
pub use installation::{
    canonical_data_root, ApiSchemaV1, ComponentV1, HandshakeError, InstallationManifestV1,
    MANIFEST_SCHEMA_VERSION, PROTOCOL_SCHEMA_VERSION,
};
pub use lease::{ComponentLeaseV1, COMPONENT_LEASE_SCHEMA_VERSION};
pub use operations::{
    operations, operations_slice, ErrorResult, OperationIndexEntry, OperationResult, OperationSpec,
    OperationsIndex, ResultKind, SuccessResult, OPERATIONS,
};
pub use types::*;

use serde::Serialize;

/// The canonical byte contract shared with the TypeScript binding.
///
/// Implemented for every protocol type via its `Serialize` impl. The methods
/// return the canonical JSON (sorted keys, no whitespace) and its `sha256:`
/// digest — the exact bytes/digest the TS side must reproduce.
pub trait CanonicalSerialize: Serialize {
    /// Canonical JSON: object keys sorted, no insignificant whitespace.
    fn canonical_json(&self) -> String {
        canonical_json_of(self)
    }
    /// `sha256:<hex>` of [`Self::canonical_json`].
    fn canonical_digest(&self) -> String {
        crate::canonical::canonical_digest_of(self)
    }
}

// Blanket-impl the canonical contract for every serializable protocol type.
impl<T: Serialize> CanonicalSerialize for T {}

/// One typed contract shape: its name, its canonical JSON Schema, and its
/// golden fixture, all embedded so both the Rust tests and any downstream
/// consumer read exactly the same artifacts the TS binding reads from disk.
pub struct ContractShape {
    /// Stable shape name, e.g. `"ScopeGrantV1"`.
    pub name: &'static str,
    /// Repo-relative path of the JSON Schema (also embedded as `schema`).
    pub schema_path: &'static str,
    /// Repo-relative path of the golden fixture (also embedded as `fixture`).
    pub fixture_path: &'static str,
    /// The canonical JSON Schema text.
    pub schema: &'static str,
    /// The golden fixture JSON text.
    pub fixture: &'static str,
}

// Schemas live at the repo root under `schemas/`; fixtures under `operations/`.
// `CARGO_MANIFEST_DIR` is `engine/crates/membrane-protocol`, so the repo root is
// three levels up.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../..");

macro_rules! shape {
    ($name:literal, $schema:literal, $fixture:literal) => {
        ContractShape {
            name: $name,
            schema_path: concat!("schemas/", $schema),
            fixture_path: concat!("operations/", $fixture),
            schema: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../schemas/",
                $schema
            )),
            fixture: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../operations/",
                $fixture
            )),
        }
    };
}

/// Every typed contract shape, in canonical order. Used by the round-trip tests
/// and re-exported for downstream gates.
pub const SHAPES: &[ContractShape] = &[
    shape!(
        "ScopeGrantV1",
        "scope-grant.v1.schema.json",
        "scope-grant.v1.golden.json"
    ),
    shape!(
        "ContextCandidateSetV1",
        "context-candidate-set.v1.schema.json",
        "context-candidate-set.v1.golden.json"
    ),
    shape!(
        "ContextPacketV1",
        "context-packet.v1.schema.json",
        "context-packet.v1.golden.json"
    ),
    shape!(
        "ContextReceiptV1",
        "context-receipt.v1.schema.json",
        "context-receipt.v1.golden.json"
    ),
    shape!(
        "KnowledgeEmissionV1",
        "knowledge-emission.v1.schema.json",
        "knowledge-emission.v1.golden.json"
    ),
];

/// Absolute path to the repository root (for tests that read from disk).
pub fn repo_root() -> &'static str {
    ROOT
}
