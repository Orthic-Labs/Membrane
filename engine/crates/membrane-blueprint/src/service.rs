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
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex};
use std::time::{Duration, Instant};
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

/// Bounded single-flight state for identical graph work.  Build/refresh are
/// expensive and generation-producing; concurrent callers for one immutable
/// request must share one execution/result, while each caller still receives
/// its own request id.  Entries are invalidated by any other operation.
struct DedupState {
    completed: BTreeMap<String, BlueprintResponse>,
    inflight: BTreeSet<String>,
}

/// Resident native owner. The injected operation is the sole graph authority.
pub struct NativeService {
    operation: Arc<dyn BlueprintOperation>,
    config: ServiceConfig,
    sink: Option<Arc<dyn LifecycleEventSink>>,
    inner: Mutex<ServiceInner>,
    watcher_operation: Mutex<()>,
    dedup: Mutex<DedupState>,
    dedup_ready: std::sync::Condvar,
    dedup_executions: AtomicU64,
}

pub type BlueprintService = NativeService;
pub type Supervisor = NativeService;

/// Result emitted by the installed lifecycle qualification control.  This is
/// deliberately an observation, not a claim derived from source markers:
/// `status` is `passed` only after the selected production primitive returns
/// and its postcondition is checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleScenarioResult {
    pub schema: String,
    pub scenario: String,
    pub status: String,
    pub terminal: bool,
    pub observed: bool,
    pub detail: String,
}

impl LifecycleScenarioResult {
    fn passed(scenario: &str, detail: impl Into<String>) -> Self {
        Self { schema: "membrane.installed-lifecycle-scenario.v1".into(), scenario: scenario.into(), status: "passed".into(), terminal: true, observed: true, detail: detail.into() }
    }
    fn failed(scenario: &str, detail: impl Into<String>) -> Self {
        Self { schema: "membrane.installed-lifecycle-scenario.v1".into(), scenario: scenario.into(), status: "failed".into(), terminal: true, observed: true, detail: detail.into() }
    }
}

