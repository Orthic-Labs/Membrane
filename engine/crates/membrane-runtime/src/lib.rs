//! Membrane runtime — one implementation surface for Pull, Push, Cortex,
//! Blueprint, Ledger, and Adapt. Cortex durable storage remains isolated from
//! Pull acquisition and Push reduction.

pub mod adapt;
pub mod adapt_efficiency;
pub mod admission_producer;
pub mod agent_adapter_producer;
pub mod agent_adapter_view;
pub mod authorization;
pub mod background_review;
pub mod cache_prefix;
pub mod catalog;
pub mod checkpoint;
pub mod cli;
pub mod code_batch;
pub mod cortex_relevance_spotcheck;
pub mod delivery_trace_view;
pub mod diagnostic_bundle;
pub mod digest;
pub mod doctor;
pub mod feedback;
pub mod fleet;
pub mod freshness;
pub mod host_observation_ingress;
pub mod hub;
pub mod hub_inputs;
pub mod hub_readonly_db;
pub mod installation_manifest;
pub mod ledger;
pub mod live_diagnostics;
pub mod live_diagnostics_service;
pub mod mcp_executor;
pub mod mcp_http;
pub mod memory_provider;
pub mod memory_sentinel_producer;
pub mod memory_sentinel_view;
pub mod native_diagnostics_pipe;
pub mod notifications;
pub mod paths;
pub mod planes;
pub mod provenance;
pub mod providers;
pub mod pull;
pub mod receipt;
pub mod release_identity;
pub mod runtime_receipt;
pub mod scratchpad;
pub mod serve;
pub mod service;
pub mod source_resolution;
pub mod sources_explorer;
pub mod sources_producer;
pub mod store;
pub mod team_policy;
pub use cortex_store::db::{record_observable_event, StoreError};
pub use cortex_store::{context_telemetry, installation_identity, memdb, scope, time};
pub use provenance::{
    capture_working_tree, observe, record_provenance, ProvenanceError, ProvenanceRowV1,
    WorkingTreeSnapshotV1, PROVENANCE_ROW_SCHEMA_VERSION, WORKING_TREE_SNAPSHOT_SCHEMA_VERSION,
};
pub mod push;
pub mod working_context;

pub use authorization::{
    authorize as authorize_native_request, intersect_authority, permits_level, AuthorityLevel,
    AuthorizationDecisionV1, AuthorizationDenial, AuthorizationGate, InstallationRegistryV1,
    RepositoryBindingV1, AUTHORIZATION_REQUEST_SCHEMA_VERSION, INSTALLATION_AUTHORITY_LEVEL,
};
pub use host_observation_ingress::project_joined_effectiveness;

pub use background_review::{
    execute_background_review, BackgroundReviewCompletion, BackgroundReviewDecision,
    BackgroundReviewLearner, BackgroundReviewLearnerResult, BackgroundReviewObservationSink,
    BackgroundReviewProducer, BackgroundReviewProduction, BackgroundReviewProposalSink,
    BackgroundReviewScheduler, BackgroundReviewSinkError, JsonlBackgroundReviewObservationSink,
    NoSemanticReviewLearner, CONFIG_PATH_ENV, DEFAULT_CONFIG_RELATIVE_PATH,
    DEFAULT_OBSERVATIONS_RELATIVE_PATH, MAX_ATTEMPTS, MAX_OBSERVATION_BATCH,
    MAX_OBSERVATION_FILE_BYTES, MAX_OBSERVATION_RECORD_BYTES, OBSERVATIONS_PATH_ENV,
};

// Re-export OKF persistence-format utilities through Membrane. Push compression
// has its own protected-span authority and does not call cortex-format codecs.
pub use cortex_format::okf;
pub use working_context::{
    render_working_context, select_working_context, verify_envelope, WorkingContextBudgetV1,
    WorkingContextEnvelopeV1, WorkingContextError, WorkingContextSelectionV1,
    WORKING_CONTEXT_BUDGET_SCHEMA_VERSION, WORKING_CONTEXT_ENVELOPE_SCHEMA_VERSION,
    WORKING_CONTEXT_SELECTION_SCHEMA_VERSION,
};

pub use checkpoint::{
    CheckpointError, CheckpointSourceRefV1, CheckpointSourceResolutionV1, CheckpointV1,
};
pub use cortex_store::MemDb;
pub use cortex_store::{
    TemporalFact, TemporalFactQuery, TemporalFactReceipt, TemporalFactStore, TemporalTransition,
};
pub use live_diagnostics_service::{
    diagnostics_native_dispatch, diagnostics_router, resident_diagnostics_routes,
    static_capabilities, DiagnosticsService, FenceEvaluateRequest, LiveDiagnosticsServiceError,
    NativeDiagnosticsRequest, NativeDiagnosticsResponse, SnapshotAwaitRequest,
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
    ApprovedProposalAdmissionV1, CortexBackupLinkV1, CortexBackupRowV1, CortexBackupV1,
    MemoryEventContext, MemoryLifecycleError, MemoryLifecycleEventV1, MemoryLifecycleInputV1,
    MemoryLifecycleKind, MemoryLifecycleOperation, MemoryLifecycleOperationV1,
    MemoryLifecycleReceiptV1, MemoryPriorityError, MemoryStore, RecallResult, VerifiedMemoryActor,
};
pub use team_policy::{
    admit_team_policy, admit_team_policy_with_opt_in, TeamPolicyAdmission,
    TeamPolicyAdmissionReason, TeamPolicyTrustVerifier, TrustedPolicyVerification,
};

pub mod adapt_service;

pub mod adapt_observations;