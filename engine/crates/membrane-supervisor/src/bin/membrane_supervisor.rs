//! `membrane-supervisor` — the per-user Membrane supervisor binary.
//!
//! Lifecycle:
//! 1. Read the JSON config from `--config <path>` (the installer writes this on its own).
//! 2. Acquire the single-instance PID lock.
//! 3. Publish a fresh lease + endpoint file.
//! 4. Decide whether the cortex watcher is already running (adopt) or missing (spawn).
//! 5. Spawn the resident (`membrane supervisor-child --lease <path>`) and wait.
//! 6. On exit, apply the restart policy; back off and try again.
//!
//! MBR-201: build a per-user Membrane supervisor.

#![cfg_attr(windows, windows_subsystem = "windows")]

use clap::{Parser, Subcommand};
use membrane_supervisor::{
    dry_run, load_or_init_state, save_state, CycleOutcome, LockOutcome, ResidentHandle, Supervisor,
    SupervisorConfig, SupervisorError, SupervisorState, WatcherAction,
};
use std::path::PathBuf;
use std::time::Duration;

const POLL_LOCK_INTERVAL: Duration = Duration::from_millis(250);
const LOCK_WAIT_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug, Parser)]
#[command(
    name = "membrane-supervisor",
    bin_name = "membrane-supervisor",
    about = "Per-user Membrane supervisor. Owns one resident, publishes discovery, dedupes the watcher.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// One-shot health check. Reads the config and reports lock + watcher status without
    /// launching anything. The doctor uses this on install/upgrade.
    Check {
        /// Path to the JSON config file.
        #[arg(long)]
        config: PathBuf,
    },
    /// Run the supervisor's outer loop. Manually invoked; on real installs the OS service
    /// manager (launchd / Task Scheduler / systemd) does this.
    Run {
        /// Path to the JSON config file.
        #[arg(long)]
        config: PathBuf,
        /// Maximum number of resident cycles before the supervisor exits 0. Used by tests
        /// to bound runs without touching the restart policy.
        #[arg(long)]
        max_resident_cycles: Option<u32>,
    },
    /// Print the path the supervisor would publish its endpoint file at, given the config.
    /// Used by client adapters and installers that want to inline the endpoint discovery.
    PrintEndpointPath {
        /// Path to the JSON config file.
        #[arg(long)]
        config: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    let exit_code = match run(cli) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("membrane-supervisor: {} ({})", error, error.label());
            map_exit_code(&error)
        }
    };
    std::process::exit(exit_code);
}

fn run(cli: Cli) -> Result<(), SupervisorError> {
    match cli.command {
        Command::Check { config } => check_subcommand(&config),
        Command::Run {
            config,
            max_resident_cycles,
        } => run_subcommand(&config, max_resident_cycles),
        Command::PrintEndpointPath { config } => {
            let parsed = read_config(&config)?;
            println!("{}", parsed.endpoint_path.display());
            Ok(())
        }
    }
}

fn read_config(path: &PathBuf) -> Result<SupervisorConfig, SupervisorError> {
    let bytes = std::fs::read(path).map_err(|error| SupervisorError::Io {
        path: path.clone(),
        reason: format!("read config: {error}"),
    })?;
    SupervisorConfig::parse(&bytes)
}

fn check_subcommand(config: &PathBuf) -> Result<(), SupervisorError> {
    let parsed = read_config(config)?;
    let state_path = membrane_supervisor::default_state_path(&parsed);
    let state = load_or_init_state(&state_path)?;
    let report = dry_run(&parsed, &state, std::process::id())?;
    let body = serde_json::to_string_pretty(&report).map_err(|error| SupervisorError::Io {
        path: PathBuf::from("<stdout>"),
        reason: format!("serialize check report: {error}"),
    })?;
    println!("{body}");
    Ok(())
}

