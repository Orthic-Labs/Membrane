#![cfg_attr(windows, windows_subsystem = "windows")]

//! Console-free Windows entrypoint for the installed resident service. No shell wrapper or
//! visible console host sits between its lifecycle owner and the resident service.

use serde::Deserialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use membrane_blueprint::{BlueprintRequest, CancellationToken, NativeService, Operation, ServiceStatus};

const LIFECYCLE_READY_WAIT: std::time::Duration = std::time::Duration::from_secs(30);
const RESIDENT_SUPERVISOR_STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// Ephemeral capability received from Membrane lifecycle channel.
/// It is copied into process memory at startup, never persisted.
#[derive(Clone)]
pub struct LifecycleControl {
    snapshot_capability: Option<Arc<str>>,
    admission_open: Arc<AtomicBool>,
    background_authority: Arc<AtomicBool>,
    shutdown_requested: Arc<AtomicBool>,
    shutdown_cancellation: tokio_util::sync::CancellationToken,
    ready: Arc<(Mutex<Option<u16>>, Condvar)>,
    command: Arc<Mutex<Option<String>>>,
    failure: Arc<Mutex<Option<String>>>,
}

impl Default for LifecycleControl {
    fn default() -> Self {
        Self {
            snapshot_capability: None,
            admission_open: Arc::new(AtomicBool::new(true)),
            background_authority: Arc::new(AtomicBool::new(false)),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
            shutdown_cancellation: tokio_util::sync::CancellationToken::new(),
            ready: Arc::new((Mutex::new(None), Condvar::new())),
            command: Arc::new(Mutex::new(None)),
            failure: Arc::new(Mutex::new(None)),
        }
    }
}

impl LifecycleControl {
    /// Bind a capability received from a validated lifecycle hello frame.
    /// The caller must retain no copy after this handoff.
    pub fn from_lifecycle_capability(capability: &str) -> Result<Self, String> {
        if capability.len() != 64
            || !capability
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err("lifecycle snapshot capability invalid".into());
        }
        Ok(Self {
            snapshot_capability: Some(Arc::<str>::from(capability)),
            ..Self::default()
        })
    }

    pub fn snapshot_authorized(&self, supplied: Option<&str>) -> bool {
        let (Some(expected), Some(actual)) = (&self.snapshot_capability, supplied) else {
            return false;
        };
        if expected.len() != actual.len() {
            return false;
        }
        expected
            .as_bytes()
            .iter()
            .zip(actual.as_bytes())
            .fold(0u8, |difference, (left, right)| difference | (left ^ right))
            == 0
    }

    fn hub_bound(&self) -> bool {
        self.snapshot_capability.is_some()
    }

    pub fn admission_open(&self) -> bool {
        self.admission_open.load(Ordering::Acquire)
    }

    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::Acquire)
    }

    /// Descendant work observes engine drain without owning engine lifetime.
    pub(crate) fn cancellation_token(&self) -> tokio_util::sync::CancellationToken {
        self.shutdown_cancellation.child_token()
    }

    /// Background work is independently authorized from engine admission.
    /// Losing the final holder drains this flag while preserving explicit
    /// request availability on the resident engine.
    pub fn background_authority_open(&self) -> bool {
        self.background_authority.load(Ordering::Acquire)
    }

    pub fn grant_background(&self, reason: &str) {
        let was_open = self
            .background_authority
            .swap(true, Ordering::AcqRel);
        if !was_open {
            emit_lifecycle("background_authority_granted", Some(reason));
        }
    }

    pub fn drain_background(&self, reason: &str) {
        let was_open = self
            .background_authority
            .swap(false, Ordering::AcqRel);
        if was_open {
            emit_lifecycle("background_authority_drained", Some(reason));
        }
    }

    pub fn request_drain(&self, command: Option<&str>) {
        if let Some(command) = command {
            if let Ok(mut current) = self.command.lock() {
                if current.is_none() {
                    *current = Some(command.to_string());
                }
            }
        }
        self.admission_open.store(false, Ordering::Release);
        self.background_authority.store(false, Ordering::Release);
        let first_drain = !self
            .shutdown_requested
            .swap(true, Ordering::AcqRel);
        self.shutdown_cancellation.cancel();
        if first_drain {
            let reason = self
                .command()
                .or_else(|| self.failure())
                .unwrap_or_else(|| "unspecified".to_string());
            emit_lifecycle("drain_requested", Some(&reason));
        }
        self.ready.1.notify_all();
    }

    pub fn fail(&self, reason: impl Into<String>) {
        if let Ok(mut failure) = self.failure.lock() {
            if failure.is_none() {
                *failure = Some(reason.into());
            }
        }
        self.request_drain(None);
    }

    pub fn failure(&self) -> Option<String> {
        self.failure.lock().ok().and_then(|value| value.clone())
    }

    pub fn command(&self) -> Option<String> {
        self.command.lock().ok().and_then(|value| value.clone())
    }

    pub(crate) fn mark_ready(&self, port: u16) {
        if let Ok(mut ready) = self.ready.0.lock() {
            *ready = Some(port);
            self.ready.1.notify_all();
        }
        emit_lifecycle("ready", Some("transport"));
    }

    pub fn wait_until_ready(&self) -> Result<u16, String> {
        let ready = self
            .ready
            .0
            .lock()
            .map_err(|_| "lifecycle ready state unavailable".to_string())?;
        let (ready, timeout) = self
            .ready
            .1
            .wait_timeout_while(ready, LIFECYCLE_READY_WAIT, |port| {
                port.is_none() && !self.shutdown_requested()
            })
            .map_err(|_| "lifecycle ready state unavailable".to_string())?;
        if self.shutdown_requested() {
            return Err(self
                .failure()
                .unwrap_or_else(|| "lifecycle startup stopped before ready".to_string()));
        }
        if let Some(port) = *ready {
            return Ok(port);
        }
        if timeout.timed_out() {
            return Err("lifecycle startup timeout".to_string());
        }
        Err(self
            .failure()
            .unwrap_or_else(|| "lifecycle startup stopped before ready".to_string()))
    }
}

static LIFECYCLE_CONTROL: OnceLock<RwLock<LifecycleControl>> = OnceLock::new();
static HUB_RUNTIME_ACTIVE: AtomicBool = AtomicBool::new(false);

struct ResidentRepo {
    root: String,
    service: Arc<NativeService>,
    generation_id: String,
    generation_complete: bool,
}

struct ResidentBlueprintState {
    repos: Vec<ResidentRepo>,
    enrolled_repo_count: u64,
    registry_error: Option<String>,
    cancellation: CancellationToken,
    /// Per-root build backoff: consecutive failure count and last attempt.
    /// Each attempt is deadline-bounded, but a repository that keeps failing
    /// (e.g. a stale multi-GB enrollment) would otherwise re-burn its whole
    /// budget every reconcile pass and starve the other enrolled roots.
    build_failures: std::collections::HashMap<String, (u32, Instant)>,
    /// What the supervisor thread is currently doing. Stop timeouts name this
    /// stage so a drain timeout is attributable without a live debugger.
    supervisor_stage: Arc<Mutex<String>>,
}

fn set_supervisor_stage(stage: &Arc<Mutex<String>>, value: &str) {
    if let Ok(mut current) = stage.lock() {
        *current = value.to_owned();
    }
}

fn supervisor_stage(stage: &Arc<Mutex<String>>) -> String {
    stage.lock().map(|current| current.clone()).unwrap_or_else(|_| "unknown".to_owned())
}

/// Bounded exponential cooldown between rebuild attempts for a root that
/// keeps failing: 120s, 240s, 480s, then capped at 600s so an unrecoverable
/// enrollment never starves the healthy roots' reconcile passes.
fn retry_cooldown(attempts: u32) -> Duration {
    std::cmp::min(
        Duration::from_secs(120) * 2u32.saturating_pow(attempts.saturating_sub(1)),
        Duration::from_secs(600),
    )
}

fn retry_due(
    failures: &std::collections::HashMap<String, (u32, Instant)>,
    key: &str,
    now: Instant,
) -> bool {
    match failures.get(key) {
        Some((attempts, last)) => now.saturating_duration_since(*last) >= retry_cooldown(*attempts),
        None => true,
    }
}

struct ResidentBlueprint {
    state: Arc<Mutex<ResidentBlueprintState>>,
    lifecycle: LifecycleControl,
    supervisor: Option<std::thread::JoinHandle<()>>,
}

static RESIDENT_BLUEPRINT: OnceLock<Mutex<Option<ResidentBlueprint>>> = OnceLock::new();

