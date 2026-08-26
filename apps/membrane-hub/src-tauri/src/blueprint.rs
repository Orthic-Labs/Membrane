//! Hub-owned lifecycle for Blueprint installed with Membrane Hub.
//!
//! Blueprint stays a separately bounded installed component: Hub only enrolls
//! active workspace, starts `service run` in foreground mode, exports one
//! canonical named-pipe endpoint, and drains complete child tree on shutdown.
//! Runtime-root overrides are retained as a compatibility name only; bundled
//! Hub launches always resolve its resource-owned runtime.

use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{mpsc, Mutex},
    thread,
    time::Duration,
};

pub const RUNTIME_ROOT_ENV: &str = "BLUEPRINT_RUNTIME_ROOT";
pub const DAEMON_ENDPOINT_ENV: &str = "BLUEPRINT_DAEMON_ENDPOINT";
pub const SERVICE_CHILD_ENV: &str = "MEMBRANE_HUB_CHILD";
pub const SERVICE_PARENT_PID_ENV: &str = "MEMBRANE_HUB_PARENT_PID";
pub const SERVICE_LAUNCH_TOKEN_ENV: &str = "MEMBRANE_HUB_LAUNCH_TOKEN";
// Service run publishes its resident listener before cold repository
// reconciliation completes. Keep startup bounded for broken children while
// allowing process/IPC setup to cross slow native-host scheduling windows.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(7);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Running,
    Unavailable,
}

impl ServiceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Typed Blueprint lifecycle outcomes. `ServiceStatus` stays deliberately
/// small for supervisor control flow; these states are the public diagnosis
/// vocabulary used when Hub reports why Blueprint is not serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    Running,
    NotConfigured,
    Stale,
    TransportUnavailable,
    HubInactive,
    ResidentOwnerActive,
}

impl LifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::NotConfigured => "not_configured",
            Self::Stale => "stale",
            Self::TransportUnavailable => "transport_unavailable",
            Self::HubInactive => "hub_inactive",
            Self::ResidentOwnerActive => "resident_owner_active",
        }
    }
}

/// Convert process/transport failures into stable lifecycle state without
/// making callers inspect free-form child stderr.
pub fn lifecycle_state_for_error(error: &str) -> LifecycleState {
    let value = error.to_ascii_lowercase();
    if value.contains("hub_inactive") || value.contains("membrane_not_running") {
        return LifecycleState::HubInactive;
    }
    if value.contains("resident_owner_active") {
        return LifecycleState::ResidentOwnerActive;
    }
    if value.contains("runtime_root")
        || value.contains("runtime_layout")
        || value.contains("not_configured")
    {
        return LifecycleState::NotConfigured;
    }
    if value.contains("stale") {
        return LifecycleState::Stale;
    }
    LifecycleState::TransportUnavailable
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLayout {
    pub root: PathBuf,
    pub node: PathBuf,
    pub package: PathBuf,
    pub cli: PathBuf,
    pub watcher: PathBuf,
}

impl RuntimeLayout {
    pub fn from_root(root: PathBuf) -> Result<Self, String> {
        let root =
            fs::canonicalize(root).map_err(|_| "blueprint_runtime_root_invalid".to_string())?;
        // Blueprint remains an explicit runtime boundary inside installation.
        let node_name = if cfg!(windows) { "node.exe" } else { "node" };
        let node = root.join("lib").join(node_name);
        let package = root.join("app").join("package");
        let cli = package.join("scripts").join("blueprint.mjs");
        let watcher = package.join("scripts").join("blueprint-watch.mjs");
        if !root.is_dir()
            || !node.is_file()
            || !package.is_dir()
            || !cli.is_file()
            || !watcher.is_file()
        {
            return Err("blueprint_runtime_layout_invalid".into());
        }
        Ok(Self {
            root,
            node,
            package,
            cli,
            watcher,
        })
    }

    pub fn installed(root: &Path) -> Result<Self, String> {
        // Production Hub must use its Tauri resource directory. Never let an
        // inherited development/runtime override replace bundled Blueprint.
        Self::from_root(root.to_path_buf())
    }
}

/// Canonical Hub endpoint. Blueprint is qualified on Windows named pipes;
/// keeping one stable name also lets native consumers inherit it from
/// the Hub process without reading Blueprint installation details.
pub fn canonical_endpoint() -> Result<String, String> {
    if cfg!(windows) {
        let profile = env::var("USERPROFILE")
            .map_err(|_| "blueprint_user_profile_unavailable".to_string())?;
        let suffix = hex::encode(Sha256::digest(profile.as_bytes()));
        Ok(format!(r"\\.\pipe\membrane-blueprint-{}", &suffix[..16]))
    } else {
        let home = env::var("HOME").map_err(|_| "blueprint_home_unavailable".to_string())?;
        Ok(PathBuf::from(home)
            .join(".blueprint")
            .join("blueprint.sock")
            .to_string_lossy()
            .into_owned())
    }
}

/// Mint one high-entropy capability per Hub-owned child launch. The child
/// receives this through its private stdin handshake as well as its inherited
/// environment, so an env-only spoof cannot authorize residency.
pub fn launch_token() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    fill_launch_random(&mut bytes)
        .map_err(|_| "blueprint_launch_random_unavailable".to_string())?;
    Ok(hex::encode(bytes))
}

