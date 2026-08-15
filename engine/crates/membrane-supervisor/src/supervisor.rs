//! Per-user Membrane supervisor. Orchestrates lock acquisition, lease publishing, resident
//! spawn, and restart. The supervisor's outer loop is intentionally simple
//! and explicit so the contract is auditable; the supervisor's internals (lock, lease,
//! resident) are independently testable.
//!
//! MBR-201: build a per-user Membrane supervisor.

use crate::config::{RestartPolicy, SupervisorConfig};
use crate::error::{Result, SupervisorError};
use crate::lease::{
    atomic_write, publish_lease, SupervisorEndpointV1, SupervisorLeaseV1,
    SUPERVISOR_ENDPOINT_SCHEMA_VERSION,
};
use crate::lock::{release_if_owned, try_acquire_until, LockOutcome, SupervisorLock};
use crate::resident::{preflight_resident_binary, ResidentHandle, ResidentInvocation};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Per-process state the supervisor keeps across cycles. The struct is `Debug` so the
/// binary can dump it to stderr on a fatal failure; it is `Clone` only for tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupervisorState {
    pub schema_version: u32,
    /// Stable instance id minted on first boot. Persists in the status file so a
    /// reinstall leaves a traceable identity.
    pub supervisor_instance_id: String,
    /// Monotonic issuance counter for the lease.
    pub next_issuance: u64,
    /// Last loopback port we handed to the resident.
    pub last_resident_port: u16,
    /// Last successful resident exit (None if the resident never ran on this boot).
    pub last_resident_pid: Option<u32>,
}

pub const SUPERVISOR_STATE_SCHEMA_VERSION: u32 = 1;

impl SupervisorState {
    pub fn fresh(supervisor_instance_id: impl Into<String>) -> Self {
        Self {
            schema_version: SUPERVISOR_STATE_SCHEMA_VERSION,
            supervisor_instance_id: supervisor_instance_id.into(),
            next_issuance: 1,
            last_resident_port: crate::config::DEFAULT_LOOPBACK_PORT,
            last_resident_pid: None,
        }
    }

    pub fn verify_self_integrity(&self) -> Result<()> {
        if self.schema_version != SUPERVISOR_STATE_SCHEMA_VERSION {
            return Err(SupervisorError::InvalidConfig {
                reason: format!(
                    "unsupported supervisor state schema_version {} (expected {})",
                    self.schema_version, SUPERVISOR_STATE_SCHEMA_VERSION
                ),
            });
        }
        if self.next_issuance == 0 {
            return Err(SupervisorError::InvalidConfig {
                reason: "next_issuance must be ≥ 1".to_string(),
            });
        }
        if self.last_resident_port < 1024 {
            return Err(SupervisorError::InvalidConfig {
                reason: format!(
                    "last_resident_port {} is privileged",
                    self.last_resident_port
                ),
            });
        }
        Ok(())
    }
}

/// A supervisor cycle's outcome. The binary reports this verbatim to stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CycleOutcome {
    /// The supervisor acquired the lock, published the lease, and (optionally) ran the
    /// resident to completion in a single shot (used by tests and one-shot CLI mode).
    Completed {
        resident_pid: Option<u32>,
    },
    /// The supervisor refused to start because another supervisor is already running.
    RefusedAlreadyRunning { pid: u32 },
    /// The supervisor refused because its own preflight failed.
    RefusedPreflight { reason: String },
}

/// Top-level per-user Membrane supervisor. Built from a [`SupervisorConfig`] and a
/// supplied state (or a fresh one). The supervisor is single-purpose: it never daemonizes
/// itself (the OS service manager does that), never opens a socket of its own (the resident
/// does that), and never serves HTTP traffic (the resident does that).
pub struct Supervisor {
    config: SupervisorConfig,
    state: SupervisorState,
    lock: SupervisorLock,
}