/// Start the one resident native Blueprint watcher for the installed Hub.
/// Enrollment is read from the canonical installation registry; no watcher
/// is created for an absent or malformed registry. The returned status is
/// consumed by health, while this same service owns polling, refresh and drain.
pub fn start_resident_blueprint() -> Result<(), String> {
    let slot = RESIDENT_BLUEPRINT.get_or_init(|| Mutex::new(None));
    let mut current = slot
        .lock()
        .map_err(|_| "resident Blueprint state unavailable".to_string())?;
    if let Some(existing) = current.as_mut() {
        let running = existing
            .state
            .lock()
            .map(|state| (state.enrolled_repo_count == 0 || state.repos.len() == state.enrolled_repo_count as usize)
                && state.repos.iter().all(|repo| repo.service.status() == ServiceStatus::Running))
            .unwrap_or(false);
        if running {
            return Ok(());
        }
        // A failed supervisor leaves a terminal service object behind. Stop it
        // while the singleton slot is still occupied; never detach a worker
        // that may still own writable repository services.
        existing.stop_bounded(RESIDENT_SUPERVISOR_STOP_TIMEOUT)?;
    }
    current.take();
    let registry = crate::authorization::load_installation_registry().map_err(|error| error.to_string())?;
    let roots = enrolled_roots(&registry)?;
    let cancellation = CancellationToken::new();
    let supervisor_stage = Arc::new(Mutex::new(String::from("startup")));
    let enrolled_repo_count = roots.len() as u64;
    let state = Arc::new(Mutex::new(ResidentBlueprintState {
        enrolled_repo_count,
        repos: Vec::new(),
        registry_error: None,
        cancellation,
        build_failures: Default::default(),
        supervisor_stage: Arc::clone(&supervisor_stage),
    }));
    let lifecycle = lifecycle_control();
    let supervised = Arc::clone(&state);
    let supervisor_lifecycle = lifecycle.clone();
    let supervisor = std::thread::Builder::new()
        .name("membrane-blueprint-resident-watcher".into())
        .spawn(move || {
            eprintln!("{}", serde_json::json!({"event":"resident_blueprint_initialization", "stage":"started", "enrolledRepoCount": enrolled_repo_count}));
            let mut ledger_maintenance_at = Instant::now();
            while !supervisor_lifecycle.shutdown_requested() {
                if supervisor_lifecycle.background_authority_open() {
                    set_supervisor_stage(&supervisor_stage, "supervise");
                    if !supervise_resident_repositories(&supervised) {
                        break;
                    }
                    set_supervisor_stage(&supervisor_stage, "reconcile");
                    reconcile_resident_repositories(&supervised, &supervisor_stage);
                    if Instant::now() >= ledger_maintenance_at {
                        set_supervisor_stage(&supervisor_stage, "ledger_maintain");
                        reconcile_resident_ledger(&supervised);
                        ledger_maintenance_at = Instant::now() + LEDGER_MAINTENANCE_INTERVAL;
                    }
                } else {
                    // Holder loss drains automatic repository work while the
                    // engine/listener remains available for explicit calls.
                    set_supervisor_stage(&supervisor_stage, "holder_drain");
                    drain_resident_repositories(&supervised);
                }
                set_supervisor_stage(&supervisor_stage, "idle");
                std::thread::sleep(Duration::from_millis(250));
            }
            eprintln!("{}", serde_json::json!({"event":"resident_blueprint_initialization", "stage":"stopped"}));
            drain_resident_repositories(&supervised);
        })
        .map_err(|error| {
            drain_resident_repositories(&state);
            format!("resident Blueprint supervisor unavailable: {error}")
        })?;
    *current = Some(ResidentBlueprint {
        state,
        lifecycle,
        supervisor: Some(supervisor),
    });
    Ok(())
}

fn enrolled_roots(registry: &crate::authorization::InstallationRegistryV1) -> Result<Vec<PathBuf>, String> {
    // A registry binding whose repository has since been deleted, moved, or is
    // otherwise unreachable must NOT take the whole resident down: the resident
    // serves every still-enrolled repository and records the unavailable ones as
    // omissions. Canonicalize-or-skip keeps one stale binding (e.g. a cleaned-up
    // qualification temp repo) from crash-looping the daemon, honouring the
    // "record material omissions/degradation" invariant instead of failing closed
    // on state the operator can no longer influence.
    let mut roots = Vec::new();
    for binding in registry.bindings() {
        match PathBuf::from(&binding.root).canonicalize() {
            Ok(root) if root.is_dir() => roots.push(root),
            Ok(root) => {
                eprintln!(
                    "{}",
                    serde_json::json!({"event":"resident_blueprint_enrollment_skipped","reason":"not_a_directory","root":root})
                );
            }
            Err(error) => {
                eprintln!(
                    "{}",
                    serde_json::json!({"event":"resident_blueprint_enrollment_skipped","reason":"unavailable","root":binding.root,"error":error.to_string()})
                );
            }
        }
    }
    roots.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
    roots.dedup();
    Ok(roots)
}

fn build_resident_repo(root: &Path, cancellation: &CancellationToken) -> Result<ResidentRepo, String> {
    let started = std::time::Instant::now();
    eprintln!("{}", serde_json::json!({"event":"resident_blueprint_repository_build", "stage":"started", "root":root}));
    let service = Arc::new(NativeService::resident(membrane_blueprint::NativeBlueprintOperation, root.to_path_buf()));
    service.start().map_err(|error| format!("resident Blueprint startup service start: {error}"))?;
    let mut build = BlueprintRequest::new(format!("resident-build-{}", std::process::id()), Operation::Build, root.to_string_lossy());
    // Cold first-build of a large enrolled repo can exceed two minutes under
    // in-resident contention (a second repo's watcher plus daemon overhead push
    // a repo that builds in ~80s standalone past a 120s ceiling). Give the
    // initial build room to finish and persist its generation; once cached,
    // subsequent starts refresh incrementally and are fast. Without this the
    // build is cancelled at the deadline, never caches, and retries cold
    // forever — leaving the Hub permanently short of full watcher coverage.
    build.deadline_ms = membrane_blueprint::model::MAX_BUILD_DEADLINE_MS;
    let response = service.dispatch(build, cancellation.clone());
    eprintln!("{}", serde_json::json!({"event":"resident_blueprint_repository_build", "stage":if response.ok { "completed" } else { "failed" }, "root":root, "elapsedMs":started.elapsed().as_millis()}));
    let generation_id = response.result.as_ref().and_then(|result| result.get("generationId")).and_then(serde_json::Value::as_str).map(str::to_owned);
    let complete = response.result.as_ref().and_then(|result| result.get("complete")).and_then(serde_json::Value::as_bool) == Some(true);
    if !response.ok || generation_id.is_none() {
        let detail = response.error.map(|error| format!("resident Blueprint startup initial build {}: {}", error.code, error.message)).unwrap_or_else(|| "resident Blueprint startup initial build failed".into());
        let _ = service.drain();
        return Err(detail);
    }
    let Some(generation_id) = generation_id else {
        let _ = service.drain();
        return Err("resident Blueprint startup initial build returned no generation".into());
    };
    Ok(ResidentRepo { root: root.to_string_lossy().into_owned(), service, generation_id, generation_complete: complete })
}

fn supervise_resident_repositories(state: &Arc<Mutex<ResidentBlueprintState>>) -> bool {
    let services = {
        let Ok(mut state) = state.lock() else { return false };
        state.repos.retain(|repo| repo.service.status() == ServiceStatus::Running);
        state.repos.iter().map(|repo| (repo.root.clone(), Arc::clone(&repo.service))).collect::<Vec<_>>()
    };
    let cancellation = state
        .lock()
        .map(|state| state.cancellation.clone())
        .unwrap_or_default();
    // One degraded watcher retires only that repository — never the whole
    // resident. A deleted or unwatched enrolled root (e.g. a cleaned-up
    // qualification temp repo) otherwise took every healthy repo's watcher
    // down with it and left the Hub reporting watcher_unavailable until the
    // next engine start. Reconcile owns enrollment truth and drops or
    // rebuilds the retired root on the next pass.
    let mut failed = Vec::new();
    for (root, service) in &services {
        if service.supervise_with_cancellation(cancellation.clone()) != ServiceStatus::Running {
            let readiness = service.readiness();
            let detail = format!("resident Blueprint watcher {root}: {:?}: {}", readiness.status,
                readiness.detail.as_deref().unwrap_or("watcher unavailable without detail"));
            eprintln!("{}", serde_json::json!({"event":"resident_blueprint_supervision", "stage":"failed", "root":root, "error":detail}));
            failed.push((root.clone(), detail));
        }
    }
    if !failed.is_empty() {
        let retired = {
            let Ok(mut state) = state.lock() else { return false };
            state.registry_error = failed.first().map(|(_, detail)| detail.clone());
            // A retired-but-still-enrolled root is rebuilt by reconcile; feed
            // the same backoff as a failed build so a flapping watcher cannot
            // churn unbounded rebuild work.
            for (root, _) in &failed {
                let entry = state.build_failures.entry(root.clone()).or_insert((0, Instant::now()));
                entry.0 = entry.0.saturating_add(1);
                entry.1 = Instant::now();
            }
            let failed_roots = failed.iter().map(|(root, _)| root.clone()).collect::<std::collections::BTreeSet<_>>();
            let mut retired = Vec::new();
            state.repos.retain(|repo| {
                if failed_roots.contains(&repo.root) { retired.push(Arc::clone(&repo.service)); false } else { true }
            });
            retired
        };
        for service in retired { let _ = service.drain(); }
        return true;
    }
    if let Ok(mut state) = state.lock() {
        for repo in &mut state.repos {
            if let Some((generation_id, complete)) = repo.service.generation_metadata() {
                repo.generation_id = generation_id;
                repo.generation_complete = complete;
            }
        }
    }
    true
}

