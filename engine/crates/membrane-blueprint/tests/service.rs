use membrane_blueprint::api::{
    BlueprintApi, BlueprintOperation, BlueprintRequest, CancellationToken, RequestContext,
};
use membrane_blueprint::model::Operation;
use membrane_blueprint::service::{
    EventCollector, LifecycleEventKind, NativeService, OneShotExecutor, ServiceConfig,
    ServiceStatus,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::tempdir;

struct Echo;

impl BlueprintOperation for Echo {
    fn execute(
        &self,
        request: &BlueprintRequest,
        _: &RequestContext,
    ) -> Result<Value, membrane_blueprint::BlueprintError> {
        Ok(serde_json::json!({ "method": request.method.as_str() }))
    }
}

struct RecordingRefresh {
    calls: Arc<AtomicUsize>,
    paths: Arc<Mutex<Vec<String>>>,
}

struct RetryRefresh {
    calls: Arc<AtomicUsize>,
}

impl BlueprintOperation for RetryRefresh {
    fn execute(
        &self,
        request: &BlueprintRequest,
        _: &RequestContext,
    ) -> Result<Value, membrane_blueprint::BlueprintError> {
        assert_eq!(request.method, Operation::Refresh);
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            Err(membrane_blueprint::BlueprintError::new("refresh_failed", "injected refresh failure"))
        } else {
            Ok(serde_json::json!({ "refreshed": true }))
        }
    }
}

struct RetryBatchRefresh {
    paths: Arc<Mutex<Vec<String>>>,
    failed: Arc<AtomicUsize>,
}

impl BlueprintOperation for RetryBatchRefresh {
    fn execute(
        &self,
        request: &BlueprintRequest,
        _: &RequestContext,
    ) -> Result<Value, membrane_blueprint::BlueprintError> {
        assert_eq!(request.method, Operation::Refresh);
        let path = request.input["paths"][0].as_str().unwrap().to_owned();
        self.paths.lock().unwrap().push(path.clone());
        if path == "b.txt" && self.failed.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(membrane_blueprint::BlueprintError::new("refresh_failed", "injected batch failure"));
        }
        Ok(serde_json::json!({ "refreshed": true }))
    }
}

struct SlowRefresh;

impl BlueprintOperation for SlowRefresh {
    fn execute(
        &self,
        request: &BlueprintRequest,
        _: &RequestContext,
    ) -> Result<Value, membrane_blueprint::BlueprintError> {
        assert_eq!(request.method, Operation::Refresh);
        std::thread::sleep(std::time::Duration::from_millis(2_100));
        Ok(serde_json::json!({ "refreshed": true }))
    }
}

impl BlueprintOperation for RecordingRefresh {
    fn execute(
        &self,
        request: &BlueprintRequest,
        _: &RequestContext,
    ) -> Result<Value, membrane_blueprint::BlueprintError> {
        assert_eq!(request.method, Operation::Refresh);
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(path) = request.input["paths"].as_array().and_then(|paths| paths.first()).and_then(Value::as_str) {
            self.paths.lock().unwrap().push(path.to_owned());
        }
        Ok(serde_json::json!({ "refreshed": true }))
    }
}

#[test]
fn resident_service_starts_idempotently_and_emits_events() {
    let root = tempdir().unwrap();
    let sink = Arc::new(EventCollector::default());
    let service = NativeService::from_operation(Echo, ServiceConfig::new(root.path()))
        .with_sink(sink.clone());
    assert_eq!(service.start().unwrap(), ServiceStatus::Running);
    assert_eq!(service.start().unwrap(), ServiceStatus::Running);
    assert!(service.is_ready());
    let kinds = sink
        .events()
        .into_iter()
        .map(|event| event.kind)
        .collect::<Vec<_>>();
    assert!(kinds.contains(&LifecycleEventKind::Ready));
    assert!(kinds.contains(&LifecycleEventKind::StartIdempotent));
}