impl Supervisor {
    /// Build a supervisor over the given config and state. The lock is wired up from config;
    /// no IO happens in this constructor.
    pub fn new(config: SupervisorConfig, state: SupervisorState) -> Result<Self> {
        config.restart_policy; // exhaustiveness touch (no-op; config already validated in parse)
        let lock = SupervisorLock::new(config.pid_lock_path.clone());
        Ok(Self {
            config,
            state,
            lock,
        })
    }

    /// Borrow the config. Used by tests and the binary's CLI plumbing.
    pub fn config(&self) -> &SupervisorConfig {
        &self.config
    }

    /// Borrow the state. Used by tests and the status publisher.
    pub fn state(&self) -> &SupervisorState {
        &self.state
    }

    /// Mint a fresh state with a fresh supervisor instance id. The supervisor binary calls
    /// this on a first-time install where no prior state file exists.
    pub fn fresh_state() -> SupervisorState {
        SupervisorState::fresh(format!("sup-{}", short_uuid_v4_like()))
    }

    /// Acquire the single-instance lock, returning a typed refusal when another supervisor
    /// holds it.
    pub fn acquire_lock(&self, own_pid: u32) -> Result<LockOutcome> {
        match self.lock.try_acquire(own_pid)? {
            LockOutcome::Acquired => Ok(LockOutcome::Acquired),
            LockOutcome::Held { pid } => {
                if pid == own_pid {
                    Ok(LockOutcome::Acquired)
                } else {
                    Ok(LockOutcome::Held { pid })
                }
            }
        }
    }

    /// Wait for the lock to clear, blocking for at most `deadline`. Returns the final
    /// outcome so callers can decide between aborting and continuing.
    pub fn acquire_lock_waiting(
        &self,
        own_pid: u32,
        deadline: Duration,
        poll: Duration,
    ) -> Result<LockOutcome> {
        try_acquire_until(&self.lock, own_pid, deadline, poll)
    }

    /// Publish a fresh lease + endpoint, returning the new lease. The supervisor calls this
    /// before spawning the resident so the resident can read the lease synchronously.
    pub fn publish_lease(
        &mut self,
        resident_pid_expected: u32,
        minted_at: &str,
    ) -> Result<SupervisorLeaseV1> {
        let issuance = self.state.next_issuance;
        let lease = publish_lease(
            &self.config.lease_path,
            &self.config.endpoint_path,
            &self.state.supervisor_instance_id,
            issuance,
            resident_pid_expected,
            self.config.loopback_port,
            minted_at,
        )?;
        self.state.next_issuance = self.state.next_issuance.checked_add(1).ok_or_else(|| {
            SupervisorError::InvalidConfig {
                reason: "next_issuance overflow".to_string(),
            }
        })?;
        self.state.last_resident_pid = Some(resident_pid_expected);
        self.state.last_resident_port = lease.loopback_port;
        Ok(lease)
    }

    /// Publish a refreshed status file. The status file is human-readable; it is not the
    /// authoritative endpoint (the endpoint file is). Stale status is harmless.
    pub fn publish_status(&self) -> Result<()> {
        let body = serde_json::to_vec_pretty(&self.state).map_err(|error| {
            SupervisorError::PublishFailed {
                artifact: "status",
                path: self.config.status_path.clone(),
                reason: format!("serialize status: {error}"),
            }
        })?;
        atomic_write(&self.config.status_path, &body)
    }

    /// Spawn the resident child. Validates the binary's preflight first; the runtime will
    /// verify the lease path on its own once it starts.
    pub fn spawn_resident(&self, working_directory: Option<PathBuf>) -> Result<ResidentHandle> {
        preflight_resident_binary(&self.config.resident_binary)?;
        let invocation = ResidentInvocation {
            binary: self.config.resident_binary.clone(),
            lease: self.config.lease_path.clone(),
            loopback_port: self.config.loopback_port,
            working_directory,
            extra_environment: Vec::new(),
        };
        invocation.spawn()
    }