fn reconcile_resident_repositories(state: &Arc<Mutex<ResidentBlueprintState>>, supervisor_stage: &Arc<Mutex<String>>) {
    let registry = match crate::authorization::load_installation_registry() {
        Ok(registry) => registry,
        Err(error) => {
            mark_registry_error(state, error.to_string());
            return;
        }
    };
    let roots = match enrolled_roots(&registry) {
        Ok(roots) => roots,
        Err(error) => {
            mark_registry_error(state, error);
            return;
        }
    };
    let desired = roots.iter().map(|root| root.to_string_lossy().into_owned()).collect::<std::collections::BTreeSet<_>>();
    let (existing, cancellation, failures) = {
        let Ok(mut state) = state.lock() else { return };
        state.enrolled_repo_count = desired.len() as u64;
        let mut removed = Vec::new();
        state.repos.retain(|repo| {
            let keep = desired.contains(&repo.root);
            if !keep { removed.push(Arc::clone(&repo.service)); }
            keep
        });
        drop(removed);
        (
            state.repos.iter().map(|repo| repo.root.clone()).collect::<std::collections::BTreeSet<_>>(),
            state.cancellation.clone(),
            state.build_failures.clone(),
        )
    };
    // Roots that have never failed build first; a repository inside its
    // backoff window is skipped rather than re-burning the sequential budget.
    let mut ordered_roots = roots;
    ordered_roots.sort_by_key(|root| failures.contains_key(&root.to_string_lossy().into_owned()));
    let mut build_errors = Vec::new();
    for root in ordered_roots {
        if cancellation.is_cancelled() { break; }
        let key = root.to_string_lossy().into_owned();
        if existing.contains(&key) { continue; }
        if !retry_due(&failures, &key, Instant::now()) { continue; }
        set_supervisor_stage(supervisor_stage, &format!("build:{key}"));
        match build_resident_repo(&root, &cancellation) {
            Ok(repo) => {
                // Publish each completed root immediately. A large enrolled
                // repository must not hide smaller completed roots from
                // health or watcher identity until the whole batch finishes.
                if cancellation.is_cancelled() {
                    let _ = repo.service.drain();
                } else if let Ok(mut state) = state.lock() {
                    state.build_failures.remove(&key);
                    if desired.contains(&repo.root)
                        && !state.repos.iter().any(|current| current.root == repo.root)
                    {
                        state.repos.push(repo);
                    }
                }
            }
            Err(error) => {
                if let Ok(mut state) = state.lock() {
                    let entry = state.build_failures.entry(key.clone()).or_insert((0, Instant::now()));
                    entry.0 = entry.0.saturating_add(1);
                    entry.1 = Instant::now();
                }
                build_errors.push(error);
            }
        }
    }
    if let Ok(mut state) = state.lock() {
        state.registry_error = build_errors.into_iter().next();
        if let Some(error) = &state.registry_error {
            eprintln!("{}", serde_json::json!({"event":"resident_blueprint_initialization", "stage":"failed", "error": error}));
        } else if !state.repos.is_empty() {
            eprintln!("{}", serde_json::json!({"event":"resident_blueprint_initialization", "stage":"completed", "repoCount": state.repos.len()}));
        }
    }
}

/// Resident Ledger maintenance cadence: the persisted document projection is
/// reconciled in the background under the same `background_authority_open`
/// gate as watcher work — never inside a retrieval request. Five minutes is
/// the maintenance interval; a faster discovery path stays available through
/// the explicit `ledger sync` operation.
const LEDGER_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(300);

/// Authorized Ledger maintenance for every enrolled root: reconciles the
/// persisted document projection through `LedgerService::maintain` — the
/// same `sync_locked` index/update mechanism the explicit "sync" operation
/// uses. Runs only when the supervisor holds background authority, bounded
/// per root, and reports typed failure rather than stopping the loop.
fn reconcile_resident_ledger(state: &Arc<Mutex<ResidentBlueprintState>>) {
    let Ok(registry) = crate::authorization::load_installation_registry() else { return };
    let Ok(owner) = crate::ledger::service::active_owner() else { return };
    for binding in registry.bindings() {
        let cancelled = state.lock().map(|state| state.cancellation.is_cancelled()).unwrap_or(true);
        if cancelled { break; }
        let root = Path::new(&binding.root);
        let canonical_id = membrane_federation::root::canonical_repository_id(root);
        let caller = match crate::ledger::service::Caller::enrolled(root, &canonical_id) {
            Ok(caller) => caller,
            Err(error) => {
                eprintln!("{}", serde_json::json!({"event":"resident_ledger_maintenance", "stage":"skipped", "root":binding.root, "error":error}));
                continue;
            }
        };
        let started = Instant::now();
        let budget = crate::ledger::limits::WorkBudget::bounded(Duration::from_secs(120));
        match owner.maintain(&caller, &budget) {
            Ok(report) => eprintln!("{}", serde_json::json!({"event":"resident_ledger_maintenance", "stage":"completed", "root":binding.root, "elapsedMs":started.elapsed().as_millis(), "generation":report.index_generation})),
            Err(error) => eprintln!("{}", serde_json::json!({"event":"resident_ledger_maintenance", "stage":"failed", "root":binding.root, "elapsedMs":started.elapsed().as_millis(), "error":error})),
        }
    }
}

fn mark_registry_error(state: &Arc<Mutex<ResidentBlueprintState>>, error: String) {
    if let Ok(mut state) = state.lock() {
        state.repos.clear();
        state.enrolled_repo_count = 0;
        state.registry_error = Some(error);
    }
}

fn drain_resident_repositories(state: &Arc<Mutex<ResidentBlueprintState>>) {
    if let Ok(mut state) = state.lock() {
        for repo in state.repos.drain(..) { let _ = repo.service.drain(); }
    }
}

fn stop_resident_blueprint() -> Result<(), String> {
    let Some(slot) = RESIDENT_BLUEPRINT.get() else {
        return Ok(());
    };
    let mut current = slot
        .lock()
        .map_err(|_| "resident Blueprint state unavailable".to_string())?;
    if let Some(resident) = current.as_mut() {
        // Keep singleton ownership in the slot until supervisor termination is
        // observed. A timeout is surfaced to the caller; no writable worker is
        // silently detached or allowed to outlive its owner.
        resident.stop_bounded(RESIDENT_SUPERVISOR_STOP_TIMEOUT)?;
    }
    current.take();
    Ok(())
}

impl Drop for ResidentBlueprint {
    fn drop(&mut self) {
        // Normal teardown goes through stop_bounded while this value remains
        // in RESIDENT_BLUEPRINT. Keep Drop defensive and bounded for failed
        // startup/replacement paths; it must never block process teardown
        // forever.
        let _ = self.stop_bounded(RESIDENT_SUPERVISOR_STOP_TIMEOUT);
    }
}

impl ResidentBlueprint {
    fn stop_bounded(&mut self, timeout: Duration) -> Result<(), String> {
        self.state.lock().ok().map(|state| state.cancellation.cancel());
        self.lifecycle.request_drain(Some("resident_runtime_exit"));
        if let Some(supervisor) = self.supervisor.as_ref() {
            if !wait_for_thread_exit(supervisor, timeout) {
                let stage = match self.state.lock() {
                    Ok(state) => supervisor_stage(&state.supervisor_stage),
                    Err(_) => "state_unavailable".to_owned(),
                };
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "event": "resident_blueprint_supervisor_stop_timeout",
                        "stage": stage,
                        "timeoutMs": timeout.as_millis() as u64,
                    })
                );
                return Err(format!(
                    "resident Blueprint supervisor drain timeout; singleton retained (supervisor stage at timeout: {stage})"
                ));
            }
        }
        if let Some(supervisor) = self.supervisor.take() {
            supervisor.join().map_err(|_| "resident Blueprint supervisor panicked while draining".to_string())?;
        }
        drain_resident_repositories(&self.state);
        Ok(())
    }
}

