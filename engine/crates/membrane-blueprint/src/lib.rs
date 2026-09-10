//! Native Blueprint foundation and in-process service lifecycle.
//!
//! Blueprint is exposed as typed Rust modules. Resident ownership is provided
//! by [`service`]; bounded explicit work uses [`service::OneShotExecutor`]
//! without requiring resident ownership.

pub mod api;
pub mod contracts;
pub mod cli;
pub mod doc_truth;
pub mod engine;
pub mod export;
pub mod findings;
pub mod freshness;
pub mod graph;
pub mod identity;
pub mod index;
pub mod migrations;
pub mod model;
pub mod phase2;
pub mod query;
pub mod security;
pub mod service;
pub mod store;
pub mod watch;

pub use api::{
    BlueprintApi, BlueprintBounds, BlueprintError, BlueprintOperation, BlueprintRequest,
    BlueprintResponse, BlueprintWireRequest, BlueprintWireResponse, Bounds, CancellationToken,
    Deadline, FreshnessBinding, FreshnessState, GenerationBinding, GraphEdge, GraphNode,
    RequestContext,
};
pub use cli::{architecture_orientation, expand, manual_refresh, recall, run_one_shot, run_query, search, status};
pub use engine::{native_blueprint_operation, NativeBlueprintOperation};
pub use export::{build_evidence_pack, execute_export, findings_to_sarif};
pub use findings::execute_findings;
pub use graph::GraphGeneration;
pub use model::{ImpactFrontierClass, Operation, RepositoryScope, RequestScope, PROTOCOL_VERSION};
pub use service::{
    execute_one_shot, BlueprintService, EventCollector, LifecycleEvent, LifecycleEventSink,
    LifecycleEventKind, LifecycleState, NativeService, OneShotExecutor, Readiness, ServiceConfig,
    ServiceError, ServiceStatus, Supervisor,
};

/// Map a stable error code to lifecycle degradation without requiring callers
/// to parse implementation-specific error text.
pub fn lifecycle_state_for_error(error: &str) -> LifecycleState {
    let value = error.to_ascii_lowercase();
    if value.contains("hub_inactive") || value.contains("membrane_not_running") {
        return LifecycleState::HubInactive;
    }
    if value.contains("resident_owner_active") {
        return LifecycleState::ResidentOwnerActive;
    }
    if value.contains("watcher") || value.contains("snapshot") {
        return LifecycleState::WatcherUnavailable;
    }
    if value.contains("stale") {
        return LifecycleState::Stale;
    }
    if value.contains("not_started") || value.contains("not_configured") {
        return LifecycleState::NotConfigured;
    }
    LifecycleState::TransportUnavailable
}