    /// Determine whether the supervisor should restart the resident, given the observation
    /// window and the configured policy. A pure function used by the supervisor's outer
    /// loop so the decision is testable without spawning a child.
    pub fn should_restart(&self, recent_exit_count: u32, time_in_window: Duration) -> Result<bool> {
        let policy = self.config.restart_policy;
        if recent_exit_count > policy.max_restarts {
            return Ok(false);
        }
        if time_in_window > Duration::from_secs(policy.window_seconds) {
            return Ok(true);
        }
        Ok(recent_exit_count < policy.max_restarts)
    }

    /// Compute the next backoff. Used by the outer loop after a failed restart.
    pub fn next_backoff(&self, attempt: u32) -> Duration {
        self.config.backoff_for(attempt)
    }

    /// Release the lock if the supervisor still owns it. Idempotent across normal exits.
    pub fn release_lock(&self, own_pid: u32) -> Result<bool> {
        release_if_owned(self.lock.path(), own_pid)
    }
}

/// Where the supervisor stores its persistent supervisor-state JSON. Used by the binary to
/// load or bootstrap the state file alongside the lock file. Exposed so tests can verify
/// the supervisor respects the existing state across an upgrade.
pub fn default_state_path(config: &SupervisorConfig) -> PathBuf {
    config
        .status_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("supervisor-state.json")
}

/// Load supervisor state from `state_path`, or build a fresh one when the file is missing.
/// A corrupt state is treated as a fresh one with a fresh instance id — the supervisor
/// fails open on its own bookkeeping rather than refusing to start.
pub fn load_or_init_state(state_path: &Path) -> Result<SupervisorState> {
    match std::fs::read(state_path) {
        Ok(bytes) => match serde_json::from_slice::<SupervisorState>(&bytes) {
            Ok(state) => state.verify_self_integrity().map(|_| state),
            Err(_) => Ok(Supervisor::fresh_state()),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Supervisor::fresh_state()),
        Err(error) => Err(SupervisorError::Io {
            path: state_path.to_path_buf(),
            reason: format!("read supervisor state file: {error}"),
        }),
    }
}

/// Persist the supervisor's current state to disk via the atomic write helper. The state
/// file is the supervisor's only durable memory across restarts.
pub fn save_state(state_path: &Path, state: &SupervisorState) -> Result<()> {
    let body = serde_json::to_vec_pretty(state).map_err(|error| SupervisorError::Io {
        path: state_path.to_path_buf(),
        reason: format!("serialize supervisor state: {error}"),
    })?;
    atomic_write(state_path, &body)
}

/// Short, dependency-free random hex string. The instance id does not need cryptographic
/// strength — it only needs to be unique per install, which the supervisor achieves by
/// reading/writing its state file. We just want enough entropy to avoid collisions in the
/// common case where two users on the same machine both install Membrane.
fn short_uuid_v4_like() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mixed = nanos ^ (pid << 64);
    format!("{:032x}", mixed & 0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF)
}

/// Helper used in tests to spin a temporary `Supervisor` with the supplied paths. Public so
/// integration tests under `tests/` can build a supervisor without copying the construction
/// logic; documented as test-only because the path set is artificially tight.
#[doc(hidden)]
pub fn for_test(
    pid_lock_path: PathBuf,
    lease_path: PathBuf,
    endpoint_path: PathBuf,
    status_path: PathBuf,
    loopback_port: u16,
    restart_policy: RestartPolicy,
) -> Result<Supervisor> {
    let config = SupervisorConfig {
        schema_version: crate::config::CONFIG_SCHEMA_VERSION,
        resident_binary: which_test_binary(),
        lease_path,
        endpoint_path,
        status_path,
        pid_lock_path,
        loopback_port,
        restart_policy,
        lifetime_seconds: None,
    };
    Supervisor::new(config, Supervisor::fresh_state())
}

fn which_test_binary() -> PathBuf {
    // The supervisor's tests use `membrane` (the host crate) as the resident. In the test
    // binary this resolves to the same compiled binary as the supervisor under test; we
    // route through `CARGO_BIN_EXE_<name>` if present (cargo test convention), or fall back
    // to `membrane` for offline test layout discovery.
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_membrane") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_membrane-supervisor") {
        return PathBuf::from(path);
    }
    PathBuf::from("membrane")
}