fn wait_for_thread_exit(thread: &std::thread::JoinHandle<()>, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while !thread.is_finished() {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

/// The resident Blueprint service enrolled for `repo_root`, if this engine
/// holds one. Freshness reads use this to consult the resident watcher's
/// own observation state (`barrier_all`) plus the sealed generation basis
/// instead of spawning a per-request live worktree fingerprint. `None`
/// means no resident coverage — caller must fall back or degrade honestly.
pub(crate) fn resident_blueprint_service(repo_root: &Path) -> Option<Arc<NativeService>> {
    let slot = RESIDENT_BLUEPRINT.get()?;
    let current = slot.lock().ok()?;
    let resident = current.as_ref()?;
    let state = resident.state.lock().ok()?;
    let wanted = std::fs::canonicalize(repo_root)
        .map(|path| path.to_string_lossy().to_lowercase())
        .unwrap_or_else(|_| repo_root.to_string_lossy().to_lowercase());
    state.repos.iter().find(|repo| {
        if repo.root.eq_ignore_ascii_case(repo_root.to_string_lossy().as_ref()) {
            return true;
        }
        std::fs::canonicalize(&repo.root)
            .map(|path| path.to_string_lossy().to_lowercase() == wanted)
            .unwrap_or(false)
    }).map(|repo| Arc::clone(&repo.service))
}

/// Actual resident watcher state for health and Hub composition.
pub fn resident_blueprint_status() -> serde_json::Value {
    let Some(slot) = RESIDENT_BLUEPRINT.get() else {
        return serde_json::json!({
            "watcherRunning": false,
            "enrolledRepoCount": 0,
            "watcherIdentity": null,
        });
    };
    let Ok(current) = slot.lock() else {
        return serde_json::json!({"watcherRunning": false, "enrolledRepoCount": 0});
    };
    let Some(resident) = current.as_ref() else {
        return serde_json::json!({
            "watcherRunning": false,
            "enrolledRepoCount": 0,
            "watcherIdentity": null,
        });
    };
    let Ok(state) = resident.state.lock() else {
        return serde_json::json!({"watcherRunning": false, "enrolledRepoCount": 0});
    };
    resident_status_json(&state)
}

fn resident_status_json(state: &ResidentBlueprintState) -> serde_json::Value {
    let watcher_running = state.enrolled_repo_count > 0
        && state.repos.len() == state.enrolled_repo_count as usize
        && state.repos.iter().all(|repo| repo.service.status() == ServiceStatus::Running);
    let watcher_ready = watcher_running && state.repos.iter().all(|repo| repo.service.is_ready());
    let coverage = if state.enrolled_repo_count > 0
        && state.repos.len() == state.enrolled_repo_count as usize
        && state.repos.iter().all(|repo| repo.generation_complete) { "complete" } else { "partial" };
    let identities = state.repos.iter().map(|repo| {
        let events = repo.service.events();
        serde_json::json!({
            "root": repo.root,
            "generationId": repo.generation_id,
            "complete": repo.generation_complete,
            "eventSequence": events.last().map(|event| event.sequence).unwrap_or(0),
            "lastEvent": events.last().map(|event| event.kind.clone()),
        })
    }).collect::<Vec<_>>();
    serde_json::json!({
        "watcherRunning": watcher_running,
        "watcherReady": watcher_ready,
        "watcherState": if watcher_ready { "running" } else if state.enrolled_repo_count == 0 { "not_configured" } else { "watcher_unavailable" },
        "watcherCoverage": coverage,
        "watcherDetail": state.registry_error.clone().or_else(|| (coverage == "partial").then_some("generation_partial_coverage".into())),
        "enrolledRepoCount": state.enrolled_repo_count,
        "watcherIdentity": identities,
    })
}

pub fn install_lifecycle_control(control: LifecycleControl) -> Result<(), String> {
    let slot = LIFECYCLE_CONTROL.get_or_init(|| RwLock::new(LifecycleControl::default()));
    *slot
        .write()
        .map_err(|_| "lifecycle control unavailable".to_string())? = control;
    Ok(())
}

pub fn lifecycle_control() -> LifecycleControl {
    LIFECYCLE_CONTROL
        .get_or_init(|| RwLock::new(LifecycleControl::default()))
        .read()
        .map(|control| control.clone())
        .unwrap_or_default()
}

struct HubRuntimeClaim;

impl HubRuntimeClaim {
    fn acquire() -> Result<Self, String> {
        HUB_RUNTIME_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "membrane_hub_runtime_already_active".to_string())
    }
}

impl Drop for HubRuntimeClaim {
    fn drop(&mut self) {
        HUB_RUNTIME_ACTIVE.store(false, Ordering::Release);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeConfig {
    schema_version: u32,
    service_id: String,
    host: String,
    port: u16,
}

pub(crate) struct Runtime {
    pub(crate) workspace_root: PathBuf,
    pub(crate) db: PathBuf,
    pub(crate) token: PathBuf,
    pub(crate) ort: PathBuf,
    pub(crate) hf_home: PathBuf,
    pub(crate) port: u16,
    pub(crate) origin: &'static str,
    pub(crate) stable_current: Option<PathBuf>,
    pub(crate) version_root: Option<PathBuf>,
}

/// OS-backed owner for the installed resident engine.  The lock is acquired
/// before any store, embedder, watcher, or listener initialization.  A
/// contender fails closed instead of selecting another port/store.
pub(crate) struct EngineOwner {
    file: File,
    path: PathBuf,
}

impl EngineOwner {
    pub(crate) fn acquire(runtime: &Runtime) -> Result<Self, String> {
        let state_root = std::fs::canonicalize(&runtime.workspace_root)
            .map_err(|error| format!("canonicalize resident state root: {error}"))?;
        let path = state_root.join("tools/.cache/memory/membrane-engine.lock");
        let parent = path
            .parent()
            .ok_or_else(|| "resident owner lock has no parent".to_string())?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create resident owner lock directory: {error}"))?;
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| format!("open resident owner lock {}: {error}", path.display()))?;
        lock_exclusive(&file).map_err(|error| {
            emit_lifecycle("owner_contended", Some("owner_lock_held"));
            format!("membrane_engine_already_owned: {error}")
        })?;
        file.set_len(0)
            .map_err(|error| format!("truncate resident owner lock: {error}"))?;
        let metadata = serde_json::json!({
            "schemaVersion": 1,
            "pid": std::process::id(),
            "stateRoot": state_root,
            "db": runtime.db,
            "observedAtUnixMs": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis() as u64)
                .unwrap_or_default(),
        });
        writeln!(file, "{metadata}")
            .map_err(|error| format!("write resident owner metadata: {error}"))?;
        file.flush()
            .map_err(|error| format!("flush resident owner metadata: {error}"))?;
        emit_lifecycle("owner_acquired", Some("resident_engine"));
        Ok(Self { file, path })
    }
}

impl Drop for EngineOwner {
    fn drop(&mut self) {
        unlock_exclusive(&self.file);
        emit_lifecycle("owner_released", Some("resident_engine"));
        let _ = &self.path;
    }
}

#[cfg(unix)]
fn lock_exclusive(file: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    // flock is process-owned and released by the kernel on crash/termination.
    let result = unsafe { flock(file.as_raw_fd(), 2 | 4) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn unlock_exclusive(file: &File) {
    use std::os::fd::AsRawFd;
    let _ = unsafe { flock(file.as_raw_fd(), 8) };
}

#[cfg(unix)]
unsafe extern "C" {
    fn flock(fd: std::os::raw::c_int, operation: std::os::raw::c_int) -> std::os::raw::c_int;
}

#[cfg(windows)]
fn lock_exclusive(file: &File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    let mut overlapped = std::mem::MaybeUninit::<OVERLAPPED>::zeroed();
    let ok = unsafe {
        LockFileEx(
            file.as_raw_handle() as *mut _,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            u32::MAX,
            u32::MAX,
            overlapped.as_mut_ptr(),
        )
    };
    if ok != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn unlock_exclusive(file: &File) {
    use std::os::windows::io::AsRawHandle;
    let mut overlapped = std::mem::MaybeUninit::<OVERLAPPED>::zeroed();
    unsafe {
        let _ = UnlockFileEx(
            file.as_raw_handle() as *mut _,
            0,
            u32::MAX,
            u32::MAX,
            overlapped.as_mut_ptr(),
        );
    }
}

#[cfg(windows)]
const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x00000001;
#[cfg(windows)]
const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x00000002;
#[cfg(windows)]
#[repr(C)]
struct OVERLAPPED {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    h_event: *mut std::ffi::c_void,
}

#[cfg(windows)]
unsafe extern "system" {
    fn LockFileEx(
        file: *mut std::ffi::c_void,
        flags: u32,
        reserved: u32,
        low: u32,
        high: u32,
        overlapped: *mut OVERLAPPED,
    ) -> i32;
    fn UnlockFileEx(
        file: *mut std::ffi::c_void,
        reserved: u32,
        low: u32,
        high: u32,
        overlapped: *mut OVERLAPPED,
    ) -> i32;
}

fn build_info() -> serde_json::Value {
    serde_json::json!({
        "product_version": env!("CARGO_PKG_VERSION"),
        "membrane_source_commit": crate::release_identity::source_commit().unwrap_or("unknown"),
        "source_tree_sha256": crate::release_identity::source_tree_sha256().unwrap_or("unknown"),
        "release_generation": crate::release_identity::release_generation(),
        "target": crate::release_identity::target_triple(),
    })
}

pub(crate) fn prepare_runtime_identity(
    runtime: &Runtime,
) -> Result<
    (
        crate::installation_identity::InstallationIdentity,
        crate::installation_identity::StartupClaim,
    ),
    String,
> {
    crate::installation_identity::prepare_service_start(&runtime.workspace_root)
        .map_err(|error| format!("prepare installation identity: {error}"))
}

fn runtime_from_exe_at_workspace(
    exe: &Path,
    workspace_root: Option<&Path>,
    allow_hub_bundle: bool,
) -> Result<Runtime, String> {
    if workspace_root.is_none() {
        if let Ok(runtime) = runtime_from_installed_exe(exe) {
            return Ok(runtime);
        }
    }
    let direct_bin = exe
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "bin"))
        .filter(|path| {
            path.parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "tools")
        });
    let linked_bin = workspace_root.and_then(|root| {
        if !root.is_absolute() {
            return None;
        }
        let bin = root.join("tools/bin");
        let service_name = if cfg!(windows) {
            "membrane.exe"
        } else {
            "membrane"
        };
        let actual = std::fs::canonicalize(exe).ok()?;
        let service = bin.join(service_name);
        let metadata = std::fs::symlink_metadata(&service).ok()?;
        if !metadata.file_type().is_symlink() {
            return None;
        }
        let linked = std::fs::canonicalize(service).ok()?;
        (linked == actual).then_some(bin)
    });
    let bundled_bin = workspace_root.and_then(|root| {
        if !allow_hub_bundle || !root.is_absolute() || !is_hub_bundled_membrane(exe) {
            return None;
        }
        Some(root.join("tools/bin"))
    });
    let bin = direct_bin
        .map(Path::to_path_buf)
        .or(linked_bin)
        .or(bundled_bin)
        .ok_or_else(|| {
            "membrane resident must be Hub-owned or run from its exact canonical tools/bin path"
                .to_string()
        })?;
    let tools = bin
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "tools"))
        .ok_or_else(|| "membrane resident could not locate the tools directory".to_string())?;
    let config_path = tools.join("lib/memory/runtime.json");
    let config: RuntimeConfig = serde_json::from_slice(
        &std::fs::read(&config_path)
            .map_err(|error| format!("read {}: {error}", config_path.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", config_path.display()))?;
    if config.schema_version != 1
        || config.service_id != "membrane-local-v1"
        || config.host != "127.0.0.1"
        || config.port < 1024
    {
        return Err(format!(
            "invalid runtime identity in {}",
            config_path.display()
        ));
    }
    let ort_name = if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    };
    Ok(Runtime {
        workspace_root: tools
            .parent()
            .ok_or_else(|| "membrane runtime could not locate workspace root".to_string())?
            .to_path_buf(),
        db: tools.join(".cache/memory/cortex-engine.db"),
        token: tools.join(".cache/memory/api-token"),
        ort: bin.join(ort_name),
        hf_home: tools.join(".cache/fastembed"),
        port: config.port,
        origin: "development",
        stable_current: None,
        version_root: None,
    })
}

static RESIDENT_STORE: OnceLock<crate::MemoryStore> = OnceLock::new();

pub(crate) fn install_resident_store(store: crate::MemoryStore) -> Result<(), String> {
    RESIDENT_STORE.set(store).map_err(|_| "resident Cortex store already owned".to_owned())
}

pub(crate) fn resident_store() -> Option<crate::MemoryStore> { RESIDENT_STORE.get().cloned() }

pub(crate) fn resident_store_for_db(path: &Path) -> Option<crate::MemoryStore> {
    let store = resident_store()?;
    let exe = std::env::current_exe().ok()?;
    let runtime = runtime_from_installed_exe(&exe).ok()?;
    let requested = std::fs::canonicalize(path).ok()?;
    let installed = std::fs::canonicalize(runtime.db).ok()?;
    if requested == installed { Some(store) } else { None }
}

pub(crate) fn open_installed_store() -> Result<crate::MemoryStore, String> {
    if let Some(store) = resident_store() { return Ok(store); }
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let runtime = runtime_from_installed_exe(&exe)?;
    if let Some(parent) = runtime.db.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    crate::MemoryStore::try_open(crate::MemDb::open(&runtime.db).map_err(|error| error.to_string())?)
}

pub(crate) fn emit_lifecycle(event: &str, reason: Option<&str>) {
    let mut value = serde_json::json!({
        "event": event,
        "pid": std::process::id(),
        "observedAtUnixMs": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or_default(),
    });
    if let Some(reason) = reason {
        value["reason"] = serde_json::Value::String(reason.to_owned());
    }
    eprintln!("{value}");
}

/// Foreground hook owner. Opens persisted Cortex rows without loading the
/// optional embedding runtime; callers use its lexical read-only arm.
pub(crate) fn open_installed_lexical_store() -> Result<crate::MemoryStore, String> {
    if let Some(store) = resident_store() { return Ok(store); }
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let runtime = runtime_from_installed_exe(&exe)?;
    // Hook startup is a read-only bounded path.  In particular, do not create
    // an absent store here: missing sealed state must become a typed omission.
    crate::MemoryStore::try_open_lexical(
        crate::MemDb::open_read_only(&runtime.db).map_err(|error| error.to_string())?,
    )
}

pub(crate) fn runtime_from_installed_exe(exe: &Path) -> Result<Runtime, String> {
    let executable_root = exe.parent().ok_or_else(|| "executable has no parent".to_string())?;
    let current = if executable_root.file_name().is_some_and(|name| name == "current") {
        executable_root.to_path_buf()
    } else if executable_root.parent().and_then(Path::file_name).is_some_and(|name| name == "versions") {
        executable_root.parent().and_then(Path::parent)
            .ok_or_else(|| "installed version has no product root".to_string())?.join("current")
    } else {
        return Err("executable is not under installed current".into());
    };
    let product_root = current
        .parent()
        .ok_or_else(|| "installed current has no product root".to_string())?;
    let versions = product_root.join("versions");
    let pointer = std::fs::read_link(&current)
        .map_err(|error| format!("read installed current pointer: {error}"))?;
    let pointer = if pointer.is_absolute() {
        pointer
    } else {
        product_root.join(pointer)
    };
    let version_root = std::fs::canonicalize(pointer)
        .map_err(|error| format!("resolve installed version: {error}"))?;
    let versions = std::fs::canonicalize(versions)
        .map_err(|error| format!("resolve installed versions: {error}"))?;
    if version_root.parent() != Some(versions.as_path()) || !version_root.is_dir() {
        return Err("installed current does not target one direct version".into());
    }
    if std::fs::canonicalize(executable_root).map_err(|error| error.to_string())? != version_root {
        return Err("executable is not active installed version".into());
    }
    let state = product_root.join("state");
    runtime_from_installed_state(&state, current.to_path_buf(), version_root)
}

/// Public, read-only projection used by explicit native enrollment. It derives
/// paths from the signed executable's active `current` installation; it never
/// falls back to workspace or process CWD state.
pub fn installed_binding_projection() -> Result<serde_json::Value, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let runtime = runtime_from_installed_exe(&exe)?;
    let identity_path = crate::installation_identity::InstallationPaths::defaults_for_workspace(&runtime.workspace_root).identity;
    let identity: serde_json::Value = serde_json::from_slice(&std::fs::read(&identity_path)
        .map_err(|e| format!("read installed identity {}: {e}", identity_path.display()))?)
        .map_err(|e| format!("parse installed identity: {e}"))?;
    Ok(serde_json::json!({
        "workspaceRoot": runtime.workspace_root,
        "stableCurrent": runtime.stable_current,
        "host": "127.0.0.1",
        "port": runtime.port,
        "endpoint": format!("http://127.0.0.1:{}", runtime.port),
        "db": runtime.db,
        "tokenPath": runtime.token,
        "installationId": identity.get("installation_id").cloned().unwrap_or(serde_json::Value::Null),
        "serviceInstanceId": identity.get("current_service_instance_id").cloned().unwrap_or(serde_json::Value::Null),
    }))
}

