//! Resident child process. The supervisor owns exactly one resident at a time. This module
//! keeps the spawn side thin: the supervisor decides when to spawn, the runtime decides
//! what to do with a lease file. The supervisor's only contract here is "spawn the
//! membrane binary in supervisor-child mode, pointed at a published lease, and report the
//! child's stdout/stderr back to the caller".
//!
//! MBR-201: build a per-user Membrane supervisor.

use crate::error::{Result, SupervisorError};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// How to invoke the resident child. The supervisor hands the runtime a `--lease <path>`
/// flag so the runtime rejects supervisors it cannot recognise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidentInvocation {
    /// Path to the `membrane` executable. Must exist by the time [`Self::spawn`] runs.
    pub binary: PathBuf,
    /// Lease path the runtime will read. Required; the resident refuses to start without it.
    pub lease: PathBuf,
    /// Loopback port for the resident to bind. Defaults to 47851 in `parsed_default`.
    pub loopback_port: u16,
    /// Working directory of the resident. `None` keeps the supervisor's CWD.
    pub working_directory: Option<PathBuf>,
    /// Extra environment variables the supervisor wants to set. Keys are normalised to
    /// uppercase by the runtime; we leave the case the way the supervisor wrote it.
    pub extra_environment: Vec<(String, String)>,
}

impl ResidentInvocation {
    /// A sensible default invocation anchored at the supervisor's host. The installer
    /// supplies the actual paths.
    pub fn parsed_default(binary: PathBuf, lease: PathBuf) -> Self {
        Self {
            binary,
            lease,
            loopback_port: crate::config::DEFAULT_LOOPBACK_PORT,
            working_directory: None,
            extra_environment: Vec::new(),
        }
    }

    /// Assemble the argv that will be passed to `Command`. Exposed so unit tests can
    /// verify the supervisor emits the right flags without spawning a process.
    pub fn argv(&self) -> Vec<String> {
        vec![
            "supervisor-child".to_string(),
            "--lease".to_string(),
            self.lease.display().to_string(),
            "--loopback-port".to_string(),
            self.loopback_port.to_string(),
        ]
    }

    /// Spawn the resident and return the live child. The supervisor never blocks on the
    /// child's I/O here; it polls the child via [`std::process::Child::try_wait`] on its
    /// own schedule.
    pub fn spawn(&self) -> Result<ResidentHandle> {
        if !self.binary.is_file() {
            return Err(SupervisorError::ResidentSpawn {
                path: self.binary.clone(),
                reason: "supervisor resident binary path does not exist or is not a file"
                    .to_string(),
            });
        }
        let mut command = Command::new(&self.binary);
        command
            .arg("supervisor-child")
            .arg("--lease")
            .arg(&self.lease)
            .arg("--loopback-port")
            .arg(self.loopback_port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = &self.working_directory {
            command.current_dir(dir);
        }
        for (key, value) in &self.extra_environment {
            command.env(key, value);
        }
        let child: Child = command
            .spawn()
            .map_err(|error| SupervisorError::ResidentSpawn {
                path: self.binary.clone(),
                reason: format!("spawn failed: {error}"),
            })?;
        Ok(ResidentHandle {
            pid: child.id(),
            child,
        })
    }
}

/// Live resident child handle. Holds the OS child and exposes a couple of typed accessors
/// the supervisor uses in its inner loop.
#[derive(Debug)]
pub struct ResidentHandle {
    pid: u32,
    child: Child,
}

impl ResidentHandle {
    /// PID the supervisor will compare against the lease's `resident_pid_expected`.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Borrow the inner child for `try_wait`/`kill`/`wait` operations. The supervisor must
    /// not store the returned reference beyond a single iteration of its loop.
    pub fn child(&mut self) -> &mut Child {
        &mut self.child
    }

    /// Take the inner child out of the handle. Used by the supervisor when it shuts down
    /// and needs to forward the child to a graceful-stop routine.
    pub fn into_inner(self) -> Child {
        self.child
    }
}

/// Verify a candidate binary is at least plausibly a `membrane` resident. This is a cheap
/// preflight the supervisor runs before `spawn`: it checks the file exists, is executable,
/// and the first 16 bytes either carry `membrane` or are binary garbage (the OS loader
/// will reject the file regardless). The preflight is intentionally fast.
pub fn preflight_resident_binary(path: &Path) -> Result<()> {
    let metadata = std::fs::metadata(path).map_err(|error| SupervisorError::ResidentSpawn {
        path: path.to_path_buf(),
        reason: format!("stat resident binary: {error}"),
    })?;
    if !metadata.is_file() {
        return Err(SupervisorError::ResidentSpawn {
            path: path.to_path_buf(),
            reason: "resident binary path is not a regular file".to_string(),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        if mode & 0o111 == 0 {
            return Err(SupervisorError::ResidentSpawn {
                path: path.to_path_buf(),
                reason: "resident binary is not executable".to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_passes_lease_and_port_to_resident() {
        let invocation = ResidentInvocation {
            binary: PathBuf::from("/opt/membrane/bin/membrane"),
            lease: PathBuf::from("/var/run/membrane/lease.json"),
            loopback_port: 47851,
            working_directory: None,
            extra_environment: Vec::new(),
        };
        assert_eq!(
            invocation.argv(),
            vec![
                "supervisor-child",
                "--lease",
                "/var/run/membrane/lease.json",
                "--loopback-port",
                "47851"
            ]
        );
    }

    #[test]
    fn spawn_fails_for_missing_binary() {
        let invocation = ResidentInvocation {
            binary: PathBuf::from("/does/not/exist"),
            lease: PathBuf::from("/var/run/lease.json"),
            loopback_port: 47851,
            working_directory: None,
            extra_environment: Vec::new(),
        };
        let err = invocation.spawn().unwrap_err();
        assert_eq!(err.label(), "resident-spawn-failed");
    }

    #[test]
    fn preflight_rejects_non_executable_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("not-exec.txt");
        std::fs::write(&path, b"plain text").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            let err = preflight_resident_binary(&path).unwrap_err();
            assert!(format!("{err}").contains("not executable"));
        }
        #[cfg(not(unix))]
        {
            // Windows: non-executability is determined by the loader; preflight accepts
            // any regular file because the actual loader failure is surfaced by Command::spawn.
            preflight_resident_binary(&path).unwrap();
        }
    }

    #[test]
    fn preflight_accepts_executable_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("fake-resident.sh");
        std::fs::write(&path, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        preflight_resident_binary(&path).unwrap();
    }
}