#[cfg(unix)]
fn fill_launch_random(bytes: &mut [u8]) -> std::io::Result<()> {
    fs::File::open("/dev/urandom")?.read_exact(bytes)
}

#[cfg(windows)]
fn fill_launch_random(bytes: &mut [u8]) -> std::io::Result<()> {
    use std::{ffi::c_void, ptr::null_mut};

    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(
            h_algorithm: *mut c_void,
            buffer: *mut u8,
            length: u32,
            flags: u32,
        ) -> i32;
    }

    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;
    let length = u32::try_from(bytes.len()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "launch token too large")
    })?;
    let status = unsafe {
        BCryptGenRandom(
            null_mut(),
            bytes.as_mut_ptr(),
            length,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status))
    }
}

#[cfg(not(any(unix, windows)))]
fn fill_launch_random(_bytes: &mut [u8]) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "OS randomness unavailable",
    ))
}

struct State {
    child: Option<Child>,
}

/// One Hub-owned Blueprint service. This object deliberately does not own
/// Blueprint data or provide a second protocol implementation.
pub struct Supervisor {
    workspace_root: PathBuf,
    runtime_root: PathBuf,
    state: Mutex<State>,
}

impl Supervisor {
    pub fn new(workspace_root: PathBuf, runtime_root: PathBuf) -> Self {
        Self {
            workspace_root,
            runtime_root,
            state: Mutex::new(State { child: None }),
        }
    }

    pub fn start(&self) -> Result<ServiceStatus, String> {
        let layout = RuntimeLayout::installed(&self.runtime_root)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "blueprint_service_state_unavailable".to_string())?;
        if let Some(child) = state.child.as_mut() {
            if child
                .try_wait()
                .map_err(|_| "blueprint_service_probe_failed".to_string())?
                .is_none()
            {
                return Ok(ServiceStatus::Running);
            }
            state.child = None;
        }

        enroll_workspace(&layout, &self.workspace_root)?;
        let endpoint = canonical_endpoint()?;
        let launch_token = launch_token()?;
        let mut command = Command::new(&layout.node);
        command
            .arg(&layout.cli)
            .arg("service")
            .arg("run")
            .arg("--root")
            .arg(&self.workspace_root)
            .current_dir(&layout.package)
            .env(DAEMON_ENDPOINT_ENV, &endpoint)
            .env(SERVICE_CHILD_ENV, "1")
            .env(SERVICE_PARENT_PID_ENV, std::process::id().to_string())
            .env(SERVICE_LAUNCH_TOKEN_ENV, &launch_token)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|_| "blueprint_service_spawn_failed".to_string())?;
        let Some(stdin) = child.stdin.as_mut() else {
            kill_tree(&mut child);
            return Err("blueprint_service_stdin_unavailable".into());
        };
        stdin
            .write_all(format!("{launch_token}\n").as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|_| {
                kill_tree(&mut child);
                "blueprint_service_launch_handshake_failed".to_string()
            })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "blueprint_service_stdout_unavailable".to_string())?;
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut lines = BufReader::new(stdout).lines();
            let line = lines.next().transpose().unwrap_or(None);
            let _ = sender.send(line);
            // Keep stdout drained so a verbose Blueprint process cannot block.
            for _ in lines {}
        });
        match receiver.recv_timeout(STARTUP_TIMEOUT) {
            Ok(Some(line))
                if line.contains("\"state\":\"running\"")
                    || line.contains("\"state\": \"running\"") =>
            {
                state.child = Some(child);
                Ok(ServiceStatus::Running)
            }
            Ok(Some(line)) => {
                kill_tree(&mut child);
                Err(startup_failure(&line))
            }
            Ok(None) | Err(_) => {
                kill_tree(&mut child);
                Err("blueprint_service_startup_failed".into())
            }
        }
    }

    pub fn supervise(&self) -> ServiceStatus {
        let Ok(mut state) = self.state.lock() else {
            return ServiceStatus::Unavailable;
        };
        match state.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(None) => ServiceStatus::Running,
                Ok(Some(_)) | Err(_) => {
                    state.child = None;
                    ServiceStatus::Unavailable
                }
            },
            None => ServiceStatus::Unavailable,
        }
    }

    pub fn stop(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(mut child) = state.child.take() else {
            return;
        };
        kill_tree(&mut child);
    }
}

