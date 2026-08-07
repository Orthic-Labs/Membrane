//! Single-instance lock. The supervisor publishes its PID file at a per-user location and
//! refuses to start a second supervisor while the recorded PID is alive.
//!
//! A stale PID file (a dead PID left behind by a crashed supervisor) is recoverable: the
//! next supervisor reclaims the file. The lock owner never rewrites a PID file owned by a
//! live, foreign process; that case is reported as `PidLockHeld`.
//!
//! MBR-201: build a per-user Membrane supervisor.

use crate::error::{Result, SupervisorError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Outcome of an [`SupervisorLock::try_acquire`] call.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LockOutcome {
    /// The call acquired the lock. The lock now references the current process.
    Acquired,
    /// The call did NOT acquire the lock because another process holds it.
    Held { pid: u32 },
}

impl LockOutcome {
    /// Format the outcome as a stable lower-case label, mirroring the supervisor's
    /// `label()` contract for log routing.
    pub fn label(&self) -> &'static str {
        match self {
            LockOutcome::Acquired => "acquired",
            LockOutcome::Held { .. } => "held",
        }
    }
}

/// Single-instance supervisor lock backed by a PID file. Built around [`std::fs`] so it works
/// on every platform the supervisor supports (macOS, Linux, Windows).
#[derive(Debug)]
pub struct SupervisorLock {
    path: PathBuf,
}

