//! Supervisor configuration. Parses the JSON config the installer lays down and holds the
//! resolved paths the runtime uses for each per-user artifact.
//!
//! MBR-201: build a per-user Membrane supervisor.

use crate::error::{Result, SupervisorError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Schema version of the supervisor config. The supervisor refuses config files with a newer
/// `schema_version` so a downgrade cannot silently widen authority.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

/// Default unprivileged loopback port the supervisor expects the resident to bind. Mirrors
/// the constant embedded in `membrane-dispatch::LoopbackArgs` so the supervisor's default
/// is consistent with the membrane binary's own default.
pub const DEFAULT_LOOPBACK_PORT: u16 = 47851;

/// Restart policy for the resident child. The supervisor enforces this on every observed
/// exit so a crash loop cannot wedge the supervisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestartPolicy {
    /// Maximum restarts within `window_seconds` before the supervisor disables itself.
    pub max_restarts: u32,
    /// Sliding window in seconds used to count restarts against `max_restarts`.
    pub window_seconds: u64,
    /// Initial backoff before the first restart. Subsequent restarts multiply by
    /// `backoff_multiplier` (rounded down) up to `max_backoff_seconds`.
    pub initial_backoff_seconds: u64,
    /// Cap on the per-restart backoff so a late-night crash loop cannot make the supervisor
    /// wait hours between attempts.
    pub max_backoff_seconds: u64,
    /// Linear backoff multiplier applied after each restart.
    pub backoff_multiplier: u32,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 5,
            window_seconds: 60,
            initial_backoff_seconds: 1,
            max_backoff_seconds: 30,
            backoff_multiplier: 2,
        }
    }
}

/// Watcher coordination policy. The supervisor never spawns or owns the watcher;
/// it only observes the pidfile to learn whether a live watcher exists (purely
/// informational, never influencing what Membrane spawns). Duplicate watchers are
/// impossible because Membrane never spawns — it only adopts a live pid as a
/// read-only liveness probe, per D-S09.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatcherPolicy {
    /// Reserved flag; the supervisor never spawns regardless of this value.
    /// `true`/`false` both mean "observe only" — the supervisor only adopts a
    /// live pid as a read-only liveness probe and never spawns, per D-S09.
    pub managed: bool,
    /// PID file the watcher is required to publish. The supervisor reads this on every
    /// decision cycle and refuses to spawn a duplicate.
    pub pid_file: PathBuf,
    /// Optional path to the watcher script. When `None`, the coordinator reports
    /// `Unavailable`. Retained for `Adopt`'s script-presence check to distinguish
    /// "cortex not installed" from a dead pid — not used to spawn, per D-S09.
    pub script: Option<PathBuf>,
}

/// Typed supervisor config, deserialized from the JSON the installer writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupervisorConfig {
    pub schema_version: u32,
    /// Path to the `membrane` resident binary the supervisor will spawn.
    pub resident_binary: PathBuf,
    /// Path the supervisor publishes its lease file at. The resident child reads this from
    /// `MEMBRANE_SUPERVISOR_LEASE` (set when invoked with `--lease`).
    pub lease_path: PathBuf,
    /// Path the supervisor publishes its endpoint discovery file at. Client adapters read
    /// this to learn the supervisor's loopback port and the active instance id.
    pub endpoint_path: PathBuf,
    /// Path the supervisor publishes its status file at. Distinct from the endpoint file so
    /// client adapters can poll cheap status without parsing the endpoint shape.
    pub status_path: PathBuf,
    /// Path the supervisor writes its PID lock to. Must be on a per-user filesystem location
    /// so two users on the same machine do not collide.
    pub pid_lock_path: PathBuf,
    /// Loopback port the supervisor expects the resident to bind. Defaults to
    /// [`DEFAULT_LOOPBACK_PORT`].
    pub loopback_port: u16,
    /// Restart policy applied to the resident.
    pub restart_policy: RestartPolicy,
    /// Watcher coordination policy.
    pub watcher_policy: WatcherPolicy,
    /// Hard ceiling on the supervisor's lifetime in seconds. The OS service manager restarts
    /// the supervisor when this elapses, forcing a clean process identity. `None` keeps the
    /// supervisor running indefinitely (subject to crash-only restart).
    pub lifetime_seconds: Option<u64>,
}