#[test]
fn watcher_failure_is_typed_and_does_not_fake_graph_success() {
    let service = NativeService::from_operation(Echo, ServiceConfig::new("missing-blueprint-root"));
    let error = service.start().unwrap_err();
    assert_eq!(error.code(), "watcher_unavailable");
    assert_eq!(service.status(), ServiceStatus::Degraded);
    assert!(!service.is_ready());
}

#[test]
fn drain_is_idempotent_and_reaches_stopped() {
    let root = tempdir().unwrap();
    let service = NativeService::from_operation(Echo, ServiceConfig::new(root.path()));
    service.start().unwrap();
    service.drain().unwrap();
    service.drain().unwrap();
    assert_eq!(service.status(), ServiceStatus::Stopped);
    assert!(!service.is_ready());
}

#[test]
fn supervise_dispatches_file_change_to_native_refresh_operation() {
    let root = tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let paths = Arc::new(Mutex::new(Vec::new()));
    let service = NativeService::from_operation(
        RecordingRefresh {
            calls: calls.clone(),
            paths: paths.clone(),
        },
        ServiceConfig::new(root.path()),
    );
    service.start().unwrap();
    std::fs::write(root.path().join("changed.txt"), b"changed").unwrap();
    assert_eq!(service.supervise(), ServiceStatus::Running);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let recorded = paths.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0], "changed.txt");
}

#[test]
fn degraded_restart_replays_failed_refresh_before_new_snapshot() {
    let root = tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let service = NativeService::from_operation(
        RetryRefresh { calls: calls.clone() },
        ServiceConfig::new(root.path()),
    );
    service.start().unwrap();
    std::fs::write(root.path().join("changed.txt"), b"changed").unwrap();

    assert_eq!(service.supervise(), ServiceStatus::Degraded);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(service.start().unwrap(), ServiceStatus::Running);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn degraded_restart_replays_failed_and_later_batch_events_in_order() {
    let root = tempdir().unwrap();
    let paths = Arc::new(Mutex::new(Vec::new()));
    let service = NativeService::from_operation(
        RetryBatchRefresh { paths: paths.clone(), failed: Arc::new(AtomicUsize::new(0)) },
        ServiceConfig::new(root.path()),
    );
    service.start().unwrap();
    std::fs::write(root.path().join("a.txt"), b"a").unwrap();
    std::fs::write(root.path().join("b.txt"), b"b").unwrap();
    std::fs::write(root.path().join("c.txt"), b"c").unwrap();

    assert_eq!(service.supervise(), ServiceStatus::Degraded);
    assert_eq!(&*paths.lock().unwrap(), &["a.txt", "b.txt"]);
    assert_eq!(service.start().unwrap(), ServiceStatus::Running);
    assert_eq!(&*paths.lock().unwrap(), &["a.txt", "b.txt", "b.txt", "c.txt"]);
}

#[test]
fn refresh_returning_after_deadline_degrades_with_typed_error() {
    let root = tempdir().unwrap();
    let service = NativeService::from_operation(SlowRefresh, ServiceConfig::new(root.path()));
    service.start().unwrap();
    std::fs::write(root.path().join("changed.txt"), b"changed").unwrap();

    assert_eq!(service.supervise(), ServiceStatus::Degraded);
    let readiness = service.readiness();
    assert_eq!(readiness.state, Some(membrane_blueprint::service::LifecycleState::Stale));
    assert!(readiness.detail.unwrap().starts_with("rebuild callback failed: deadline_exceeded:"));
}

#[test]
fn one_shot_runs_without_resident_service() {
    let executor = OneShotExecutor::from_operation(Echo);
    let request = BlueprintRequest::new("one-shot", Operation::Status, ".");
    let response = executor.dispatch(request, CancellationToken::new());
    assert!(response.ok);
    assert_eq!(response.result.unwrap()["method"], "status");
}
