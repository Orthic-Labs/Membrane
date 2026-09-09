//! Native in-process Blueprint lifecycle.
//!
//! The service owns resident watcher state and delegates every graph request
//! to an injected [`BlueprintOperation`]. It does not invent graph results,
//! own storage, or require a separate process. One-shot execution uses the
//! same operation boundary while remaining independent of resident state.

use crate::api::{
    BlueprintApi, BlueprintError, BlueprintOperation, BlueprintRequest, BlueprintResponse,
    Bounds, CancellationToken,
};
use crate::model::Operation;
use crate::watch::{NativeWatcher, SnapshotConfig, WatchError, WatchEvent};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    Stopped,
    Starting,
    Running,
    Draining,
    Degraded,
    Unavailable,
}

impl ServiceStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Draining => "draining",
            Self::Degraded => "degraded",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Running,
    NotConfigured,
    Stale,
    WatcherUnavailable,
    TransportUnavailable,
    HubInactive,
    ResidentOwnerActive,
    Draining,
}

impl LifecycleState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::NotConfigured => "not_configured",
            Self::Stale => "stale",
            Self::WatcherUnavailable => "watcher_unavailable",
            Self::TransportUnavailable => "transport_unavailable",
            Self::HubInactive => "hub_inactive",
            Self::ResidentOwnerActive => "resident_owner_active",
            Self::Draining => "draining",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleEventKind {
    StartRequested,
    Started,
    StartIdempotent,
    Ready,
    Supervised,
    RebuildScheduled,
    Degraded,
    DrainRequested,
    DrainIdempotent,
    Drained,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleEvent {
    pub sequence: u64,
    pub kind: LifecycleEventKind,
    pub status: ServiceStatus,
    pub state: Option<LifecycleState>,
    pub detail: Option<String>,
}

pub trait LifecycleEventSink: Send + Sync {
    fn observe(&self, event: LifecycleEvent);
}

#[derive(Default)]
pub struct EventCollector(Mutex<Vec<LifecycleEvent>>);

impl EventCollector {
    pub fn events(&self) -> Vec<LifecycleEvent> {
        self.0.lock().map(|events| events.clone()).unwrap_or_default()
    }
}

impl LifecycleEventSink for EventCollector {
    fn observe(&self, event: LifecycleEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServiceConfig {
    pub workspace_root: PathBuf,
    pub watcher: Option<SnapshotConfig>,
    pub max_events: usize,
}

impl ServiceConfig {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        let workspace_root = workspace_root.into();
        Self {
            watcher: Some(SnapshotConfig::new(workspace_root.clone())),
            workspace_root,
            max_events: 256,
        }
    }

    pub fn without_watcher(mut self) -> Self {
        self.watcher = None;
        self
    }

    pub fn with_watcher(mut self, watcher: SnapshotConfig) -> Self {
        self.watcher = Some(watcher);
        self
    }

    pub fn max_events(mut self, max_events: usize) -> Self {
        self.max_events = max_events.max(1);
        self
    }
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self::new(".")
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ServiceError {
    #[error("Blueprint service is not ready")]
    NotReady,
    #[error("Blueprint service is draining")]
    Draining,
    #[error("Blueprint service watcher failed: {0}")]
    Watcher(String),
    #[error("Blueprint service operation failed: {0}")]
    Operation(#[from] BlueprintError),
}

impl ServiceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotReady => "service_not_ready",
            Self::Draining => "service_draining",
            Self::Watcher(_) => "watcher_unavailable",
            Self::Operation(error) => match error.code.as_str() {
                "request_cancelled" => "request_cancelled",
                "deadline_exceeded" => "deadline_exceeded",
                _ => "blueprint_operation_failed",
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Readiness {
    pub live: bool,
    pub ready: bool,
    pub status: ServiceStatus,
    pub state: Option<LifecycleState>,
    pub detail: Option<String>,
}

struct ServiceInner {
    status: ServiceStatus,
    state: Option<LifecycleState>,
    detail: Option<String>,
    sequence: u64,
    events: Vec<LifecycleEvent>,
    watcher: Option<NativeWatcher>,
    pending_refreshes: Vec<WatchEvent>,
}

/// Resident native owner. The injected operation is the sole graph authority.
pub struct NativeService {
    operation: Arc<dyn BlueprintOperation>,
    config: ServiceConfig,
    sink: Option<Arc<dyn LifecycleEventSink>>,
    inner: Mutex<ServiceInner>,
}

pub type BlueprintService = NativeService;
pub type Supervisor = NativeService;

impl NativeService {
    pub fn new(operation: Arc<dyn BlueprintOperation>, config: ServiceConfig) -> Self {
        Self {
            operation,
            config,
            sink: None,
            inner: Mutex::new(ServiceInner {
                status: ServiceStatus::Stopped,
                state: None,
                detail: None,
                sequence: 0,
                events: Vec::new(),
                watcher: None,
                pending_refreshes: Vec::new(),
            }),
        }
    }

    pub fn from_operation<O>(operation: O, config: ServiceConfig) -> Self
    where
        O: BlueprintOperation + 'static,
    {
        Self::new(Arc::new(operation), config)
    }

    pub fn resident<O>(operation: O, workspace_root: impl Into<PathBuf>) -> Self
    where
        O: BlueprintOperation + 'static,
    {
        Self::from_operation(operation, ServiceConfig::new(workspace_root))
    }

    pub fn with_sink(mut self, sink: Arc<dyn LifecycleEventSink>) -> Self {
        self.sink = Some(sink);
        self
    }

    pub fn status(&self) -> ServiceStatus {
        self.inner
            .lock()
            .map(|inner| inner.status)
            .unwrap_or(ServiceStatus::Unavailable)
    }

    pub fn lifecycle_state(&self) -> Option<LifecycleState> {
        self.inner.lock().ok().and_then(|inner| inner.state)
    }

    pub fn readiness(&self) -> Readiness {
        let Ok(inner) = self.inner.lock() else {
            return Readiness {
                live: false,
                ready: false,
                status: ServiceStatus::Unavailable,
                state: Some(LifecycleState::TransportUnavailable),
                detail: Some("service_state_unavailable".into()),
            };
        };
        Readiness {
            live: !matches!(inner.status, ServiceStatus::Stopped | ServiceStatus::Unavailable),
            ready: inner.status == ServiceStatus::Running,
            status: inner.status,
            state: inner.state,
            detail: inner.detail.clone(),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.readiness().ready
    }

    pub fn events(&self) -> Vec<LifecycleEvent> {
        self.inner
            .lock()
            .map(|inner| inner.events.clone())
            .unwrap_or_default()
    }

    pub fn start(&self) -> Result<ServiceStatus, ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Watcher("service_state_unavailable".into()))?;
        if inner.status == ServiceStatus::Running {
            self.emit_locked(&mut inner, LifecycleEventKind::StartIdempotent, None);
            return Ok(ServiceStatus::Running);
        }
        if inner.status == ServiceStatus::Draining {
            return Err(ServiceError::Draining);
        }
        inner.status = ServiceStatus::Starting;
        inner.state = None;
        inner.detail = None;
        self.emit_locked(&mut inner, LifecycleEventKind::StartRequested, None);

        // A callback failure leaves the watcher snapshot behind the current
        // filesystem state. Preserve those refreshes across a degraded
        // restart and replay them before taking a new snapshot; otherwise a
        // fresh watcher would incorrectly acknowledge the failed work.
        if !inner.pending_refreshes.is_empty() {
            let pending = std::mem::take(&mut inner.pending_refreshes);
            let operation = self.operation.clone();
            let workspace_root = self.config.workspace_root.to_string_lossy().into_owned();
            for (index, event) in pending.iter().enumerate() {
                if let Err(error) = execute_refresh_event(&operation, &workspace_root, event) {
                    inner.pending_refreshes.extend(pending[index..].iter().cloned());
                    inner.status = ServiceStatus::Degraded;
                    inner.state = Some(LifecycleState::Stale);
                    inner.detail = Some(error.to_string());
                    self.emit_locked(&mut inner, LifecycleEventKind::Degraded, Some(error.to_string()));
                    return Err(ServiceError::Operation(error));
                }
            }
        }

        let watcher = match self.config.watcher.clone() {
            Some(config) => match NativeWatcher::start(config) {
                Ok(watcher) => Some(watcher),
                Err(error) => {
                    let detail = error.to_string();
                    inner.status = ServiceStatus::Degraded;
                    inner.state = Some(LifecycleState::WatcherUnavailable);
                    inner.detail = Some(detail.clone());
                    self.emit_locked(&mut inner, LifecycleEventKind::Degraded, Some(detail.clone()));
                    return Err(ServiceError::Watcher(detail));
                }
            },
            None => None,
        };
        inner.watcher = watcher;
        inner.status = ServiceStatus::Running;
        inner.state = Some(LifecycleState::Running);
        self.emit_locked(&mut inner, LifecycleEventKind::Started, None);
        self.emit_locked(&mut inner, LifecycleEventKind::Ready, None);
        Ok(ServiceStatus::Running)
    }

    /// Reconcile watcher state and return typed resident status. Watcher
    /// callbacks are only rebuild hints; graph semantics stay in operation.
    pub fn supervise(&self) -> ServiceStatus {
        let Ok(mut inner) = self.inner.lock() else {
            return ServiceStatus::Unavailable;
        };
        if inner.status != ServiceStatus::Running {
            return inner.status;
        }
        let operation = self.operation.clone();
        let workspace_root = self.config.workspace_root.to_string_lossy().into_owned();
        let mut pending_refreshes = std::mem::take(&mut inner.pending_refreshes);
        let mut callback_failure = None;
        let poll_result = inner.watcher.as_mut().map(|watcher| {
            watcher.poll(|event: &WatchEvent| {
                // NativeWatcher stops on a callback error. Keep polling after
                // the first failed event so every later event in this batch
                // is retained for ordered replay on degraded restart.
                if callback_failure.is_some() {
                    pending_refreshes.push(event.clone());
                    return Ok(());
                }
                match execute_refresh_event(&operation, &workspace_root, event) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        pending_refreshes.push(event.clone());
                        callback_failure = Some(error);
                        Ok(())
                    }
                }
            })
        });
        inner.pending_refreshes = pending_refreshes;
        if let Some(result) = poll_result {
            match result {
                Ok(events) => {
                    if let Some(error) = callback_failure {
                        let detail = format!("rebuild callback failed: {error}");
                        inner.status = ServiceStatus::Degraded;
                        inner.state = Some(if error.code == "deadline_exceeded" {
                            LifecycleState::Stale
                        } else {
                            LifecycleState::WatcherUnavailable
                        });
                        inner.detail = Some(detail.clone());
                        self.emit_locked(&mut inner, LifecycleEventKind::Degraded, Some(detail));
                        return inner.status;
                    }
                    if !events.is_empty() {
                        self.emit_locked(&mut inner, LifecycleEventKind::RebuildScheduled, Some(events.len().to_string()));
                    }
                }
                Err(error) => {
                    let detail = error.to_string();
                    inner.status = ServiceStatus::Degraded;
                    inner.state = Some(if detail.contains("deadline_exceeded:") {
                        LifecycleState::Stale
                    } else {
                        LifecycleState::WatcherUnavailable
                    });
                    inner.detail = Some(detail.clone());
                    self.emit_locked(&mut inner, LifecycleEventKind::Degraded, Some(detail));
                    return inner.status;
                }
            }
        }
        self.emit_locked(&mut inner, LifecycleEventKind::Supervised, None);
        inner.status
    }

    pub fn drain(&self) -> Result<(), ServiceError> {
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Watcher("service_state_unavailable".into()))?;
        if inner.status == ServiceStatus::Stopped {
            self.emit_locked(&mut inner, LifecycleEventKind::DrainIdempotent, None);
            return Ok(());
        }
        if inner.status == ServiceStatus::Draining {
            return Ok(());
        }
        inner.status = ServiceStatus::Draining;
        inner.state = Some(LifecycleState::Draining);
        self.emit_locked(&mut inner, LifecycleEventKind::DrainRequested, None);
        if let Some(watcher) = inner.watcher.as_mut() {
            watcher.shutdown().map_err(|error| ServiceError::Watcher(error.to_string()))?;
        }
        inner.watcher = None;
        inner.status = ServiceStatus::Stopped;
        inner.state = None;
        self.emit_locked(&mut inner, LifecycleEventKind::Drained, None);
        Ok(())
    }

    pub fn stop(&self) -> Result<(), ServiceError> {
        self.drain()
    }

    pub fn dispatch_request(
        &self,
        request: BlueprintRequest,
        cancellation: CancellationToken,
    ) -> BlueprintResponse {
        if self.status() != ServiceStatus::Running {
            return BlueprintResponse::failure(
                Some(request.request_id.clone()),
                BlueprintError::new("service_not_ready", "resident Blueprint service is not ready"),
            );
        }
        self.operation.dispatch(request, cancellation)
    }

    pub fn dispatch(
        &self,
        request: BlueprintRequest,
        cancellation: CancellationToken,
    ) -> BlueprintResponse {
        self.dispatch_request(request, cancellation)
    }

    fn emit_locked(&self, inner: &mut ServiceInner, kind: LifecycleEventKind, detail: Option<String>) {
        inner.sequence = inner.sequence.saturating_add(1);
        let event = LifecycleEvent {
            sequence: inner.sequence,
            kind,
            status: inner.status,
            state: inner.state,
            detail,
        };
        if inner.events.len() >= self.config.max_events {
            inner.events.remove(0);
        }
        inner.events.push(event.clone());
        if let Some(sink) = &self.sink {
            sink.observe(event);
        }
    }
}

fn execute_refresh_event(
    operation: &Arc<dyn BlueprintOperation>,
    workspace_root: &str,
    event: &WatchEvent,
) -> Result<(), BlueprintError> {
    let mut input = json!({
        "repoRoot": workspace_root,
        "paths": [event.path],
        "sourceClock": event.source_clock,
        "eventKind": format!("{:?}", event.kind).to_ascii_lowercase(),
    });
    if let Some(rename_to) = &event.rename_to {
        input["renameTo"] = json!(rename_to);
    }
    let mut request = BlueprintRequest::new(
        format!("watch-refresh-{}-{}", event.source_clock, event.path),
        Operation::Refresh,
        workspace_root,
    );
    request.input = input;
    let mut context = request.validate(Bounds::daemon())?;
    context.cancellation = CancellationToken::new();
    context.check()?;
    operation.execute(&request, &context)?;
    // A callback that returns after its deadline is not a successful refresh.
    context.check()
}

impl BlueprintApi for NativeService {
    fn dispatch(&self, request: BlueprintRequest, cancellation: CancellationToken) -> BlueprintResponse {
        self.dispatch_request(request, cancellation)
    }
}

/// Holder-independent bounded executor for explicit Blueprint operations.
pub struct OneShotExecutor {
    operation: Arc<dyn BlueprintOperation>,
    bounds: Bounds,
}

impl OneShotExecutor {
    pub fn new(operation: Arc<dyn BlueprintOperation>) -> Self {
        Self {
            operation,
            bounds: Bounds::one_shot(),
        }
    }

    pub fn from_operation<O>(operation: O) -> Self
    where
        O: BlueprintOperation + 'static,
    {
        Self::new(Arc::new(operation))
    }

    pub fn with_bounds(mut self, bounds: Bounds) -> Self {
        self.bounds = bounds;
        self
    }

    pub fn execute(&self, request: BlueprintRequest, cancellation: CancellationToken) -> BlueprintResponse {
        let request_id = request.request_id.clone();
        let response = match request.validate(self.bounds) {
            Ok(mut context) => {
                context.cancellation = cancellation;
                match context
                    .check()
                    .and_then(|_| self.operation.execute(&request, &context))
                    .and_then(|value| context.check().map(|_| value))
                {
                    Ok(value) => BlueprintResponse::success(request_id.clone(), request.generation.clone(), value),
                    Err(error) => BlueprintResponse::failure(Some(request_id.clone()), error),
                }
            }
            Err(error) => BlueprintResponse::failure(Some(request_id), error),
        };
        if let Err(error) = response.validate(self.bounds) {
            return BlueprintResponse::failure(response.request_id.clone(), error);
        }
        response
    }

    pub fn dispatch(&self, request: BlueprintRequest, cancellation: CancellationToken) -> BlueprintResponse {
        self.execute(request, cancellation)
    }
}

impl BlueprintApi for OneShotExecutor {
    fn dispatch(&self, request: BlueprintRequest, cancellation: CancellationToken) -> BlueprintResponse {
        self.execute(request, cancellation)
    }
}

pub fn execute_one_shot(
    operation: Arc<dyn BlueprintOperation>,
    request: BlueprintRequest,
    cancellation: CancellationToken,
) -> BlueprintResponse {
    OneShotExecutor::new(operation).execute(request, cancellation)
}

impl From<WatchError> for ServiceError {
    fn from(error: WatchError) -> Self {
        Self::Watcher(error.to_string())
    }
}