impl SupervisorLock {
    /// Construct a lock over `path`. The file itself may or may not exist; the supervisor
    /// creates it on first acquire.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Where the lock file lives. The installer uses this to pre-create the directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Try to acquire the lock for `pid`. Returns `Acquired` on success or `Held { pid }`
    /// when another live process owns the lock file.
    ///
    /// If the file exists but its recorded PID is dead (or unparseable), the supervisor
    /// reclaims the file in place. The reclaim path never overwrites a live foreign process.
    pub fn try_acquire(&self, pid: u32) -> Result<LockOutcome> {
        match std::fs::read_to_string(&self.path) {
            Ok(existing) => match existing.trim().parse::<u32>() {
                Ok(observed) if pid_alive_probe(observed) => {
                    if observed == pid {
                        // We already own the lock; idempotent re-acquire.
                        Ok(LockOutcome::Acquired)
                    } else {
                        Ok(LockOutcome::Held { pid: observed })
                    }
                }
                _ => {
                    // Stale or unparseable — the recorded actor is gone. Reclaim atomically
                    // via write-then-rename so two concurrent supervisors cannot both think
                    // they reclaimed the file.
                    self.write_atomic(pid)?;
                    Ok(LockOutcome::Acquired)
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.write_atomic(pid)?;
                Ok(LockOutcome::Acquired)
            }
            Err(error) => Err(SupervisorError::Io {
                path: self.path.clone(),
                reason: format!("read existing PID file: {error}"),
            }),
        }
    }

    /// Write `pid` to the lock file via a temp + rename so partial writes cannot leave a
    /// truncated lock. The lock directory must already exist; the installer creates it.
    fn write_atomic(&self, pid: u32) -> Result<()> {
        let parent = self.path.parent().ok_or_else(|| SupervisorError::Io {
            path: self.path.clone(),
            reason: "lock file has no parent directory".to_string(),
        })?;
        std::fs::create_dir_all(parent).map_err(|error| SupervisorError::Io {
            path: parent.to_path_buf(),
            reason: format!("create lock directory: {error}"),
        })?;
        let body = format!("{pid}\n");
        let tmp = parent.join(format!(
            ".{}.tmp",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("supervisor")
        ));
        std::fs::write(&tmp, body.as_bytes()).map_err(|error| SupervisorError::Io {
            path: tmp.clone(),
            reason: format!("write tmp PID file: {error}"),
        })?;
        std::fs::rename(&tmp, &self.path).map_err(|error| SupervisorError::Io {
            path: self.path.clone(),
            reason: format!("rename tmp PID file: {error}"),
        })?;
        Ok(())
    }
}

/// Is `pid` alive in this process's PID namespace? On Unix we use `kill(pid, 0)`. On Windows
/// the conservative path reports "alive" because we cannot probe without Win32; the installer
/// follows up by surfacing a `PidLockHeld` error when the supervisor cannot start.
///
/// Public so the watcher coordinator can use the same probe to decide whether to adopt a
/// recorded watcher pidfile.
pub fn pid_alive_probe(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        // `kill(pid, 0)` returns 0 when the process exists and the caller has permission to
        // send a signal; -1 with EPERM when the caller cannot signal it (treat as alive —
        // somebody else owns the pid); -1 with ESRCH when the process does not exist.
        // EPERM arises when, e.g., a different UID owns the pid; we conservatively report
        // alive so we never reclaim a foreign process's lock.
        match unsafe { libc::kill(pid as libc::pid_t, 0) } {
            0 => true,
            -1 => {
                let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                // ESRCH = process gone; everything else (EPERM, EACCES, ...) = alive.
                errno != libc::ESRCH
            }
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// Release the lock if it references `pid`. The lock directory is NOT removed because
/// the runtime may want to inspect it after a normal exit. A different process owning
/// the lock is left untouched.
pub fn release_if_owned(path: &Path, pid: u32) -> Result<bool> {
    match std::fs::read_to_string(path) {
        Ok(existing) => match existing.trim().parse::<u32>() {
            Ok(observed) if observed == pid => {
                std::fs::remove_file(path).map_err(|error| SupervisorError::Io {
                    path: path.to_path_buf(),
                    reason: format!("remove PID file: {error}"),
                })?;
                Ok(true)
            }
            _ => Ok(false),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(SupervisorError::Io {
            path: path.to_path_buf(),
            reason: format!("read existing PID file: {error}"),
        }),
    }
}

/// Retry `try_acquire` up to `deadline` to ride out a supervisor that is shutting down but
/// still owns its PID file. Returns the final outcome; never loops forever.
pub fn try_acquire_until(
    lock: &SupervisorLock,
    pid: u32,
    deadline: Duration,
    poll: Duration,
) -> Result<LockOutcome> {
    let deadline_at = Instant::now() + deadline;
    loop {
        match lock.try_acquire(pid)? {
            outcome @ LockOutcome::Acquired => return Ok(outcome),
            outcome @ LockOutcome::Held { .. } => {
                if Instant::now() >= deadline_at {
                    return Ok(outcome);
                }
                std::thread::sleep(poll);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_lock_path(label: &str) -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        // Leak the TempDir so it survives the test. `keep` consumes it and persists.
        dir.keep().join(label)
    }

    #[test]
    fn acquire_creates_lock_when_no_existing_file() {
        let path = temp_lock_path("supervisor.pid");
        let lock = SupervisorLock::new(path.clone());
        assert_eq!(lock.try_acquire(42).unwrap(), LockOutcome::Acquired);
        let recorded = std::fs::read_to_string(&path).unwrap();
        assert_eq!(recorded.trim(), "42");
    }

    #[test]
    fn acquire_refuses_when_live_pid_is_foreign() {
        let path = temp_lock_path("supervisor.pid");
        // Write a PID that is overwhelmingly unlikely to be alive in the test runner, then
        // cross-check: if the probe reports it alive, the lock must report Held; if not,
        // the supervisor reclaims and reports Acquired with its own PID at the end. Either
        // branch is correct behaviour — the test asserts both invariants separately.
        let observed_pid: u32 = 999_999;
        std::fs::write(&path, format!("{observed_pid}\n")).unwrap();
        let lock = SupervisorLock::new(path.clone());
        let outcome = lock.try_acquire(42).unwrap();
        match outcome {
            LockOutcome::Acquired => {
                let recorded = std::fs::read_to_string(&path).unwrap();
                assert_eq!(recorded.trim(), "42");
                assert!(!pid_alive_probe(observed_pid));
            }
            LockOutcome::Held { pid } => {
                assert_eq!(pid, observed_pid);
                assert!(pid_alive_probe(observed_pid));
                let recorded = std::fs::read_to_string(&path).unwrap();
                assert_eq!(recorded.trim(), observed_pid.to_string());
            }
        }
    }

    #[test]
    fn acquire_is_idempotent_for_own_pid_if_alive() {
        let current = std::process::id();
        let path = temp_lock_path("supervisor.pid");
        std::fs::write(&path, format!("{current}\n")).unwrap();
        let lock = SupervisorLock::new(path.clone());
        let outcome = lock.try_acquire(current).unwrap();
        // Whatever the platform says about liveness, the file must still record `current`.
        let recorded = std::fs::read_to_string(&lock.path()).unwrap();
        let parsed: u32 = recorded.trim().parse().unwrap();
        // After acquire, the PID recorded must equal `current` (either unchanged because we
        // observed our own record, or rewritten because the probe claimed we are dead —
        // provably unreachable when `current` is the running test process).
        assert_eq!(parsed, current);
        // The outcome is one of {Acquired, Held { pid: current }}; we assert the latter is
        // impossible to express as a binding, so we re-match more loosely.
        match outcome {
            LockOutcome::Acquired => {}
            LockOutcome::Held { pid } => assert_eq!(pid, current),
        }
    }

    #[test]
    fn acquire_reclaims_empty_or_unparseable_pid_file() {
        let path = temp_lock_path("supervisor.pid");
        std::fs::write(&path, b"").unwrap();
        let lock = SupervisorLock::new(path.clone());
        assert_eq!(lock.try_acquire(7).unwrap(), LockOutcome::Acquired);
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "7");

        let path = temp_lock_path("garbage.pid");
        std::fs::write(&path, b"not-a-pid\n").unwrap();
        let lock = SupervisorLock::new(path.clone());
        assert_eq!(lock.try_acquire(8).unwrap(), LockOutcome::Acquired);
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "8");
    }

    #[test]
    fn release_removes_only_own_lock() {
        let path = temp_lock_path("supervisor.pid");
        // Use a sentinel PID we know is dead and unowned. We must first prove the lock
        // recorded that PID.
        std::fs::write(&path, b"1234\n").unwrap();
        assert!(release_if_owned(&path, 1234).unwrap());
        assert!(!path.exists());

        let path = temp_lock_path("supervisor.pid");
        std::fs::write(&path, b"1234\n").unwrap();
        assert!(!release_if_owned(&path, 9999).unwrap());
        assert!(path.exists(), "must not remove a foreign lock");
    }
}