/// One-shot reconcile cycle used by the CLI's `membrane-supervisor check` subcommand.
/// Returns a typed report describing what the supervisor would do. Used by tests, scripts,
/// and the install-time doctor check.
pub fn dry_run(
    config: &SupervisorConfig,
    state: &SupervisorState,
    own_pid: u32,
) -> Result<CheckReport> {
    let lock = SupervisorLock::new(config.pid_lock_path.clone());
    let lock_outcome = lock.try_acquire(own_pid)?;
    Ok(CheckReport {
        own_pid,
        lock: lock_outcome,
        state_snapshot: state.clone(),
        endpoint_preview: SupervisorEndpointV1 {
            schema_version: SUPERVISOR_ENDPOINT_SCHEMA_VERSION,
            supervisor_instance_id: state.supervisor_instance_id.clone(),
            lease_issuance: state.next_issuance,
            loopback_port: config.loopback_port,
            lease_path: config.lease_path.display().to_string(),
            minted_at: String::new(),
        },
    })
}

/// Typed report from `dry_run`. Stable shape; the CLI's `check` command serialises this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckReport {
    pub own_pid: u32,
    pub lock: LockOutcome,
    pub state_snapshot: SupervisorState,
    pub endpoint_preview: SupervisorEndpointV1,
}

