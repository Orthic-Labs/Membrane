//! Crypt — the productizable memory engine: SQLite + quantized vectors + cognitive tiers +
//! hybrid retriever + effectiveness gate + dream consolidation, with multi-project `scope_id`
//! isolation. CodeRight (the product) and the workspace both CONSUME this crate; neither owns the
//! engine. Self-contained and publishable (depends only on crypt-core primitives).

extern crate self as crypt;

pub mod admission_policy;
pub mod agent_adapter_view;
pub mod cache_prefix;
pub mod catalog;
pub mod checkpoint;
pub mod cli;
pub mod code_batch;
pub mod compress;
pub mod compression_provider;
pub mod delivery_trace_view;
pub mod diagnostic_bundle;
pub mod digest;
pub mod doc_candidate_provider;
pub mod doc_projection;
pub mod doc_shadow;
pub mod doc_spine;
pub mod doctor;
pub mod federation;
pub mod federation_worker;
pub mod feedback;
pub mod fleet;
pub mod freshness;
pub mod hub;
pub mod installation_manifest;
pub mod mcp_http;
pub mod memory_provider;
pub mod memory_sentinel_view;
pub mod notifications;
pub mod outline;
pub mod paths;
pub mod plan_context;
pub mod planes;
pub mod planner_metrics;
pub mod prep;
pub mod provenance;
pub mod receipt;
pub mod release_identity;
pub mod runc;
pub mod scratchpad;
pub mod serve;
pub mod service;
pub mod skel;
pub mod source_resolution;
pub mod sources_explorer;
pub mod store;
pub mod team_policy;
pub use crypt_store::db::{record_observable_event, StoreError};
pub use crypt_store::{context_telemetry, installation_identity, memdb, scope, time};
pub use provenance::{
    capture_working_tree, observe, record_provenance, ProvenanceError, ProvenanceRowV1,
    WorkingTreeSnapshotV1, PROVENANCE_ROW_SCHEMA_VERSION, WORKING_TREE_SNAPSHOT_SCHEMA_VERSION,
};
pub mod truncate;
pub mod vocabulary;
pub mod working_context;

// Re-export OKF utilities so consumers import from one crate (`crypt`) during unification.
pub use crypt_format::okf;
pub use vocabulary::{
    crypt_migration_notice_text, crypt_notice_emitted, emit_facade_notice_once,
    format_notice_log_line, membrane_notice_emitted, membrane_product_surface_notice_text,
    ProductSurface, CRYPT_FACADE_MIGRATION_NOTICE, MEMBRANE_PRODUCT_SURFACE_NOTICE,
};
pub use working_context::{
    render_working_context, select_working_context, verify_envelope, WorkingContextBudgetV1,
    WorkingContextEnvelopeV1, WorkingContextError, WorkingContextSelectionV1,
    WORKING_CONTEXT_BUDGET_SCHEMA_VERSION, WORKING_CONTEXT_ENVELOPE_SCHEMA_VERSION,
    WORKING_CONTEXT_SELECTION_SCHEMA_VERSION,
};

pub use admission_policy::{
    admit, AdmissionDecision, AdmissionError, AdmissionRequest, Authority, InstructionPolicy,
    Origin, ProtectedSpan, ProtectedSpanKind, QuarantineStatus,
};
pub use checkpoint::{
    CheckpointError, CheckpointSourceRefV1, CheckpointSourceResolutionV1, CheckpointV1,
};
pub use crypt_store::MemDb;
pub use crypt_store::{
    TemporalFact, TemporalFactQuery, TemporalFactReceipt, TemporalFactStore, TemporalTransition,
};
pub use paths::{cache_root, config_root, data_root, log_root, Roots, PRODUCT_DIR_NAME};
pub use planes::{plane_for_path, Plane, PlaneBoundary, PLANE_BOUNDARIES};
pub use receipt::{
    clear_receipt_registry, is_receipt_owned, register_receipt_owned, register_receipt_owned_path,
    remove_receipt_owned, snapshot as receipt_snapshot, ReceiptError, ReceiptOwnedFile,
    UninstallReceipt,
};
pub use scope::{
    normalize_scope, path_to_scope, scope_chain, ScopeDescriptorError, ScopeDescriptorV1,
};
pub use store::{
    MemoryEventContext, MemoryLifecycleError, MemoryLifecycleEventV1, MemoryLifecycleInputV1,
    MemoryLifecycleKind, MemoryLifecycleOperation, MemoryLifecycleOperationV1,
    MemoryLifecycleReceiptV1, MemoryPriorityError, MemoryStore, RecallResult, VerifiedMemoryActor,
};
pub use team_policy::{
    admit_team_policy, admit_team_policy_with_opt_in, TeamPolicyAdmission,
    TeamPolicyAdmissionReason, TeamPolicyTrustVerifier, TrustedPolicyVerification,
};