fn run_subcommand(
    config: &PathBuf,
    max_resident_cycles: Option<u32>,
) -> Result<(), SupervisorError> {
    let parsed = read_config(config)?;
    let state_path = membrane_supervisor::default_state_path(&parsed);
    let state = load_or_init_state(&state_path)?;
    let mut supervisor = Supervisor::new(parsed.clone(), state)?;
    match supervisor.acquire_lock_waiting(
        std::process::id(),
        LOCK_WAIT_DEADLINE,
        POLL_LOCK_INTERVAL,
    )? {
        LockOutcome::Acquired => {}
        LockOutcome::Held { pid } => {
            eprintln!("membrane-supervisor: another supervisor already running with pid {pid}");
            return Err(SupervisorError::AlreadyRunning { pid });
        }
    }
    let outcome = run_outer_loop(&mut supervisor, &state_path, max_resident_cycles)?;
    let _ = supervisor.release_lock(std::process::id());
    eprintln!(
        "membrane-supervisor: outer loop ended cleanly: {:?}",
        outcome
    );
    Ok(())
}

/// Run the supervisor's outer loop. Each iteration: reconcile the watcher, publish a fresh
/// lease + endpoint, spawn the resident, wait for it. When the resident exits cleanly the
/// loop exits; when it exits with a non-zero code the loop applies the restart policy.
fn run_outer_loop(
    supervisor: &mut Supervisor,
    state_path: &std::path::Path,
    max_resident_cycles: Option<u32>,
) -> Result<CycleOutcome, SupervisorError> {
    let mut state: SupervisorState = supervisor.state().clone();
    let mut cycle: u32 = 0;
    loop {
        if let Some(cap) = max_resident_cycles {
            if cycle >= cap {
                return Ok(CycleOutcome::Completed {
                    resident_pid: state.last_resident_pid,
                    watcher_action: WatcherAction::Unavailable,
                });
            }
        }
        let watcher_action = supervisor.reconcile_watcher()?;
        let lease = supervisor.publish_lease(0, &iso8601_now())?;
        lease.verify_self_integrity()?;
        supervisor.publish_status()?;
        save_state(state_path, &state)?;
        let mut resident_handle: ResidentHandle = supervisor.spawn_resident(None)?;
        let own_pid = resident_handle.pid();
        state.last_resident_pid = Some(own_pid);
        let status = resident_handle
            .child()
            .wait()
            .map_err(|error| SupervisorError::Io {
                path: supervisor.config().resident_binary.clone(),
                reason: format!("wait on resident child: {error}"),
            })?;
        cycle = cycle.saturating_add(1);
        if !status.success() {
            let should_restart = supervisor.should_restart(
                cycle,
                Duration::from_secs(supervisor.config().restart_policy.window_seconds),
            )?;
            if !should_restart {
                return Ok(CycleOutcome::Completed {
                    resident_pid: Some(own_pid),
                    watcher_action,
                });
            }
            let backoff = supervisor.next_backoff(cycle);
            membrane_supervisor::interruptible_sleep(backoff);
        } else {
            return Ok(CycleOutcome::Completed {
                resident_pid: Some(own_pid),
                watcher_action,
            });
        }
    }
}

fn iso8601_now() -> String {
    // Approximate RFC 3339 UTC timestamp without bringing in chrono. Drift is bounded by
    // one day; the lease's minted_at is informational only.
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total = duration.as_secs();
    let secs_in_day = 86_400;
    let days = total / secs_in_day;
    let secs_today = total % secs_in_day;
    let h = secs_today / 3600;
    let m = (secs_today / 60) % 60;
    let s = secs_today % 60;
    let (y_off, mo, d) = civil_from_days(days as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        1970 + y_off,
        mo + 1,
        d + 1,
        h,
        m,
        s
    )
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    // Howard Hinnant's days_from_civil inverse, simplified — the supervisor's lease only
    // needs an approximate timestamp for forensic value. License: CC0.
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as i64;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as i64;
    let y = if m <= 2 { y + 1 } else { y };
    (y - 1970, m - 1, d)
}

fn map_exit_code(error: &SupervisorError) -> i32 {
    match error {
        // Mirrors the membrane binary's contract so platform scripts can use one table.
        SupervisorError::InvalidConfig { .. } => 2,
        SupervisorError::AlreadyRunning { .. } => 3,
        _ => 1,
    }
}
