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
pub mod paths;
pub mod plan_context;
pub mod planner_metrics;
pub mod prep;
pub mod receipt;
pub mod release_identity;
pub mod runc;
pub mod serve;
pub mod service;
pub mod skel;
pub mod store;
pub use crypt_store::{context_telemetry, installation_identity, memdb, scope, time};
pub mod truncate;
pub mod vocabulary;

// Re-export OKF utilities so consumers import from one crate (`crypt`) during unification.
pub use crypt_format::okf;
pub use vocabulary::{
    crypt_migration_notice_text, crypt_notice_emitted, emit_facade_notice_once,
    membrane_notice_emitted, membrane_product_surface_notice_text, format_notice_log_line,
    ProductSurface, CRYPT_FACADE_MIGRATION_NOTICE, MEMBRANE_PRODUCT_SURFACE_NOTICE,
};

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
pub use paths::{cache_root, config_root, data_root, log_root, Roots, PRODUCT_DIR_NAME};
pub use receipt::{
    clear_receipt_registry, is_receipt_owned, register_receipt_owned, register_receipt_owned_path,
    remove_receipt_owned, snapshot as receipt_snapshot, ReceiptError, ReceiptOwnedFile,
    UninstallReceipt,
};
pub use store::{
    MemoryEventContext, MemoryLifecycleError, MemoryLifecycleEventV1, MemoryLifecycleInputV1,
    MemoryLifecycleKind, MemoryPriorityError, MemoryStore,
};
