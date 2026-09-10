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

pub mod dependency_dag;

pub mod merkle_ledger;

pub mod bm25_index;

pub mod entry_points;

pub mod conformance_verifier;
pub mod freshness_receipt;

pub mod ast_structural_search;
pub mod ast_walker;

pub mod static_provider;
pub mod delta_store;

pub mod contract_registry;
pub mod conventions;
pub mod evidence_authority;
pub mod liveness;

pub mod architecture_model;
pub mod framework_intelligence;
pub mod git_source_observation;
pub mod lib_path_confinement;

pub mod atomic_adopt;
pub mod confidence_tiers;
pub mod bootstrap;
pub mod store_delta;

pub mod recall_circuit;
pub mod module_resolution;
pub mod lib_redaction;

pub mod lib_hooks_graph_index;
pub mod lib_init_detect_hosts;
pub mod lib_init_plan;
pub mod lib_init_host_configs;
pub mod lib_run_ledger;
pub mod lib_comment_claims;
pub mod lib_rules_parser;
pub mod lib_rules_exceptions;
pub mod lib_rules_baseline;
pub mod lib_rules_evaluate;
pub mod lib_languages_custom_config;
pub mod lib_orientation_evidence;
pub mod lib_receipt_store;
pub mod lib_operations_repair;
pub mod lib_operations_support_bundle;
pub mod lib_operations_doctor;
pub mod lib_init_state_integrity;

pub mod lib_application_errors;
pub mod lib_application_root_registry;
pub mod lib_runtime_capabilities;
pub mod lib_application_normalize;
pub mod lib_token_budget;
pub mod lib_http_server;
pub mod lib_phase2_completion;
pub mod lib_generated_docs;
pub mod lib_admission;
pub mod lib_update_channel;
pub mod lib_update_manifest;
pub mod lib_explorer_layout;
pub mod lib_explorer_static;
pub mod lib_findings_specifier;
pub mod lib_init_apply;
pub mod lib_update_apply;
pub mod lib_update_rollback;
pub mod lib_application_document_truth;
pub mod lib_application_snapshots;
pub mod lib_application_federate;
pub mod analytics;
pub mod change_impact;
pub mod test_recommendation;
pub mod reanchor;
pub mod process_projection;
pub mod signature_projection;
pub mod orientation_projection;

pub mod providers;

pub mod lib_operations_init;
pub mod lib_operations_update;

pub mod lib_cli_languages;
pub mod lib_cli_rules;
pub mod lib_cli_mcp;
pub mod lib_cli_uninstall;
pub mod architecture_views;
pub mod freshness_observation;