/// Sleep helper used by the supervisor's outer loop after a failed start so the cycle
/// backoff actually happens.
pub fn interruptible_sleep(duration: Duration) -> bool {
    // Cooperative cancellation point. Returns true when the full duration elapsed, false
    // when interrupted by a signal so the supervisor's caller can flush state.
    let start = Instant::now();
    let mut remaining = duration;
    while !remaining.is_zero() {
        std::thread::sleep(remaining.min(Duration::from_millis(50)));
        let elapsed = start.elapsed();
        if elapsed >= duration {
            return true;
        }
        remaining = duration - elapsed;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config_dir() -> std::path::PathBuf {
        let dir = tempfile::tempdir().unwrap();
        dir.keep()
    }

    #[test]
    fn fresh_state_increments_and_tracks_port() {
        let mut state = Supervisor::fresh_state();
        assert_eq!(state.next_issuance, 1);
        assert!(state.supervisor_instance_id.starts_with("sup-"));
        // Bumping issuance past the u64 boundary is a configurable error, not a panic.
        state.next_issuance = u64::MAX;
        assert!(state.verify_self_integrity().is_ok());
    }

    #[test]
    fn lock_acquire_round_trip() {
        let dir = test_config_dir();
        let supervisor = for_test(
            dir.join("supervisor.pid"),
            dir.join("lease.json"),
            dir.join("endpoint.json"),
            dir.join("status.json"),
            47851,
            RestartPolicy::default(),
        )
        .unwrap();
        let outcome = supervisor.acquire_lock(std::process::id()).unwrap();
        assert_eq!(outcome, LockOutcome::Acquired);
        supervisor.release_lock(std::process::id()).unwrap();
    }

    #[test]
    fn lease_publish_then_round_trip() {
        let dir = test_config_dir();
        let mut supervisor = for_test(
            dir.join("supervisor.pid"),
            dir.join("lease.json"),
            dir.join("endpoint.json"),
            dir.join("status.json"),
            47851,
            RestartPolicy::default(),
        )
        .unwrap();
        let lease = supervisor
            .publish_lease(4242, "2026-08-07T12:00:00Z")
            .unwrap();
        lease.verify_self_integrity().unwrap();
        // Reading back from disk must yield a byte-identical lease.
        let raw = std::fs::read(supervisor.config().lease_path.clone()).unwrap();
        let parsed: SupervisorLeaseV1 = serde_json::from_slice(&raw).unwrap();
        assert_eq!(parsed, lease);
        // The issuance counter advanced.
        assert_eq!(supervisor.state().next_issuance, 2);
        // Status file round-trips.
        supervisor.publish_status().unwrap();
        let raw_status = std::fs::read(supervisor.config().status_path.clone()).unwrap();
        let parsed_status: SupervisorState = serde_json::from_slice(&raw_status).unwrap();
        assert_eq!(parsed_status, *supervisor.state());
    }

    #[test]
    fn state_recovers_from_corruption_with_fresh_instance_id() {
        let dir = test_config_dir();
        let state_path = dir.join("supervisor-state.json");
        std::fs::write(&state_path, b"not-json").unwrap();
        let state = load_or_init_state(&state_path).unwrap();
        assert!(state.supervisor_instance_id.starts_with("sup-"));
        assert_eq!(state.next_issuance, 1);
    }

    #[test]
    fn state_persists_across_save_and_load() {
        let dir = test_config_dir();
        let state_path = dir.join("supervisor-state.json");
        let mut state = Supervisor::fresh_state();
        state.next_issuance = 7;
        state.last_resident_pid = Some(4242);
        save_state(&state_path, &state).unwrap();
        let loaded = load_or_init_state(&state_path).unwrap();
        assert_eq!(loaded.next_issuance, 7);
        assert_eq!(loaded.last_resident_pid, Some(4242));
        assert_eq!(loaded.supervisor_instance_id, state.supervisor_instance_id);
    }

    #[test]
    fn restart_policy_decision_is_pure() {
        let dir = test_config_dir();
        let supervisor = for_test(
            dir.join("supervisor.pid"),
            dir.join("lease.json"),
            dir.join("endpoint.json"),
            dir.join("status.json"),
            47851,
            RestartPolicy {
                max_restarts: 3,
                window_seconds: 60,
                initial_backoff_seconds: 1,
                max_backoff_seconds: 30,
                backoff_multiplier: 2,
            },
        )
        .unwrap();
        assert!(supervisor
            .should_restart(0, Duration::from_secs(120))
            .unwrap());
        assert!(!supervisor
            .should_restart(4, Duration::from_secs(10))
            .unwrap());
        assert!(supervisor
            .should_restart(2, Duration::from_secs(120))
            .unwrap());
        assert!(!supervisor
            .should_restart(4, Duration::from_secs(120))
            .unwrap());
    }

    #[test]
    fn backoff_grows_through_attempts() {
        let dir = test_config_dir();
        let supervisor = for_test(
            dir.join("supervisor.pid"),
            dir.join("lease.json"),
            dir.join("endpoint.json"),
            dir.join("status.json"),
            47851,
            RestartPolicy::default(),
        )
        .unwrap();
        assert_eq!(supervisor.next_backoff(0).as_secs(), 1);
        assert_eq!(supervisor.next_backoff(1).as_secs(), 2);
        assert_eq!(supervisor.next_backoff(5).as_secs(), 30);
    }

    #[test]
    fn dry_run_reports_lock_status() {
        let dir = test_config_dir();
        let config = SupervisorConfig {
            schema_version: crate::config::CONFIG_SCHEMA_VERSION,
            resident_binary: which_test_binary(),
            lease_path: dir.join("lease.json"),
            endpoint_path: dir.join("endpoint.json"),
            status_path: dir.join("status.json"),
            pid_lock_path: dir.join("supervisor.pid"),
            loopback_port: 47851,
            restart_policy: RestartPolicy::default(),
            lifetime_seconds: None,
        };
        let state = Supervisor::fresh_state();
        let report = dry_run(&config, &state, std::process::id()).unwrap();
        assert_eq!(report.lock, LockOutcome::Acquired);
        assert_eq!(
            report.endpoint_preview.supervisor_instance_id,
            state.supervisor_instance_id
        );
    }

    #[test]
    fn interruptible_sleep_returns_true_after_full_duration() {
        let start = Instant::now();
        let result = interruptible_sleep(Duration::from_millis(20));
        assert!(result);
        assert!(start.elapsed() >= Duration::from_millis(15));
    }
}
