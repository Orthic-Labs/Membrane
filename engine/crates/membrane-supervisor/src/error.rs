//! Typed supervisor errors. Every public surface uses these so the supervisor binary's stderr
//! can route the cause to launchd / Task Scheduler / systemd without parsing free-form text.
//!
//! MBR-201: build a per-user Membrane supervisor.

use std::path::PathBuf;
use thiserror::Error;

/// Every recoverable failure a supervisor site can observe. The binary maps each variant to a
/// non-zero exit code and to a stable log label.
#[derive(Debug, Error)]
pub enum SupervisorError {
    /// A supervisor is already running for this user; the new instance refused to start.
    #[error("another membrane-supervisor is already running with pid {pid}")]
    AlreadyRunning { pid: u32 },

    /// The supervisor tried to acquire its PID lock and the recorded PID was neither alive
    /// nor removable. The supervisor surfaces this as a hard failure rather than silently
    /// rewriting someone else's PID file.
    #[error("supervisor PID lock held by an unkillable process: {path}")]
    PidLockHeld { path: PathBuf },

    /// The config file could not be parsed or contained values that would make the supervisor
    /// refuse to start (e.g. port outside the unprivileged range).
    #[error("invalid supervisor configuration: {reason}")]
    InvalidConfig { reason: String },

    /// The resident binary was not at the configured path or could not be spawned.
    #[error("supervisor failed to spawn resident at {path}: {reason}")]
    ResidentSpawn { path: PathBuf, reason: String },

    /// The supervisor could not publish one of its discovery artifacts (lease file, endpoint
    /// JSON, status file).
    #[error("supervisor could not publish {artifact} at {path}: {reason}")]
    PublishFailed {
        artifact: &'static str,
        path: PathBuf,
        reason: String,
    },

    /// A watcher sidecar (cortex-watch.mjs) reported a state the supervisor could not act on.
    #[error("watcher coordinator could not decide action: {reason}")]
    WatcherDecision { reason: String },

    /// MBR-207: the heartbeat table reported one client as both
    /// installed and delivering — the Hub/CLI conflation the supervisor
    /// exists to prevent. Carries the offending client id and the two
    /// statuses so the receipt can be reproduced.
    #[error("heartbeat conflation for client {client_id}: installed and delivering coexist")]
    HeartbeatConflation {
        client_id: String,
        installed: bool,
        delivering: bool,
    },

    /// A generic IO failure. Kept distinct from the typed cases above so callers can match.
    #[error("supervisor IO failure at {path}: {reason}")]
    Io { path: PathBuf, reason: String },
}

impl SupervisorError {
    /// Stable short label suitable for log routing. Always lowercase, never user-facing prose.
    pub fn label(&self) -> &'static str {
        match self {
            SupervisorError::AlreadyRunning { .. } => "already-running",
            SupervisorError::PidLockHeld { .. } => "pid-lock-held",
            SupervisorError::InvalidConfig { .. } => "invalid-config",
            SupervisorError::ResidentSpawn { .. } => "resident-spawn-failed",
            SupervisorError::PublishFailed { .. } => "publish-failed",
            SupervisorError::WatcherDecision { .. } => "watcher-decision-failed",
            SupervisorError::HeartbeatConflation { .. } => "heartbeat-conflation",
            SupervisorError::Io { .. } => "io-failed",
        }
    }
}

/// Convenience result alias for the supervisor crate.
pub type Result<T> = std::result::Result<T, SupervisorError>;
