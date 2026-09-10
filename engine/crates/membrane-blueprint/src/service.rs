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
use crate::watch::{Barrier, BarrierPoll, NativeWatcher, SnapshotConfig, WatchError, WatchEvent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
    /// Additional enrolled roots owned by this resident service. The first
    /// root remains available through `watcher` for source compatibility.
    pub additional_watchers: Vec<SnapshotConfig>,
    pub max_events: usize,
    pub debounce_ms: u64,
}

impl ServiceConfig {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        let workspace_root = workspace_root.into();
        Self {
            watcher: Some(SnapshotConfig::new(workspace_root.clone())),
            workspace_root,
            additional_watchers: Vec::new(),
            max_events: 256,
            debounce_ms: crate::watch::DEFAULT_DEBOUNCE_MS,
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

    pub fn with_watchers<I>(mut self, watchers: I) -> Self
    where
        I: IntoIterator<Item = SnapshotConfig>,
    {
        let mut watchers = watchers.into_iter();
        self.watcher = watchers.next();
        self.additional_watchers = watchers.collect();
        self
    }

    pub fn debounce_ms(mut self, debounce_ms: u64) -> Self {
        self.debounce_ms = debounce_ms.min(crate::watch::MAX_DEBOUNCE_MS);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HolderKind {
    Hub,
    CodeRight,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BarrierReceipt {
    pub repo_root: String,
    pub source_clock: u64,
    pub applied_clock: u64,
    pub event_gap: bool,
    pub barrier_result: crate::contracts::BarrierResult,
    pub generation_id: Option<String>,
    pub generation_complete: bool,
}

struct ServiceInner {
    status: ServiceStatus,
    state: Option<LifecycleState>,
    detail: Option<String>,
    sequence: u64,
    events: Vec<LifecycleEvent>,
    watchers: Vec<NativeWatcher>,
    pending_refreshes: Vec<WatchEvent>,
    generation_id: Option<String>,
    generation_complete: Option<bool>,
    active_cancellation: Option<CancellationToken>,
    holders: [u32; 2],
}

/// Resident native owner. The injected operation is the sole graph authority.
pub struct NativeService {
    operation: Arc<dyn BlueprintOperation>,
    config: ServiceConfig,
    sink: Option<Arc<dyn LifecycleEventSink>>,
    inner: Mutex<ServiceInner>,
    watcher_operation: Mutex<()>,
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
                watchers: Vec::new(),
                pending_refreshes: Vec::new(),
                generation_id: None,
                generation_complete: None,
                active_cancellation: None,
                holders: [0, 0],
            }),
            watcher_operation: Mutex::new(()),
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

    pub fn generation_metadata(&self) -> Option<(String, bool)> {
        self.inner.lock().ok().and_then(|inner| inner.generation_id.clone().zip(inner.generation_complete))
    }

    pub fn enrolled_roots(&self) -> Vec<PathBuf> {
        self.config.watcher.iter().map(|watcher| watcher.root.clone())
            .chain(self.config.additional_watchers.iter().map(|watcher| watcher.root.clone()))
            .collect()
    }

    pub fn holder_count(&self, kind: HolderKind) -> u32 {
        self.inner.lock().map(|inner| inner.holders[holder_index(kind)]).unwrap_or(0)
    }

    pub fn active_holder_count(&self) -> u32 {
        self.inner.lock().map(|inner| inner.holders.iter().copied().sum()).unwrap_or(0)
    }

    /// Acquire additive Hub/CodeRight residency. The first holder starts the
    /// service; releasing the final holder drains it, while either peer keeps
    /// watchers alive.
    pub fn acquire_holder(&self, kind: HolderKind) -> Result<(), ServiceError> {
        {
            let mut inner = self.inner.lock().map_err(|_| ServiceError::Watcher("service_state_unavailable".into()))?;
            inner.holders[holder_index(kind)] = inner.holders[holder_index(kind)].saturating_add(1);
        }
        if self.status() == ServiceStatus::Running { return Ok(()); }
        if let Err(error) = self.start() {
            if let Ok(mut inner) = self.inner.lock() {
                inner.holders[holder_index(kind)] = inner.holders[holder_index(kind)].saturating_sub(1);
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn release_holder(&self, kind: HolderKind) -> Result<(), ServiceError> {
        let should_drain = {
            let mut inner = self.inner.lock().map_err(|_| ServiceError::Watcher("service_state_unavailable".into()))?;
            let index = holder_index(kind);
            if inner.holders[index] == 0 { return Ok(()); }
            inner.holders[index] -= 1;
            inner.holders.iter().all(|count| *count == 0)
        };
        if should_drain { self.drain()?; }
        Ok(())
    }

    /// Fan out independent freshness barriers across all enrolled roots. A
    /// root without a complete generation is never reported caught-up.
    pub fn barrier_all(&self, deadline: Option<std::time::Duration>) -> Vec<BarrierReceipt> {
        let Ok(inner) = self.inner.lock() else { return Vec::new(); };
        let generation_id = inner.generation_id.clone();
        let generation_complete = inner.generation_complete == Some(true);
        self.enrolled_roots().into_iter().map(|root| {
            let watcher = inner.watchers.iter().find(|watcher| watcher.root() == root.as_path());
            let (source_clock, applied_clock, event_gap, mut barrier_result) = match watcher {
                Some(watcher) => {
                    let source_clock = watcher.source_clock();
                    let applied_clock = watcher.applied_clock();
                    let event_gap = watcher.gap().is_some();
                    let barrier_result = match watcher.barrier(Barrier { target_source_clock: source_clock, deadline }) {
                        BarrierPoll::Waiting => crate::contracts::BarrierResult::Timeout,
                        BarrierPoll::Complete(result) => result,
                    };
                    (source_clock, applied_clock, event_gap, barrier_result)
                }
                // Enrollment without an admitted watcher is explicitly stale;
                // it must not disappear from a barrier-all response.
                None => (0, 0, true, crate::contracts::BarrierResult::GapBlocked),
            };
            if !generation_complete && matches!(barrier_result, crate::contracts::BarrierResult::CaughtUp) {
                barrier_result = crate::contracts::BarrierResult::GapBlocked;
            }
            BarrierReceipt {
                repo_root: root.to_string_lossy().into_owned(),
                source_clock,
                applied_clock,
                event_gap,
                barrier_result,
                generation_id: generation_id.clone(),
                generation_complete,
            }
        }).collect()
    }

    pub fn start(&self) -> Result<ServiceStatus, ServiceError> {
        let _operation = self.watcher_operation.lock().map_err(|_| ServiceError::Watcher("service_state_unavailable".into()))?;
        let (pending, operation, workspace_root, cancellation) = {
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
        let cancellation = CancellationToken::new();
        inner.active_cancellation = Some(cancellation.clone());
        self.emit_locked(&mut inner, LifecycleEventKind::StartRequested, None);
            (std::mem::take(&mut inner.pending_refreshes), self.operation.clone(), self.config.workspace_root.to_string_lossy().into_owned(), cancellation)
        };

        // A callback failure leaves the watcher snapshot behind the current
        // filesystem state. Preserve those refreshes across a degraded
        // restart and replay them before taking a new snapshot; otherwise a
        // fresh watcher would incorrectly acknowledge the failed work.
        if !pending.is_empty() {
            for (index, event) in pending.iter().enumerate() {
                match execute_refresh_event(
                    &operation,
                    &workspace_root,
                    event,
                    cancellation.clone(),
                ) {
                    Ok(value) => self.record_generation(&value),
                    Err(error) => {
                        let mut inner = self.inner.lock().map_err(|_| ServiceError::Watcher("service_state_unavailable".into()))?;
                        inner.pending_refreshes.extend(pending[index..].iter().cloned());
                        inner.active_cancellation = None;
                        if inner.status == ServiceStatus::Draining {
                            return Err(ServiceError::Draining);
                        }
                        inner.status = ServiceStatus::Degraded;
                        inner.state = Some(LifecycleState::Stale);
                        inner.detail = Some(error.to_string());
                        self.emit_locked(&mut inner, LifecycleEventKind::Degraded, Some(error.to_string()));
                        return Err(ServiceError::Operation(error));
                    }
                }
            }
        }

        let mut watchers = Vec::new();
        let watcher_configs = self.config.watcher.iter().cloned()
            .chain(self.config.additional_watchers.iter().cloned())
            .map(|config| {
                // Preserve a root-specific debounce unless the service was
                // explicitly configured away from its legacy default.
                let debounce_ms = if self.config.debounce_ms == crate::watch::DEFAULT_DEBOUNCE_MS {
                    config.debounce_ms
                } else {
                    self.config.debounce_ms
                };
                let max_events = if self.config.max_events == 256 { config.max_events } else { self.config.max_events };
                config.max_events(max_events).debounce_ms(debounce_ms)
            });
        for config in watcher_configs {
            match NativeWatcher::start_with_cancellation(config, &cancellation) {
                Ok(watcher) => watchers.push(watcher),
                Err(error) => {
                    for started in &mut watchers { let _ = started.shutdown(); }
                    let detail = error.to_string();
                    let mut inner = self.inner.lock().map_err(|_| ServiceError::Watcher("service_state_unavailable".into()))?;
                    inner.active_cancellation = None;
                    if inner.status == ServiceStatus::Draining || cancellation.is_cancelled() {
                        return Err(ServiceError::Draining);
                    }
                    inner.status = ServiceStatus::Degraded;
                    inner.state = Some(LifecycleState::WatcherUnavailable);
                    inner.detail = Some(detail.clone());
                    self.emit_locked(&mut inner, LifecycleEventKind::Degraded, Some(detail.clone()));
                    return Err(ServiceError::Watcher(detail));
                }
            }
        }
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Watcher("service_state_unavailable".into()))?;
        if inner.status == ServiceStatus::Draining {
            inner.active_cancellation = None;
            inner.watchers = watchers;
            return Err(ServiceError::Draining);
        }
        inner.active_cancellation = None;
        inner.watchers = watchers;
        inner.status = ServiceStatus::Running;
        inner.state = Some(LifecycleState::Running);
        self.emit_locked(&mut inner, LifecycleEventKind::Started, None);
        self.emit_locked(&mut inner, LifecycleEventKind::Ready, None);
        Ok(ServiceStatus::Running)
    }

    /// Reconcile watcher state and return typed resident status. Watcher
    /// callbacks are only rebuild hints; graph semantics stay in operation.
    pub fn supervise(&self) -> ServiceStatus {
        self.supervise_with_cancellation(CancellationToken::new())
    }

    pub fn supervise_with_cancellation(&self, cancellation: CancellationToken) -> ServiceStatus {
        let Ok(_operation) = self.watcher_operation.lock() else {
            return ServiceStatus::Unavailable;
        };
        let (mut watchers, operation, mut pending_refreshes) = {
            let Ok(mut inner) = self.inner.lock() else { return ServiceStatus::Unavailable; };
            if inner.status != ServiceStatus::Running { return inner.status; }
            inner.status = ServiceStatus::Degraded;
            inner.state = Some(LifecycleState::Stale);
            inner.detail = Some("watcher_refresh_in_progress".into());
            inner.active_cancellation = Some(cancellation.clone());
            (std::mem::take(&mut inner.watchers), self.operation.clone(), std::mem::take(&mut inner.pending_refreshes))
        };
        let mut callback_failure = None;
        let mut latest_generation = None;
        let mut poll_error = None;
        let mut polled_events = 0usize;
        for watcher in &mut watchers {
            let root = watcher.root().to_string_lossy().into_owned();
            match watcher.poll_debounced(|event: &WatchEvent| {
                if callback_failure.is_some() {
                    pending_refreshes.push(event.clone());
                    return Ok(());
                }
                match execute_refresh_event(&operation, &root, event, cancellation.clone()) {
                    Ok(value) => {
                        if let (Some(id), Some(complete)) = (value.get("generationId").and_then(Value::as_str), value.get("complete").and_then(Value::as_bool)) {
                            latest_generation = Some((id.to_owned(), complete));
                        }
                        Ok(())
                    }
                    Err(error) => {
                        pending_refreshes.push(event.clone());
                        callback_failure = Some(error);
                        Ok(())
                    }
                }
            }, &cancellation) {
                Ok(events) => polled_events = polled_events.saturating_add(events.len()),
                Err(error) => { poll_error = Some(error); break; }
            }
        }
        let Ok(mut inner) = self.inner.lock() else { return ServiceStatus::Unavailable; };
        inner.active_cancellation = None;
        if inner.status == ServiceStatus::Draining {
            inner.pending_refreshes = pending_refreshes;
            inner.watchers = watchers;
            return inner.status;
        }
        inner.pending_refreshes = pending_refreshes;
        inner.watchers = watchers;
        if let Some((id, complete)) = latest_generation {
            inner.generation_id = Some(id);
            inner.generation_complete = Some(complete);
        }
        if let Some(error) = poll_error {
            if !matches!(error, WatchError::Snapshot(crate::watch::SnapshotError::Cancelled)) {
                let detail = error.to_string();
                inner.status = ServiceStatus::Degraded;
                inner.state = Some(if detail.contains("deadline_exceeded:") { LifecycleState::Stale } else { LifecycleState::WatcherUnavailable });
                inner.detail = Some(detail.clone());
                self.emit_locked(&mut inner, LifecycleEventKind::Degraded, Some(detail));
                return inner.status;
            }
        }
        if let Some(error) = callback_failure {
            let detail = format!("rebuild callback failed: {error}");
            inner.status = ServiceStatus::Degraded;
            inner.state = Some(if error.code == "deadline_exceeded" { LifecycleState::Stale } else { LifecycleState::WatcherUnavailable });
            inner.detail = Some(detail.clone());
            self.emit_locked(&mut inner, LifecycleEventKind::Degraded, Some(detail));
            return inner.status;
        }
        if polled_events > 0 {
            self.emit_locked(&mut inner, LifecycleEventKind::RebuildScheduled, Some(polled_events.to_string()));
        }
        inner.status = ServiceStatus::Running;
        inner.state = Some(LifecycleState::Running);
        inner.detail = None;
        self.emit_locked(&mut inner, LifecycleEventKind::Supervised, None);
        inner.status
    }

    pub fn drain(&self) -> Result<(), ServiceError> {
        {
            let mut inner = self.inner.lock().map_err(|_| ServiceError::Watcher("service_state_unavailable".into()))?;
            if inner.status == ServiceStatus::Stopped {
                self.emit_locked(&mut inner, LifecycleEventKind::DrainIdempotent, None);
                return Ok(());
            }
            if inner.status == ServiceStatus::Draining { return Ok(()); }
            inner.status = ServiceStatus::Draining;
            inner.state = Some(LifecycleState::Draining);
            if let Some(cancellation) = &inner.active_cancellation { cancellation.cancel(); }
            self.emit_locked(&mut inner, LifecycleEventKind::DrainRequested, None);
        }
        let _operation = self.watcher_operation.lock().map_err(|_| ServiceError::Watcher("service_state_unavailable".into()))?;
        let mut inner = self.inner.lock().map_err(|_| ServiceError::Watcher("service_state_unavailable".into()))?;
        let mut shutdown_error = None;
        for watcher in &mut inner.watchers {
            if let Err(error) = watcher.shutdown() {
                shutdown_error.get_or_insert_with(|| ServiceError::Watcher(error.to_string()));
            }
        }
        inner.watchers.clear();
        inner.status = ServiceStatus::Stopped;
        inner.state = None;
        self.emit_locked(&mut inner, LifecycleEventKind::Drained, None);
        shutdown_error.map_or(Ok(()), Err)
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
        let response = self.operation.dispatch(request, cancellation);
        if response.ok {
            if let Some(result) = response.result.as_ref() {
                self.record_generation(result);
            }
        }
        response
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

    fn record_generation(&self, value: &Value) {
        let Some(id) = value.get("generationId").and_then(Value::as_str) else { return; };
        let Some(complete) = value.get("complete").and_then(Value::as_bool) else { return; };
        if let Ok(mut inner) = self.inner.lock() {
            inner.generation_id = Some(id.to_owned());
            inner.generation_complete = Some(complete);
        }
    }
}

const fn holder_index(kind: HolderKind) -> usize {
    match kind {
        HolderKind::Hub => 0,
        HolderKind::CodeRight => 1,
    }
}

fn execute_refresh_event(
    operation: &Arc<dyn BlueprintOperation>,
    workspace_root: &str,
    event: &WatchEvent,
    cancellation: CancellationToken,
) -> Result<Value, BlueprintError> {
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
    // A watcher refresh performs graph work; use the existing Refresh cap
    // rather than the short interactive-query default. Build alone may use
    // the extended build deadline.
    request.deadline_ms = crate::model::MAX_DEADLINE_MS;
    request.input = input;
    let mut context = request.validate(Bounds::daemon())?;
    context.cancellation = cancellation.bind_deadline(context.deadline);
    context.check()?;
    let result = operation.execute(&request, &context)?;
    // A callback that returns after its deadline is not a successful refresh.
    context.check()?;
    Ok(result)
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
                context.cancellation = cancellation.bind_deadline(context.deadline);
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
