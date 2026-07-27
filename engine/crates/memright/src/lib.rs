//! MemRight — the productizable memory engine: SQLite + quantized vectors + cognitive tiers +
//! hybrid retriever + effectiveness gate + dream consolidation, with multi-project `scope_id`
//! isolation. CodeRight (the product) and the workspace both CONSUME this crate; neither owns the
//! engine. Self-contained and publishable (depends only on memright-core primitives).

pub mod catalog;
pub mod checkpoint;
pub mod compress;
pub mod context_telemetry;
pub mod doc_projection;
pub mod doc_spine;
pub mod doctor;
pub mod federation;
pub mod federation_worker;
pub mod feedback;
pub mod freshness;
pub mod installation_identity;
pub mod memdb;
pub mod outline;
pub mod plan_context;
pub mod planner_metrics;
pub mod prep;
pub mod release_identity;
pub mod runc;
pub mod scope;
pub mod serve;
pub mod skel;
pub mod store;
pub mod time;
pub mod truncate;

// Re-export OKF utilities so consumers import from one crate (`memright`) during unification.
pub use memright_format::okf;

pub use checkpoint::{
    CheckpointError, CheckpointSourceRefV1, CheckpointSourceResolutionV1, CheckpointV1,
};
pub use memdb::MemDb;
pub use scope::{normalize_scope, path_to_scope, scope_chain};
pub use store::{
    MemoryEventContext, MemoryLifecycleError, MemoryLifecycleEventV1, MemoryLifecycleKind,
    MemoryPriorityError, MemoryStore,
};