fn startup_failure(line: &str) -> String {
    let line = line.to_ascii_lowercase();
    for code in [
        "resident_owner_active",
        "hub_inactive",
        "not_configured",
        "stale",
        "transport_unavailable",
    ] {
        if line.contains(code) {
            return code.into();
        }
    }
    "blueprint_service_startup_failed".into()
}

fn enroll_workspace(layout: &RuntimeLayout, workspace_root: &Path) -> Result<(), String> {
    let status = Command::new(&layout.node)
        .arg(&layout.watcher)
        .arg("enroll")
        .arg(workspace_root)
        .current_dir(&layout.package)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "blueprint_enrollment_spawn_failed".to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("blueprint_enrollment_failed".into())
    }
}

fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = child.kill();
    }
    let deadline = std::time::Instant::now() + DRAIN_TIMEOUT;
    while std::time::Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(_) => return,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_layout_requires_node_and_package_tree() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            RuntimeLayout::from_root(root.path().into()),
            Err("blueprint_runtime_root_invalid".into())
        );
        fs::create_dir_all(root.path().join("lib")).unwrap();
        fs::create_dir_all(root.path().join("app/package/scripts")).unwrap();
        let node_name = if cfg!(windows) { "node.exe" } else { "node" };
        fs::write(root.path().join("lib").join(node_name), b"").unwrap();
        fs::write(root.path().join("app/package/scripts/blueprint.mjs"), b"").unwrap();
        fs::write(
            root.path().join("app/package/scripts/blueprint-watch.mjs"),
            b"",
        )
        .unwrap();
        assert!(RuntimeLayout::from_root(root.path().into()).is_ok());
    }

    #[test]
    fn missing_installed_runtime_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let supervisor = Supervisor::new(PathBuf::from("."), root.path().join("missing"));
        let prior = env::var_os(RUNTIME_ROOT_ENV);
        env::remove_var(RUNTIME_ROOT_ENV);
        assert_eq!(
            supervisor.start(),
            Err("blueprint_runtime_root_invalid".into())
        );
        if let Some(value) = prior {
            env::set_var(RUNTIME_ROOT_ENV, value);
        }
    }

    #[test]
    fn canonical_endpoint_is_stable() {
        let prior = env::var_os("USERPROFILE");
        env::set_var("USERPROFILE", r"C:\Users\membrane-test");
        let endpoint = canonical_endpoint().unwrap();
        assert!(endpoint.starts_with(r"\\.\pipe\membrane-blueprint-"));
        assert_eq!(endpoint.len(), r"\\.\pipe\membrane-blueprint-".len() + 16);
        if let Some(value) = prior {
            env::set_var("USERPROFILE", value);
        } else {
            env::remove_var("USERPROFILE");
        }
    }

    #[test]
    fn lifecycle_errors_preserve_typed_degradation() {
        assert_eq!(
            lifecycle_state_for_error("blueprint_runtime_root_invalid"),
            LifecycleState::NotConfigured
        );
        assert_eq!(
            lifecycle_state_for_error("resident_owner_active"),
            LifecycleState::ResidentOwnerActive
        );
        assert_eq!(
            lifecycle_state_for_error("stale_blocked"),
            LifecycleState::Stale
        );
        assert_eq!(
            lifecycle_state_for_error("blueprint_service_spawn_failed"),
            LifecycleState::TransportUnavailable
        );
        assert_eq!(
            lifecycle_state_for_error("membrane_not_running"),
            LifecycleState::HubInactive
        );
        assert_eq!(
            startup_failure(r#"{"error":{"code":"resident_owner_active"}}"#),
            "resident_owner_active"
        );
    }
}