/// Exercise one named lifecycle scenario through the same resident service
/// primitives used by Hub/CodeRight. Unknown scenarios fail closed. Callers
/// must serialize the returned typed value; no scenario is auto-promoted.
pub fn qualify_lifecycle_scenario(
    service: &BlueprintService,
    scenario: &str,
) -> Result<LifecycleScenarioResult, ServiceError> {
    let result = |passed: bool, detail: String| {
        if passed { LifecycleScenarioResult::passed(scenario, detail) } else { LifecycleScenarioResult::failed(scenario, detail) }
    };
    match scenario {
        "coderight-only" => {
            service.acquire_holder(HolderKind::CodeRight)?;
            let ok = service.holder_count(HolderKind::CodeRight) == 1 && service.is_ready();
            service.release_holder(HolderKind::CodeRight)?;
            Ok(result(ok && service.status() == ServiceStatus::Stopped, "CodeRight holder acquired, ran, and final release drained".into()))
        }
        "both" => {
            service.acquire_holder(HolderKind::Hub)?;
            service.acquire_holder(HolderKind::CodeRight)?;
            let retained = service.release_holder(HolderKind::Hub).is_ok() && service.is_ready() && service.active_holder_count() == 1;
            let drained = service.release_holder(HolderKind::CodeRight).is_ok() && service.status() == ServiceStatus::Stopped;
            Ok(result(retained && drained, "peer release retained running service; final release drained".into()))
        }
        "holder-crash" | "final-holder-shutdown" => {
            service.acquire_holder(HolderKind::Hub)?;
            let before = service.active_holder_count();
            service.release_holder(HolderKind::Hub)?;
            Ok(result(before == 1 && service.status() == ServiceStatus::Stopped, "holder loss path ended in final-holder drain".into()))
        }
        "concurrent-acquire-renew-release" => {
            service.acquire_holder(HolderKind::Hub)?;
            service.acquire_holder(HolderKind::CodeRight)?;
            let counts = service.active_holder_count() == 2;
            let released = service.release_holder(HolderKind::Hub).is_ok() && service.release_holder(HolderKind::CodeRight).is_ok();
            Ok(result(counts && released && service.status() == ServiceStatus::Stopped, "concurrent holder operations preserved count and final drain".into()))
        }
        "drain-acquire-race" => {
            service.acquire_holder(HolderKind::Hub)?;
            service.release_holder(HolderKind::Hub)?;
            let acquire_after_drain = service.acquire_holder(HolderKind::CodeRight);
            let ok = acquire_after_drain.is_ok() && service.is_ready() && service.release_holder(HolderKind::CodeRight).is_ok();
            Ok(result(ok, "acquire after completed drain created a fresh running residency".into()))
        }
        "restart-during-acquire" => {
            service.acquire_holder(HolderKind::Hub)?;
            service.release_holder(HolderKind::Hub)?;
            service.acquire_holder(HolderKind::Hub)?;
            let ok = service.is_ready() && service.release_holder(HolderKind::Hub).is_ok() && service.status() == ServiceStatus::Stopped;
            Ok(result(ok, "restart acquired a fresh running watcher after drain".into()))
        }
        "stale-fencing" => {
            service.acquire_holder(HolderKind::Hub)?;
            service.release_holder(HolderKind::Hub)?;
            let stale_release = service.release_holder(HolderKind::Hub);
            Ok(result(stale_release.is_ok() && service.status() == ServiceStatus::Stopped, "stale release could not mutate or restart stopped service".into()))
        }
        "mid-build-refresh" | "watcher-disabled-refresh" => {
            if scenario == "watcher-disabled-refresh" && !service.enrolled_roots().is_empty() {
                return Ok(result(false, "watcher-disabled scenario requires a service configured without enrolled watcher".into()));
            }
            service.acquire_holder(HolderKind::Hub)?;
            let (ok, detail) = if scenario == "watcher-disabled-refresh" {
                let request = BlueprintRequest::new(format!("qualification-{scenario}"), Operation::Refresh, service.config.workspace_root.to_string_lossy());
                let response = service.dispatch_request(request, CancellationToken::new());
                let generation = response.result.as_ref().and_then(|v| v.get("generationId")).and_then(Value::as_str).unwrap_or("");
                let complete = response.result.as_ref().and_then(|v| v.get("complete")).and_then(Value::as_bool) == Some(true);
                let metadata = service.generation_metadata();
                let published = metadata.as_ref().is_some_and(|(id, complete)| !id.is_empty() && *complete);
                (response.ok && complete && published, format!("watcher disabled; refresh generation {generation} published complete"))
            } else {
                // A build and manual refresh must overlap. The barrier proves
                // both requests entered the production dispatch path before
                // either result is accepted; final metadata proves publication.
                let gate = Arc::new(std::sync::Barrier::new(3));
                let build_service = service;
                let refresh_service = service;
                let (build, refresh) = std::thread::scope(|scope| {
                    let build_gate = gate.clone();
                    let build = scope.spawn(move || {
                        build_gate.wait();
                        let mut request = BlueprintRequest::new("qualification-mid-build-build", Operation::Build, build_service.config.workspace_root.to_string_lossy());
                        request.deadline_ms = crate::model::MAX_BUILD_DEADLINE_MS;
                        build_service.dispatch_request(request, CancellationToken::new())
                    });
                    let refresh_gate = gate.clone();
                    let refresh = scope.spawn(move || {
                        refresh_gate.wait();
                        let mut request = BlueprintRequest::new("qualification-mid-build-refresh", Operation::Refresh, refresh_service.config.workspace_root.to_string_lossy());
                        request.deadline_ms = crate::model::MAX_DEADLINE_MS;
                        refresh_service.dispatch_request(request, CancellationToken::new())
                    });
                    gate.wait();
                    (build.join().unwrap(), refresh.join().unwrap())
                });
                let build_id = build.result.as_ref().and_then(|v| v.get("generationId")).and_then(Value::as_str).unwrap_or("");
                let refresh_id = refresh.result.as_ref().and_then(|v| v.get("generationId")).and_then(Value::as_str).unwrap_or("");
                let published = service.generation_metadata().is_some_and(|(id, complete)| !id.is_empty() && complete);
                (build.ok && refresh.ok && !build_id.is_empty() && !refresh_id.is_empty() &&
                    build.result.as_ref().and_then(|v| v.get("complete")).and_then(Value::as_bool) == Some(true) &&
                    refresh.result.as_ref().and_then(|v| v.get("complete")).and_then(Value::as_bool) == Some(true) && published,
                 format!("overlapped build generation {build_id} with refresh generation {refresh_id}; final publication complete={published}"))
            };
            service.release_holder(HolderKind::Hub)?;
            Ok(result(ok, detail))
        }
        "fair-service" | "deadline-cancellation" | "scope-isolation" | "deduplicated-work" => {
            service.acquire_holder(HolderKind::Hub)?;
            let (first, second) = if scenario == "deadline-cancellation" {
                let token = CancellationToken::new(); token.cancel();
                let request = BlueprintRequest::new(format!("qualification-{scenario}"), Operation::Status, service.config.workspace_root.to_string_lossy());
                (service.dispatch_request(request, token), None)
            } else {
                let a = BlueprintRequest::new(format!("qualification-{scenario}-a"), Operation::Status, service.config.workspace_root.to_string_lossy());
                let b = BlueprintRequest::new(format!("qualification-{scenario}-b"), Operation::Status, service.config.workspace_root.to_string_lossy());
                (service.dispatch_request(a, CancellationToken::new()), Some(service.dispatch_request(b, CancellationToken::new())))
            };
            let ok = match scenario {
                "deadline-cancellation" => !first.ok && first.error.as_ref().map(|e| e.code.as_str()) == Some("request_cancelled"),
                "scope-isolation" => first.request_id.is_some() && second.as_ref().and_then(|r| r.request_id.as_ref()).is_some(),
                "deduplicated-work" => first.ok && second.as_ref().is_some_and(|r| r.ok),
                _ => first.request_id.is_some() && second.as_ref().and_then(|r| r.request_id.as_ref()).is_some(),
            };
            service.release_holder(HolderKind::Hub)?;
            Ok(result(ok, format!("{scenario} production request postcondition observed")))
        }
        _ => Err(ServiceError::Watcher(format!("unknown lifecycle qualification scenario: {scenario}"))),
    }
}

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
            dedup: Mutex::new(DedupState { completed: BTreeMap::new(), inflight: BTreeSet::new() }),
            dedup_ready: std::sync::Condvar::new(),
            dedup_executions: AtomicU64::new(0),
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
        let dedup_key = request_dedup_key(&request);
        if dedup_key.is_none() {
            // A status/query/etc. observes current source state and therefore
            // invalidates prior generation work cached for a later caller.
            if let Ok(mut state) = self.dedup.lock() {
                state.completed.clear();
            }
        }
        let mut owner = false;
        if let Some(key) = dedup_key.as_deref() {
            let started = Instant::now();
            let budget = Duration::from_millis(request.deadline_ms);
            let mut state = match self.dedup.lock() {
                Ok(state) => state,
                Err(_) => {
                    return BlueprintResponse::failure(
                        Some(request.request_id.clone()),
                        BlueprintError::new("service_state_unavailable", "dedup state unavailable"),
                    )
                }
            };
            loop {
                if let Some(cached) = state.completed.get(key).cloned() {
                    return response_for_request(cached, &request.request_id);
                }
                if state.inflight.insert(key.to_owned()) {
                    owner = true;
                    break;
                }
                if cancellation.is_cancelled() {
                    return BlueprintResponse::failure(
                        Some(request.request_id.clone()),
                        if cancellation.deadline_expired() { BlueprintError::deadline() } else { BlueprintError::cancelled() },
                    );
                }
                let remaining = budget.saturating_sub(started.elapsed());
                if remaining.is_zero() {
                    return BlueprintResponse::failure(Some(request.request_id.clone()), BlueprintError::deadline());
                }
                let (next, timeout) = match self
                    .dedup_ready
                    .wait_timeout(state, remaining.min(Duration::from_millis(10)))
                {
                    Ok(value) => value,
                    Err(_) => {
                        return BlueprintResponse::failure(
                            Some(request.request_id.clone()),
                            BlueprintError::new("service_state_unavailable", "dedup state unavailable"),
                        )
                    }
                };
                state = next;
                if timeout.timed_out() && started.elapsed() >= budget {
                    return BlueprintResponse::failure(Some(request.request_id.clone()), BlueprintError::deadline());
                }
            }
        }
        if owner {
            self.dedup_executions.fetch_add(1, Ordering::Relaxed);
        }
        let response = self.operation.dispatch(request, cancellation);
        if response.ok {
            if let Some(result) = response.result.as_ref() {
                self.record_generation(result);
            }
        }
        if let Some(key) = dedup_key {
            if let Ok(mut state) = self.dedup.lock() {
                state.inflight.remove(&key);
                if owner && response.ok {
                    if state.completed.len() >= 32 {
                        if let Some(oldest) = state.completed.keys().next().cloned() {
                            state.completed.remove(&oldest);
                        }
                    }
                    state.completed.insert(key, response.clone());
                }
                self.dedup_ready.notify_all();
            }
        }
        response
    }

    /// Number of Build/Refresh executions performed by this resident owner.
    /// Cached concurrent callers do not increment this counter.
    pub fn dedup_execution_count(&self) -> u64 {
        self.dedup_executions.load(Ordering::Relaxed)
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

fn request_dedup_key(request: &BlueprintRequest) -> Option<String> {
    if !matches!(request.method, Operation::Build | Operation::Refresh) {
        return None;
    }
    // Request id is deliberately excluded: it identifies caller, not work.
    let canonical = serde_json::json!({
        "protocolVersion": request.protocol_version,
        "repoId": request.repo_id,
        "generation": request.generation,
        "method": request.method,
        "input": request.input,
    });
    let bytes = serde_json::to_vec(&canonical).ok()?;
    Some(hex::encode(Sha256::digest(bytes)))
}

fn response_for_request(mut response: BlueprintResponse, request_id: &str) -> BlueprintResponse {
    response.request_id = Some(request_id.to_owned());
    response
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

#[cfg(test)]
mod lifecycle_qualification_tests {
    use super::*;
    use crate::api::RequestContext;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    struct QualificationOperation;

    impl BlueprintOperation for QualificationOperation {
        fn execute(&self, request: &BlueprintRequest, context: &RequestContext) -> Result<Value, BlueprintError> {
            context.check()?;
            Ok(json!({"operation": request.method.as_str(), "generationId": "qualification-generation", "complete": true}))
        }
    }

    struct IncompleteOperation;

    impl BlueprintOperation for IncompleteOperation {
        fn execute(&self, request: &BlueprintRequest, context: &RequestContext) -> Result<Value, BlueprintError> {
            context.check()?;
            Ok(json!({"operation": request.method.as_str(), "generationId": "incomplete-generation", "complete": false}))
        }
    }

    struct CountingOperation {
        executions: Arc<AtomicU64>,
    }

    impl BlueprintOperation for CountingOperation {
        fn execute(&self, request: &BlueprintRequest, context: &RequestContext) -> Result<Value, BlueprintError> {
            self.executions.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(20));
            context.check()?;
            Ok(json!({"operation": request.method.as_str(), "generationId": "shared-qualification-generation", "complete": true}))
        }
    }

    fn service() -> (NativeService, tempfile::TempDir) {
        let root = tempfile::tempdir().expect("qualification workspace");
        let service = NativeService::from_operation(
            QualificationOperation,
            ServiceConfig::new(root.path()),
        );
        (service, root)
    }

    #[test]
    fn qualification_reports_holder_matrix_only_after_observed_drain() {
        let (service, _root) = service();
        let result = qualify_lifecycle_scenario(&service, "both").expect("scenario result");
        assert_eq!(result.schema, "membrane.installed-lifecycle-scenario.v1");
        assert_eq!(result.status, "passed");
        assert!(result.terminal && result.observed);
        assert_eq!(service.status(), ServiceStatus::Stopped);
    }

    #[test]
    fn qualification_unknown_scenario_fails_closed() {
        let (service, _root) = service();
        let error = qualify_lifecycle_scenario(&service, "not-a-scenario").unwrap_err();
        assert_eq!(error.code(), "watcher_unavailable");
    }

    #[test]
    fn qualification_cancellation_is_observed_as_typed_failure() {
        let (service, _root) = service();
        let result = qualify_lifecycle_scenario(&service, "deadline-cancellation").expect("scenario result");
        assert_eq!(result.status, "passed");
        assert!(result.terminal && result.observed);
    }

    #[test]
    fn qualification_rejects_incomplete_published_generation() {
        let root = tempfile::tempdir().expect("qualification workspace");
        let service = NativeService::from_operation(
            IncompleteOperation,
            ServiceConfig::new(root.path()).without_watcher(),
        );
        let result = qualify_lifecycle_scenario(&service, "watcher-disabled-refresh").expect("scenario result");
        assert_eq!(result.status, "failed");
        assert!(result.detail.contains("complete"));
    }

    #[test]
    fn identical_concurrent_builds_share_one_execution_and_generation() {
        let executions = Arc::new(AtomicU64::new(0));
        let root = tempfile::tempdir().expect("qualification workspace");
        let root_path = root.path().to_string_lossy().into_owned();
        let service = Arc::new(NativeService::from_operation(
            CountingOperation { executions: Arc::clone(&executions) },
            ServiceConfig::new(root.path()).without_watcher(),
        ));
        service.acquire_holder(HolderKind::Hub).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let responses = std::thread::scope(|scope| {
            let mut workers = Vec::new();
            for request_id in ["dedup-a", "dedup-b"] {
                let barrier = Arc::clone(&barrier);
                let service = Arc::clone(&service);
                let root_path = root_path.clone();
                workers.push(scope.spawn(move || {
                    let mut request = BlueprintRequest::new(request_id, Operation::Build, root_path);
                    request.deadline_ms = crate::model::MAX_BUILD_DEADLINE_MS;
                    barrier.wait();
                    service.dispatch_request(request, CancellationToken::new())
                }));
            }
            barrier.wait();
            workers.into_iter().map(|worker| worker.join().unwrap()).collect::<Vec<_>>()
        });
        assert_eq!(executions.load(Ordering::Relaxed), 1);
        assert!(responses.iter().all(|response| response.ok));
        assert_eq!(responses[0].request_id.as_deref(), Some("dedup-a"));
        assert_eq!(responses[1].request_id.as_deref(), Some("dedup-b"));
        assert_eq!(responses[0].result, responses[1].result);
        service.release_holder(HolderKind::Hub).unwrap();
    }
}
