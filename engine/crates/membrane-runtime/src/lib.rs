//! Crypt — the productizable memory engine: SQLite + quantized vectors + cognitive tiers +
//! hybrid retriever + effectiveness gate + dream consolidation, with multi-project `scope_id`
//! isolation. CodeRight (the product) and the workspace both CONSUME this crate; neither owns the
//! engine. Self-contained and publishable (depends only on crypt-core primitives).

extern crate self as crypt;

pub mod admission_policy;
pub mod catalog;
pub mod checkpoint;
pub mod cli;
pub mod compress;
pub mod doc_candidate_provider;
pub mod doc_projection;
pub mod doc_shadow;
pub mod doc_spine;
pub mod doctor;
pub mod federation;
pub mod federation_worker;
pub mod feedback;
pub mod freshness;
pub mod digest;
pub mod installation_manifest;
pub mod memory_provider;
pub mod outline;
pub mod plan_context;
pub mod planner_metrics;
pub mod prep;
pub mod release_identity;
pub mod runc;
pub mod serve;
pub mod service;
pub mod skel;
pub mod store;
pub use crypt_store::{context_telemetry, installation_identity, memdb, scope, time};
pub mod truncate;

// Re-export OKF utilities so consumers import from one crate (`crypt`) during unification.
pub use crypt_format::okf;

pub use admission_policy::{
    admit, AdmissionDecision, AdmissionError, AdmissionRequest, Authority, InstructionPolicy,
    Origin, ProtectedSpan, ProtectedSpanKind, QuarantineStatus,
};
pub use checkpoint::{
    CheckpointError, CheckpointSourceRefV1, CheckpointSourceResolutionV1, CheckpointV1,
};
pub use crypt_store::MemDb;
pub use scope::{
    normalize_scope, path_to_scope, scope_chain, ScopeDescriptorError, ScopeDescriptorV1,
};
pub use store::{
    MemoryEventContext, MemoryLifecycleError, MemoryLifecycleEventV1, MemoryLifecycleInputV1,
    MemoryLifecycleKind, MemoryPriorityError, MemoryStore,
};