fn runtime_from_installed_state(
    state: &Path,
    stable_current: PathBuf,
    version_root: PathBuf,
) -> Result<Runtime, String> {
    let tools = state.join("tools");
    let bin = stable_current;
    let ort_name = if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    };
    Ok(Runtime {
        workspace_root: state.to_path_buf(),
        db: tools.join(".cache/memory/cortex-engine.db"),
        token: tools.join(".cache/memory/api-token"),
        ort: if cfg!(windows) {
            version_root.join("runtime/resources/semantic-embed-runtime").join(ort_name)
        } else {
            version_root.join(ort_name)
        },
        hf_home: tools.join(".cache/fastembed"),
        port: 47_851,
        origin: "installed",
        stable_current: Some(bin),
        version_root: Some(version_root),
    })
}

fn is_hub_bundled_membrane(exe: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let Some(macos) = exe.parent() else {
            return false;
        };
        let Some(contents) = macos.parent() else {
            return false;
        };
        let Some(bundle) = contents.parent() else {
            return false;
        };
        return exe.file_name().is_some_and(|name| name == "membrane")
            && macos.file_name().is_some_and(|name| name == "MacOS")
            && contents.file_name().is_some_and(|name| name == "Contents")
            && bundle
                .file_name()
                .is_some_and(|name| name == "Membrane Hub.app")
            && macos.join("membrane-hub").is_file();
    }
    #[cfg(target_os = "windows")]
    {
        return exe.file_name().is_some_and(|name| name == "membrane.exe")
            && exe
                .parent()
                .is_some_and(|directory| directory.join("membrane-hub.exe").is_file());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

pub(crate) fn runtime_from_exe(exe: &Path) -> Result<Runtime, String> {
    let development = std::env::var_os("MEMBRANE_RUNTIME_ORIGIN")
        .is_some_and(|value| value == "development");
    let workspace = development.then(|| std::env::var_os("WORKSPACE_ROOT")).flatten().map(PathBuf::from);
    runtime_from_exe_at_workspace(exe, workspace.as_deref(), lifecycle_control().hub_bound())
}

fn runtime_from_workspace_root(workspace_root: &Path) -> Result<Runtime, String> {
    let root = std::fs::canonicalize(workspace_root)
        .map_err(|error| format!("canonicalize Hub workspace root: {error}"))?;
    if root.file_name().is_some_and(|name| name == "state")
        && root
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "Membrane")
    {
        let product_root = root
            .parent()
            .ok_or_else(|| "installed state has no product root".to_string())?;
        let current = product_root.join("current");
        let versions = std::fs::canonicalize(product_root.join("versions"))
            .map_err(|error| format!("resolve installed versions: {error}"))?;
        let pointer = std::fs::read_link(&current)
            .map_err(|error| format!("read installed current pointer: {error}"))?;
        let pointer = if pointer.is_absolute() {
            pointer
        } else {
            product_root.join(pointer)
        };
        let version_root = std::fs::canonicalize(pointer)
            .map_err(|error| format!("resolve installed version: {error}"))?;
        if version_root.parent() != Some(versions.as_path()) {
            return Err("installed current does not target one direct version".into());
        }
        return runtime_from_installed_state(&root, current, version_root);
    }
    let tools = root.join("tools");
    let bin = tools.join("bin");
    let config_path = tools.join("lib/memory/runtime.json");
    let config: RuntimeConfig = serde_json::from_slice(
        &std::fs::read(&config_path)
            .map_err(|error| format!("read {}: {error}", config_path.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", config_path.display()))?;
    if config.schema_version != 1
        || config.service_id != "membrane-local-v1"
        || config.host != "127.0.0.1"
        || config.port < 1024
    {
        return Err(format!(
            "invalid runtime identity in {}",
            config_path.display()
        ));
    }
    let ort_name = if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    };
    Ok(Runtime {
        workspace_root: root.clone(),
        db: tools.join(".cache/memory/cortex-engine.db"),
        token: tools.join(".cache/memory/api-token"),
        ort: bin.join(ort_name),
        hf_home: tools.join(".cache/fastembed"),
        port: config.port,
        origin: "development",
        stable_current: None,
        version_root: None,
    })
}

/// Run the sole resident Membrane engine for a workspace. Hub may request
/// activation, but engine ownership is independent from Hub UI lifetime.
/// The process-wide claim and OS owner lock reject duplicates before storage
/// or port binding.
pub fn run_hub_runtime(workspace_root: &Path, lifecycle: LifecycleControl) -> Result<(), String> {
    emit_lifecycle("activation_requested", Some("resident_engine"));
    let _claim = HubRuntimeClaim::acquire()?;
    install_lifecycle_control(lifecycle)?;
    let runtime = runtime_from_workspace_root(workspace_root)?;
    run_runtime(runtime)
}

// Installed ownership lasts until OS process exit, including any blocking
// tasks left behind by Tokio shutdown_timeout. Never unlock beneath a writer.
static INSTALLED_ENGINE_OWNER: OnceLock<EngineOwner> = OnceLock::new();

struct ShutdownDeadline {
    done: std::sync::mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ShutdownDeadline {
    fn start(lifecycle: LifecycleControl) -> Result<Self, String> {
        let (done, receiver) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new().name("membrane-shutdown-deadline".into())
            .spawn(move || watch_shutdown_deadline(lifecycle, receiver, Duration::from_secs(15), || {
                emit_lifecycle("engine_shutdown_timeout", Some("complete_teardown_deadline_exceeded"));
                // Only installed standalone engine uses this watchdog. OS exit
                // ends all workers before releasing singleton file ownership.
                std::process::exit(1);
            })).map_err(|error| format!("start shutdown deadline: {error}"))?;
        Ok(Self { done, thread: Some(thread) })
    }
}

impl Drop for ShutdownDeadline {
    fn drop(&mut self) {
        let _ = self.done.send(());
        if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    }
}

fn watch_shutdown_deadline(
    lifecycle: LifecycleControl,
    done: std::sync::mpsc::Receiver<()>,
    grace: Duration,
    expired: impl FnOnce(),
) {
    while !lifecycle.shutdown_requested() {
        match done.recv_timeout(Duration::from_millis(10)) {
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {},
            _ => return,
        }
    }
    emit_lifecycle("engine_shutdown_deadline_started", Some("complete_teardown"));
    if matches!(done.recv_timeout(grace), Err(std::sync::mpsc::RecvTimeoutError::Timeout)) {
        expired();
    }
}

fn run_runtime(runtime: Runtime) -> Result<(), String> {
    let startup_started = Instant::now();
    let emit_startup_stage = |stage: &str, stage_started: Instant| {
        eprintln!(
            "{}",
            serde_json::json!({
                "event": "membrane_startup_stage",
                "stage": stage,
                "durationMs": stage_started.elapsed().as_millis() as u64,
                "elapsedMs": startup_started.elapsed().as_millis() as u64,
            })
        );
    };
    // This is intentionally the first stateful operation.  Store opening,
    // identity minting, embedder initialization, watcher startup, and port
    // binding all happen only after exclusive ownership is established.
    let stage_started = Instant::now();
    let owner = EngineOwner::acquire(&runtime)?;
    emit_startup_stage("ownership", stage_started);
    let installed = runtime.origin == "installed";
    let _development_owner = if installed {
        INSTALLED_ENGINE_OWNER.set(owner).map_err(|_| "installed engine already initialized".to_owned())?;
        None
    } else { Some(owner) };
    let _shutdown_deadline = if installed { Some(ShutdownDeadline::start(lifecycle_control().clone())?) } else { None };
    let stage_started = Instant::now();
    std::env::set_var("CORTEX_DB", &runtime.db);
    std::env::set_var("MEMBRANE_PORT", runtime.port.to_string());
    std::env::set_var("MEMBRANE_API_TOKEN_FILE", &runtime.token);
    std::env::set_var("ORT_DYLIB_PATH", &runtime.ort);
    std::env::set_var("HF_HOME", &runtime.hf_home);
    std::env::set_var("HF_HUB_OFFLINE", "1");
    std::env::set_var("WORKSPACE_ROOT", &runtime.workspace_root);
    std::env::set_var("MEMBRANE_RUNTIME_ORIGIN", runtime.origin);
    let catalog_path = crate::catalog::default_catalog_path().map_err(|error| error.to_string())?;
    std::env::set_var("MEMBRANE_CATALOG", catalog_path);
    emit_startup_stage("environment", stage_started);
    // State the embedder mode once at startup. Whether recall is semantic or
    // lexical decides how much its results are worth, and the daemon log is
    // the one place that answer is available before any command is run.
    if cfg!(feature = "fastembed") {
        let ort = runtime.ort.display().to_string();
        if runtime.ort.is_file() {
            eprintln!("[startup] embedder: fastembed compiled in, ORT_DYLIB_PATH={ort}");
        } else {
            eprintln!(
                "[startup] embedder: fastembed compiled in but {ort} is missing; memory writes will be refused rather than store hash vectors"
            );
        }
    } else {
        eprintln!(
            "[startup] embedder: hash-256 (this build has no fastembed feature); recall matches lexically only"
        );
    }
    if runtime.origin == "installed" {
        let stage_started = Instant::now();
        crate::serve::migrate_installed_credential(&runtime.token)?;
        emit_startup_stage("credential_migration", stage_started);
    }
    let stage_started = Instant::now();
    let (identity, claim) = prepare_runtime_identity(&runtime)?;
    emit_startup_stage("identity", stage_started);
    let workspace_root = &runtime.workspace_root;
    // Publish the IPC handshake manifest before any peer can connect. This
    // is a hard requirement of the MBR-105 contract: a resident that has
    // not published its manifest must reject every handshake. We deliberately
    // do this AFTER `prepare_runtime_identity` so the manifest always
    // reflects the just-minted startup generation. A failure to publish is
    // fatal: the resident would otherwise serve requests that no peer can
    // verify, which silently breaks the contract.
    let active_manifest =
        crate::installation_manifest::build_active_manifest(&identity, &claim, workspace_root);
    let stage_started = Instant::now();
    crate::installation_manifest::publish_active_manifest(active_manifest)
        .map_err(|error| format!("publish installation manifest: {error}"))?;
    emit_startup_stage("active_manifest", stage_started);
    std::env::set_var("MEMBRANE_INSTALLATION_ID", &identity.installation_id);
    std::env::set_var("MEMBRANE_SERVICE_INSTANCE_ID", &claim.service_instance_id);
    // Start only after all pre-serve initialization can still fail. Startup
    // errors therefore leave no detached watcher behind, while health is
    // never published without a running, enrolled native service.
    let stage_started = Instant::now();
    start_resident_blueprint()?;
    emit_startup_stage("blueprint_supervisor_dispatch", stage_started);
    let result = crate::serve::run(
        runtime
            .db
            .to_str()
            .ok_or_else(|| "database path is not valid UTF-8".to_string())?,
        runtime.port,
        &identity,
        &claim,
        startup_started,
    );
    lifecycle_control().request_drain(Some("resident_transport_stopped"));
    stop_resident_blueprint()?;
    emit_lifecycle("engine_stopped", Some("resident_engine"));
    result
}

/// Start the installed resident engine without requiring a tray or Hub
/// parent. The OS supervisor may invoke this entrypoint directly; background
/// work remains holder-authorized and explicit requests remain available.
pub fn run_installed_runtime(lifecycle: LifecycleControl) -> Result<(), String> {
    emit_lifecycle("activation_requested", Some("standalone_daemon"));
    let _claim = HubRuntimeClaim::acquire()?;
    install_lifecycle_control(lifecycle)?;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let runtime = runtime_from_installed_exe(&exe)?;
    run_runtime(runtime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_deadline_terminates_stuck_process_before_owner_can_be_reacquired() {
        const CHILD_ROOT: &str = "MEMBRANE_SHUTDOWN_TEST_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(root);
            let runtime = Runtime { workspace_root: root.clone(), db: root.join("db"), token: root.join("token"),
                ort: root.join("ort"), hf_home: root.join("hf"), port: 0, origin: "installed", stable_current: None, version_root: None };
            let _owner = EngineOwner::acquire(&runtime).unwrap();
            let store = crate::MemoryStore::new();
            store.db().lock().execute_batch("CREATE TABLE shared_owner_probe(value INTEGER); INSERT INTO shared_owner_probe VALUES(7)").unwrap();
            install_resident_store(store).unwrap();
            for shared in [open_installed_store().unwrap(), open_installed_lexical_store().unwrap()] {
                let value: i64 = shared.db().lock().query_row("SELECT value FROM shared_owner_probe", [], |row| row.get(0)).unwrap();
                assert_eq!(value, 7);
            }
            let control = LifecycleControl::default();
            let (_done, receiver) = std::sync::mpsc::channel();
            control.request_drain(Some("test_final_holder"));
            watch_shutdown_deadline(control, receiver, Duration::from_millis(25), || std::process::exit(73));
            panic!("stuck teardown unexpectedly returned");
        }
        let root = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "service::tests::shutdown_deadline_terminates_stuck_process_before_owner_can_be_reacquired", "--nocapture"])
            .env(CHILD_ROOT, root.path()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
        let mut child = command.spawn().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() { break status; }
            if std::time::Instant::now() >= deadline { let _ = child.kill(); let _ = child.wait(); panic!("shutdown deadline did not terminate child"); }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(status.code(), Some(73));
        let runtime = Runtime { workspace_root: root.path().into(), db: root.path().join("db"), token: root.path().join("token"),
            ort: root.path().join("ort"), hf_home: root.path().join("hf"), port: 0, origin: "installed", stable_current: None, version_root: None };
        assert!(EngineOwner::acquire(&runtime).is_ok());
    }

    #[test]
    fn complete_shutdown_deadline_covers_stuck_teardown_and_disarms_cleanly() {
        for complete in [false, true] {
            let control = LifecycleControl::default();
            let (done, receiver) = std::sync::mpsc::channel();
            let timed_out = Arc::new(AtomicBool::new(false));
            let observed = timed_out.clone();
            let worker_control = control.clone();
            let thread = std::thread::spawn(move || watch_shutdown_deadline(worker_control, receiver,
                Duration::from_millis(20), || { observed.store(true, Ordering::Release); }));
            control.request_drain(Some("test_final_owner"));
            if complete { done.send(()).unwrap(); }
            thread.join().unwrap();
            assert_eq!(timed_out.load(Ordering::Acquire), !complete);
        }
    }

    #[test]
    fn resident_supervisor_wait_is_bounded_when_worker_is_stuck() {
        let thread = std::thread::spawn(|| std::thread::sleep(Duration::from_millis(100)));
        let started = std::time::Instant::now();
        assert!(!wait_for_thread_exit(&thread, Duration::from_millis(10)));
        assert!(started.elapsed() < Duration::from_millis(80));
        // Join after bounded probe so test does not leave a worker behind.
        thread.join().unwrap();
    }

    #[test]
    fn resident_supervisor_wait_accepts_clean_stop() {
        let thread = std::thread::spawn(|| {});
        assert!(wait_for_thread_exit(&thread, Duration::from_secs(1)));
        thread.join().unwrap();
    }

    #[test]
    fn lifecycle_capability_is_bounded_memory_only_authority() {
        let capability = "a".repeat(64);
        let control = LifecycleControl::from_lifecycle_capability(&capability).unwrap();
        assert!(control.snapshot_authorized(Some(&capability)));
        assert!(!control.snapshot_authorized(Some("wrong")));
        assert!(!control.snapshot_authorized(None));
        assert!(LifecycleControl::from_lifecycle_capability("").is_err());
        assert!(LifecycleControl::from_lifecycle_capability(&"x".repeat(257)).is_err());
    }

    #[test]
    fn lifecycle_control_closes_admission_and_preserves_first_command() {
        let control = LifecycleControl::default();
        control.mark_ready(47_851);
        assert_eq!(control.wait_until_ready().unwrap(), 47_851);
        control.request_drain(Some("stop"));
        control.request_drain(Some("drain"));
        assert!(!control.admission_open());
        assert!(control.shutdown_requested());
        assert_eq!(control.command().as_deref(), Some("stop"));
    }

    #[test]
    fn engine_drain_cancels_descendants_but_background_loss_does_not() {
        let control = LifecycleControl::default();
        let request = control.cancellation_token();
        let sibling = control.cancellation_token();
        request.cancel();
        assert!(!sibling.is_cancelled());
        assert!(!control.shutdown_requested());
        control.grant_background("hub");
        control.drain_background("hub_left_harness_remains");
        assert!(!sibling.is_cancelled());
        control.request_drain(Some("final_owner_loss"));
        assert!(sibling.is_cancelled());
        assert!(control.cancellation_token().is_cancelled());
        assert!(!control.admission_open());
    }

    #[test]
    fn final_holder_drain_closes_background_authority_but_not_engine_admission() {
        let control = LifecycleControl::default();
        control.grant_background("test_holder");
        assert!(control.background_authority_open());
        control.drain_background("final_holder_release");
        assert!(!control.background_authority_open());
        assert!(control.admission_open());
        assert!(!control.shutdown_requested());
    }

    #[test]
    fn engine_owner_excludes_second_process_local_owner_before_store_init() {
        let root = tempfile::tempdir().unwrap();
        let runtime = Runtime {
            workspace_root: root.path().to_path_buf(),
            db: root.path().join("tools/.cache/memory/cortex-engine.db"),
            token: root.path().join("tools/.cache/memory/api-token"),
            ort: root.path().join("ort"),
            hf_home: root.path().join("hf"),
            port: 47_851,
            origin: "development",
            stable_current: None,
            version_root: None,
        };
        let first = EngineOwner::acquire(&runtime).unwrap();
        let second = EngineOwner::acquire(&runtime);
        match second {
            Err(error) => assert!(error.contains("membrane_engine_already_owned")),
            Ok(_) => panic!("second owner unexpectedly acquired"),
        }
        drop(first);
        assert!(EngineOwner::acquire(&runtime).is_ok());
    }

    #[test]
    fn enrolled_roots_skips_vanished_bindings_instead_of_failing() {
        // A registry that still names a deleted repository (e.g. a cleaned-up
        // qualification temp repo) must not take the resident down: the present
        // root is enrolled and the missing one is dropped as an omission.
        let present = tempfile::tempdir().unwrap();
        let present_root = present.path().to_string_lossy().into_owned();
        let missing_root = present
            .path()
            .join("this-directory-does-not-exist")
            .to_string_lossy()
            .into_owned();
        let registry = crate::authorization::InstallationRegistryV1::from_roots_for_test([
            missing_root,
            present_root.clone(),
        ]);
        let roots = enrolled_roots(&registry).expect("a vanished enrolled root must not be fatal");
        let present_canonical = std::fs::canonicalize(&present_root).unwrap();
        assert_eq!(roots, vec![present_canonical]);
    }

    #[test]
    fn hub_runtime_claim_allows_exactly_one_active_owner() {
        let first = HubRuntimeClaim::acquire().unwrap();
        assert!(HubRuntimeClaim::acquire().is_err());
        drop(first);
        assert!(HubRuntimeClaim::acquire().is_ok());
    }

    #[test]
    fn build_info_exposes_source_commit_and_tree_identity_fields() {
        let info = build_info();
        assert_eq!(info["product_version"], env!("CARGO_PKG_VERSION"));
        assert!(info.get("membrane_source_commit").is_some());
        assert!(info.get("source_tree_sha256").is_some());
        assert!(info.get("release_generation").is_some());
        assert!(info.get("target").is_some());
    }

    #[test]
    fn deployed_service_resolves_canonical_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("tools/bin");
        let config_dir = temp.path().join("tools/lib/memory");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("runtime.json"),
            r#"{"schemaVersion":1,"serviceId":"membrane-local-v1","host":"127.0.0.1","port":47851}"#,
        )
        .unwrap();
        let runtime = runtime_from_exe(&bin.join("membrane.exe")).unwrap();
        assert_eq!(runtime.port, 47851);
        assert_eq!(
            runtime.db,
            temp.path().join("tools/.cache/memory/cortex-engine.db")
        );
        assert_eq!(
            runtime.token,
            temp.path().join("tools/.cache/memory/api-token")
        );
    }

    #[test]
    fn hub_runtime_resolves_directly_from_its_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("tools/lib/memory");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("runtime.json"),
            r#"{"schemaVersion":1,"serviceId":"membrane-local-v1","host":"127.0.0.1","port":47851}"#,
        )
        .unwrap();
        let runtime = runtime_from_workspace_root(temp.path()).unwrap();
        assert_eq!(runtime.port, 47851);
        assert_eq!(
            runtime.db,
            temp.path()
                .canonicalize()
                .unwrap()
                .join("tools/.cache/memory/cortex-engine.db")
        );
    }

    #[test]
    fn installed_state_binds_fixed_port_and_stable_paths() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("Membrane/state");
        let current = temp.path().join("Membrane/current");
        let version = temp.path().join("Membrane/versions/v1");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&version).unwrap();
        let runtime = runtime_from_installed_state(&state, current.clone(), version.clone()).unwrap();
        assert_eq!(runtime.port, 47_851);
        assert_eq!(runtime.workspace_root, state);
        assert_eq!(runtime.stable_current, Some(current));
        #[cfg(windows)]
        assert_eq!(runtime.ort, version.join("runtime/resources/semantic-embed-runtime/onnxruntime.dll"));
        assert_eq!(runtime.version_root, Some(version));
        assert_eq!(runtime.origin, "installed");
    }

    #[cfg(unix)]
    #[test]
    fn relocated_service_requires_exact_workspace_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let bin = workspace.join("tools/bin");
        let config_dir = workspace.join("tools/lib/memory");
        let relocated = temp.path().join("resident/membrane");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(relocated.parent().unwrap()).unwrap();
        std::fs::write(&relocated, b"fixture").unwrap();
        std::fs::write(
            config_dir.join("runtime.json"),
            r#"{"schemaVersion":1,"serviceId":"membrane-local-v1","host":"127.0.0.1","port":47851}"#,
        )
        .unwrap();
        symlink(&relocated, bin.join("membrane")).unwrap();

        let runtime = runtime_from_exe_at_workspace(&relocated, Some(&workspace), false).unwrap();
        assert_eq!(runtime.port, 47851);
        #[cfg(target_os = "macos")]
        let expected_ort = "libonnxruntime.dylib";
        #[cfg(not(target_os = "macos"))]
        let expected_ort = "libonnxruntime.so";
        assert_eq!(runtime.ort, bin.join(expected_ort));

        let other = temp.path().join("resident/other-service");
        std::fs::write(&other, b"other").unwrap();
        assert!(runtime_from_exe_at_workspace(&other, Some(&workspace), false).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bundled_service_requires_authenticated_hub_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let bin = workspace.join("tools/bin");
        let config_dir = workspace.join("tools/lib/memory");
        let app_bin = temp.path().join("Membrane Hub.app/Contents/MacOS");
        let membrane = app_bin.join("membrane");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&app_bin).unwrap();
        std::fs::write(&membrane, b"fixture").unwrap();
        std::fs::write(app_bin.join("membrane-hub"), b"fixture").unwrap();
        std::fs::write(
            config_dir.join("runtime.json"),
            r#"{"schemaVersion":1,"serviceId":"membrane-local-v1","host":"127.0.0.1","port":47851}"#,
        )
        .unwrap();

        assert!(runtime_from_exe_at_workspace(&membrane, Some(&workspace), false).is_err());
        let runtime = runtime_from_exe_at_workspace(&membrane, Some(&workspace), true).unwrap();
        assert_eq!(runtime.port, 47851);
        assert_eq!(
            runtime.db,
            workspace.join("tools/.cache/memory/cortex-engine.db")
        );
    }

    #[test]
    fn resident_startup_advances_identity_and_publishes_claim_before_serve() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = Runtime {
            workspace_root: temp.path().to_path_buf(),
            db: temp.path().join("tools/.cache/memory/cortex-engine.db"),
            token: temp.path().join("tools/.cache/memory/api-token"),
            ort: temp.path().join("tools/bin/onnxruntime.dll"),
            hf_home: temp.path().join("tools/.cache/fastembed"),
            port: 47851,
            origin: "development",
            stable_current: None,
            version_root: None,
        };

        let (identity, claim) = prepare_runtime_identity(&runtime).unwrap();

        assert_eq!(identity.startup_generation, 1);
        assert_eq!(claim.installation_id, identity.installation_id);
        assert!(temp
            .path()
            .join("memory-mirror/_installation_claims")
            .join(&claim.installation_id)
            .join(format!("{:020}", claim.startup_generation))
            .join(format!("{}.json", claim.service_instance_id))
            .is_file());
    }

    #[test]
    fn migrated_credential_then_prepare_advances_generation_once() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = Runtime {
            workspace_root: temp.path().to_path_buf(),
            db: temp.path().join("tools/.cache/memory/cortex-engine.db"),
            token: temp.path().join("tools/.cache/memory/api-token"),
            ort: temp.path().join("tools/bin/onnxruntime.dll"),
            hf_home: temp.path().join("tools/.cache/fastembed"),
            port: 47851,
            origin: "installed",
            stable_current: None,
            version_root: None,
        };
        std::fs::create_dir_all(runtime.token.parent().unwrap()).unwrap();
        std::fs::write(&runtime.token, b"legacy\n").unwrap();
        let paths = crate::installation_identity::InstallationPaths::for_workspace(&runtime.workspace_root);
        crate::installation_identity::load_or_create_installation(&paths.identity, &[]).unwrap();
        assert!(crate::serve::migrate_installed_credential(&runtime.token).unwrap());
        let (identity, claim) = prepare_runtime_identity(&runtime).unwrap();
        assert_eq!(identity.startup_generation, 1);
        assert_eq!(claim.startup_generation, 1);
    }

    struct StubRefresh {
        fail_refresh: bool,
        refreshes: std::sync::atomic::AtomicU64,
    }

    impl membrane_blueprint::BlueprintOperation for StubRefresh {
        fn execute(
            &self,
            request: &BlueprintRequest,
            _context: &membrane_blueprint::RequestContext,
        ) -> Result<serde_json::Value, membrane_blueprint::BlueprintError> {
            if request.method == Operation::Refresh {
                self.refreshes.fetch_add(1, Ordering::SeqCst);
                if self.fail_refresh {
                    return Err(membrane_blueprint::BlueprintError::new(
                        "refresh_failed",
                        "stub refresh failure",
                    ));
                }
            }
            Ok(serde_json::json!({"generationId": "stub-gen", "complete": true}))
        }
    }

    fn resident_repo(root: &Path, fail_refresh: bool) -> ResidentRepo {
        let service = Arc::new(NativeService::from_operation(
            StubRefresh { fail_refresh, refreshes: std::sync::atomic::AtomicU64::new(0) },
            membrane_blueprint::ServiceConfig::new(root),
        ));
        service.start().unwrap();
        ResidentRepo {
            root: root.to_string_lossy().into_owned(),
            service,
            generation_id: "stub-gen".into(),
            generation_complete: true,
        }
    }

    fn resident_state(repos: Vec<ResidentRepo>, enrolled: u64) -> Arc<Mutex<ResidentBlueprintState>> {
        Arc::new(Mutex::new(ResidentBlueprintState {
            repos,
            enrolled_repo_count: enrolled,
            registry_error: None,
            cancellation: CancellationToken::new(),
            build_failures: std::collections::HashMap::new(),
            supervisor_stage: Arc::new(Mutex::new(String::new())),
        }))
    }

    #[test]
    fn build_retry_backoff_is_exponential_and_bounded() {
        assert_eq!(retry_cooldown(0), Duration::from_secs(120));
        assert_eq!(retry_cooldown(1), Duration::from_secs(120));
        assert_eq!(retry_cooldown(2), Duration::from_secs(240));
        assert_eq!(retry_cooldown(3), Duration::from_secs(480));
        assert_eq!(retry_cooldown(4), Duration::from_secs(600));
        assert_eq!(retry_cooldown(30), Duration::from_secs(600));
    }

    #[test]
    fn failed_root_is_not_retried_inside_its_cooldown() {
        let mut failures = std::collections::HashMap::new();
        let now = Instant::now();
        assert!(retry_due(&failures, "root-a", now));
        failures.insert("root-a".to_owned(), (1, now));
        assert!(!retry_due(&failures, "root-a", now + Duration::from_secs(119)));
        assert!(retry_due(&failures, "root-a", now + Duration::from_secs(120)));
        // A second failure doubles the window; another enrolled root is
        // unaffected by root-a's backoff.
        failures.insert("root-a".to_owned(), (2, now));
        assert!(!retry_due(&failures, "root-a", now + Duration::from_secs(239)));
        assert!(retry_due(&failures, "root-a", now + Duration::from_secs(240)));
        assert!(retry_due(&failures, "root-b", now));
    }

    #[test]
    fn supervise_retires_only_the_failed_root_and_records_it() {
        let healthy_root = tempfile::tempdir().unwrap();
        let failed_root = tempfile::tempdir().unwrap();
        let state = resident_state(
            vec![
                resident_repo(healthy_root.path(), false),
                resident_repo(failed_root.path(), true),
            ],
            2,
        );
        // A source edit under the failed root produces the refresh event its
        // stub then rejects.
        std::fs::write(failed_root.path().join("changed.txt"), b"changed").unwrap();

        assert!(supervise_resident_repositories(&state));

        let state = state.lock().unwrap();
        assert_eq!(state.repos.len(), 1);
        assert_eq!(state.repos[0].root, healthy_root.path().to_string_lossy());
        let detail = state.registry_error.as_deref().unwrap_or("");
        assert!(detail.contains(&*failed_root.path().to_string_lossy()), "registry_error: {detail}");
        assert!(state.build_failures.contains_key(&*failed_root.path().to_string_lossy()));
        assert!(!state.build_failures.contains_key(&*healthy_root.path().to_string_lossy()));
    }

    #[test]
    fn status_surfaces_failed_root_as_typed_watcher_detail() {
        let healthy_root = tempfile::tempdir().unwrap();
        let state = resident_state(vec![resident_repo(healthy_root.path(), false)], 2);
        state.lock().unwrap().registry_error =
            Some("resident Blueprint watcher /stale-root: Degraded: watcher unavailable".into());

        let status = resident_status_json(&state.lock().unwrap());
        assert_eq!(status["watcherRunning"], serde_json::json!(false));
        assert_eq!(status["watcherState"], serde_json::json!("watcher_unavailable"));
        assert_eq!(status["watcherCoverage"], serde_json::json!("partial"));
        assert_eq!(status["enrolledRepoCount"], serde_json::json!(2));
        let detail = status["watcherDetail"].as_str().unwrap_or("");
        assert!(detail.contains("/stale-root"), "watcherDetail: {detail}");
        // The surviving root still reports its own identity; the failed root's
        // absence is the visibility contract, not a silent omission.
        let identities = status["watcherIdentity"].as_array().unwrap();
        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0]["root"], serde_json::json!(healthy_root.path().to_string_lossy()));
    }
}
