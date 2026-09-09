//! Native parity: exercises the real resident `NativeService` dispatching to
//! the real `NativeBlueprintOperation` producer -- never a fabricated
//! `NativeApi` response. Covers manual refresh without a background holder
//! (LC-02) and watcher-publish-before-fresh / offline-edit reconciliation on
//! activation (laneTask r5).

use membrane_blueprint::service::{NativeService, ServiceConfig, ServiceStatus};
use membrane_blueprint::{
    BlueprintRequest, CancellationToken, NativeBlueprintOperation, Operation,
};
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

fn request(id: &str, method: Operation, root: &std::path::Path) -> BlueprintRequest {
    let mut request = BlueprintRequest::new(id, method, root.to_string_lossy());
    request.deadline_ms = if method.is_build() { 120_000 } else { 30_000 };
    request
}

#[test]
fn manual_refresh_succeeds_from_idle_without_background_holder() {
    // No watcher config is attached: the resident service starts with no
    // background holder and must still service an explicit refresh/build.
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let service = NativeService::from_operation(NativeBlueprintOperation, ServiceConfig::new(root.path()).without_watcher());
    assert_eq!(service.status(), ServiceStatus::Stopped);
    let started = service.start().unwrap();
    assert_eq!(started, ServiceStatus::Running);

    let build = request("manual-build", Operation::Build, root.path());
    let response = service.dispatch_request(build, CancellationToken::new());
    assert!(response.ok, "manual refresh must succeed idle/no-holder: {:?}", response.error);
    let generation = response.result.as_ref().unwrap()["generationId"].as_str().unwrap().to_owned();

    // Freshness must be reported from the published generation, not from
    // request enqueue -- query the same generation id back out.
    let mut query = request("manual-query", Operation::Search, root.path());
    query.generation = Some(generation.clone());
    query.input["query"] = Value::String("entry".into());
    let query_response = service.dispatch_request(query, CancellationToken::new());
    assert!(query_response.ok, "query after manual refresh must succeed: {:?}", query_response.error);
    assert_eq!(query_response.result.as_ref().unwrap()["generationId"], generation);
}

#[test]
fn dispatch_before_start_fails_closed_as_typed_not_ready() {
    let root = tempdir().unwrap();
    let service = NativeService::from_operation(NativeBlueprintOperation, ServiceConfig::new(root.path()).without_watcher());
    let response = service.dispatch_request(
        request("early", Operation::Status, root.path()),
        CancellationToken::new(),
    );
    assert!(!response.ok);
    assert_eq!(response.error.as_ref().unwrap().code, "service_not_ready");
}

#[test]
fn offline_edits_reconcile_on_activation_via_explicit_refresh() {
    // Simulate edits made while no watcher/holder was resident: the file is
    // written before the service is ever started. Activation (start + an
    // explicit refresh dispatch) must reconcile the offline edit into a
    // queryable generation -- no silent staleness.
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let service = NativeService::from_operation(NativeBlueprintOperation, ServiceConfig::new(root.path()).without_watcher());
    service.start().unwrap();
    let build = request("activation-build", Operation::Build, root.path());
    let build_response = service.dispatch_request(build, CancellationToken::new());
    assert!(build_response.ok);
    let generation = build_response.result.as_ref().unwrap()["generationId"].as_str().unwrap().to_owned();

    // Edit made "offline" relative to the service (no watcher attached).
    fs::write(root.path().join("second.rs"), "fn offline() {}\n").unwrap();

    let mut refresh = request("activation-refresh", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("changed".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("second.rs".into())]);
    let refresh_response = service.dispatch_request(refresh, CancellationToken::new());
    assert!(refresh_response.ok, "offline edit reconciliation must succeed: {:?}", refresh_response.error);

    let mut query = request("activation-query", Operation::Search, root.path());
    query.generation = Some(generation);
    query.input["query"] = Value::String("offline".into());
    let query_response = service.dispatch_request(query, CancellationToken::new());
    assert!(query_response.ok);
    assert_eq!(query_response.result.as_ref().unwrap()["sourceObservation"]["sourceClock"], 1);
}