impl SupervisorConfig {
    /// Parse a config from a JSON byte slice. Refuses unknown schema versions.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let config: SupervisorConfig =
            serde_json::from_slice(bytes).map_err(|error| SupervisorError::InvalidConfig {
                reason: format!("config JSON did not parse: {error}"),
            })?;
        if config.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(SupervisorError::InvalidConfig {
                reason: format!(
                    "unsupported supervisor config schema_version {} (expected {})",
                    config.schema_version, CONFIG_SCHEMA_VERSION
                ),
            });
        }
        if config.loopback_port < 1024 {
            return Err(SupervisorError::InvalidConfig {
                reason: format!(
                    "loopback_port {} is privileged; supervisor refuses ports < 1024",
                    config.loopback_port
                ),
            });
        }
        if config.restart_policy.max_restarts == 0 {
            return Err(SupervisorError::InvalidConfig {
                reason: "restart_policy.max_restarts must be ≥ 1".to_string(),
            });
        }
        if config.restart_policy.window_seconds == 0 {
            return Err(SupervisorError::InvalidConfig {
                reason: "restart_policy.window_seconds must be ≥ 1".to_string(),
            });
        }
        if config.restart_policy.initial_backoff_seconds > config.restart_policy.max_backoff_seconds
        {
            return Err(SupervisorError::InvalidConfig {
                reason: "restart_policy.initial_backoff_seconds must be ≤ max_backoff_seconds"
                    .to_string(),
            });
        }
        if config.restart_policy.backoff_multiplier < 1 {
            return Err(SupervisorError::InvalidConfig {
                reason: "restart_policy.backoff_multiplier must be ≥ 1".to_string(),
            });
        }
        Ok(config)
    }

    /// Compute the next backoff for the given restart attempt index (0-based). Clamps to
    /// `max_backoff_seconds` and uses saturating math so a runaway multiplier cannot overflow.
    pub fn backoff_for(&self, attempt: u32) -> std::time::Duration {
        let policy = self.restart_policy;
        let initial = policy.initial_backoff_seconds;
        let max = policy.max_backoff_seconds;
        if attempt == 0 {
            return std::time::Duration::from_secs(initial.min(max));
        }
        let multiplier = (policy.backoff_multiplier as u64).saturating_pow(attempt.min(20));
        let scaled = initial.saturating_mul(multiplier);
        std::time::Duration::from_secs(scaled.min(max))
    }

    /// The directory the supervisor writes per-user artifacts into. Used by installers to
    /// know what tree they must pre-create.
    pub fn artifact_directory(&self) -> Option<&Path> {
        self.endpoint_path.parent()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> SupervisorConfig {
        SupervisorConfig {
            schema_version: 1,
            resident_binary: PathBuf::from("/opt/membrane/bin/membrane"),
            lease_path: PathBuf::from("/tmp/membrane/lease.json"),
            endpoint_path: PathBuf::from("/tmp/membrane/endpoint.json"),
            status_path: PathBuf::from("/tmp/membrane/status.json"),
            pid_lock_path: PathBuf::from("/tmp/membrane/supervisor.pid"),
            loopback_port: 47851,
            restart_policy: RestartPolicy::default(),
            watcher_policy: WatcherPolicy {
                managed: true,
                pid_file: PathBuf::from("/tmp/membrane/watchman.pid"),
                script: Some(PathBuf::from("/opt/membrane/bin/cortex-watch.mjs")),
            },
            lifetime_seconds: Some(86_400),
        }
    }

    #[test]
    fn parse_accepts_canonical_config() {
        let config = base_config();
        let bytes = serde_json::to_vec(&config).unwrap();
        let parsed = SupervisorConfig::parse(&bytes).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn parse_rejects_unknown_schema_version() {
        let mut config = base_config();
        config.schema_version = 99;
        let bytes = serde_json::to_vec(&config).unwrap();
        let err = SupervisorConfig::parse(&bytes).unwrap_err();
        assert_eq!(err.label(), "invalid-config");
        assert!(format!("{err}").contains("schema_version 99"));
    }

    #[test]
    fn parse_rejects_privileged_port() {
        let mut config = base_config();
        config.loopback_port = 80;
        let bytes = serde_json::to_vec(&config).unwrap();
        let err = SupervisorConfig::parse(&bytes).unwrap_err();
        assert!(format!("{err}").contains("privileged"));
    }

    #[test]
    fn parse_rejects_zero_max_restarts() {
        let mut config = base_config();
        config.restart_policy.max_restarts = 0;
        let bytes = serde_json::to_vec(&config).unwrap();
        let err = SupervisorConfig::parse(&bytes).unwrap_err();
        assert!(format!("{err}").contains("max_restarts"));
    }

    #[test]
    fn parse_rejects_inverted_backoff_window() {
        let mut config = base_config();
        config.restart_policy.initial_backoff_seconds = 120;
        config.restart_policy.max_backoff_seconds = 60;
        let bytes = serde_json::to_vec(&config).unwrap();
        let err = SupervisorConfig::parse(&bytes).unwrap_err();
        assert!(format!("{err}").contains("initial_backoff_seconds"));
    }

    #[test]
    fn backoff_for_first_attempt_uses_initial() {
        let config = base_config();
        assert_eq!(config.backoff_for(0).as_secs(), 1);
    }

    #[test]
    fn backoff_grows_then_clamps() {
        let config = base_config();
        // attempt=1 → 1 * 2 = 2s
        assert_eq!(config.backoff_for(1).as_secs(), 2);
        // attempt=2 → 1 * 4 = 4s
        assert_eq!(config.backoff_for(2).as_secs(), 4);
        // attempt=10 → 1 * 1024 clamped at 30s
        assert_eq!(config.backoff_for(10).as_secs(), 30);
        // attempt=1_000_000 → still clamped, no overflow
        assert_eq!(config.backoff_for(1_000_000).as_secs(), 30);
    }
}
