//! Installed-app activation plus native harness enrollment.
//!
//! This module is a mechanical Rust port of already-landed mechanisms:
//! - GitNexus `setup.ts`: probe a directly spawnable client CLI, then register
//!   one absolute executable command;
//! - MemOS `daemon_manager.py`: cross-process startup lock, identity-bearing
//!   health probe, bounded readiness wait, & conservative foreign-owner refusal;
//! - MemOS `install.ps1`: restore prior bindings when any later binding fails;
//! - CodeGraph `fetch-engine.js`: stage complete output before atomic promotion;
//! - OpenViking `openviking-entrypoint.sh`: start, wait for health, fail when
//!   child exits early or readiness deadline expires.
//! Existing `mcp/install.mjs::createNativeInstaller` supplies exact Membrane
//! add/get/remove command shapes & conflict-restoration contract.

use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const ACTIVATION_RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const DEACTIVATION_RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const ACTIVATION_RECEIPT_FILE: &str = "activation-receipt.json";
pub const INSTALLED_PORT: u16 = 47_851;
const LOCK_DIR: &str = ".activation.lock";
const LOCK_STALE_AFTER: Duration = Duration::from_secs(90);
const LOCK_WAIT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const MAX_HEALTH_BYTES: usize = 2 * 1024 * 1024;
const SERVICE_ID: &str = "membrane-hub";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessClient {
    Codex,
    Claude,
}

impl HarnessClient {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            _ => Err(format!(
                "unsupported harness `{value}`; expected codex or claude"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }

    fn binary_env(self) -> &'static str {
        match self {
            Self::Codex => "MEMBRANE_CODEX_BIN",
            Self::Claude => "MEMBRANE_CLAUDE_BIN",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActivationOptions {
    pub install_root: PathBuf,
    pub clients: Vec<HarnessClient>,
    pub timeout: Duration,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeOrigin {
    Installed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ClientActivationReceipt {
    pub client: HarnessClient,
    pub before: String,
    pub after: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceActivationReceipt {
    pub service_id: String,
    pub port: u16,
    pub release_generation: String,
    pub already_running: bool,
    /// Additive readiness projection. `ready` is the only state that permits
    /// activation to claim an exact resident generation.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationReceiptV1 {
    pub schema_version: u32,
    pub runtime_origin: RuntimeOrigin,
    pub install_root: PathBuf,
    pub version_root: PathBuf,
    pub membrane_executable: PathBuf,
    pub tray_executable: PathBuf,
    pub activated_at_unix_ms: u64,
    pub dry_run: bool,
    pub service: ServiceActivationReceipt,
    pub clients: Vec<ClientActivationReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDeactivationReceipt {
    pub service_id: String,
    pub port: u16,
    pub before: String,
    pub after: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeactivationReceiptV1 {
    pub schema_version: u32,
    pub runtime_origin: RuntimeOrigin,
    pub install_root: PathBuf,
    pub version_root: PathBuf,
    pub membrane_executable: PathBuf,
    pub tray_executable: PathBuf,
    pub deactivated_at_unix_ms: u64,
    pub dry_run: bool,
    pub service: ServiceDeactivationReceipt,
    pub clients: Vec<ClientActivationReceipt>,
    pub claude_hooks_matched: usize,
    pub claude_hooks_removed: usize,
    pub user_path_present: bool,
    pub user_path_removed: bool,
    pub startup_entries_matched: usize,
    pub startup_entries_removed: usize,
    pub activation_receipt_matched: bool,
    pub activation_receipt_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServerConfig {
    command: String,
    args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClientState {
    NotInstalled,
    Absent,
    AlreadyCorrect,
    Conflict(ServerConfig),
}

impl ClientState {
    fn label(&self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::Absent => "absent",
            Self::AlreadyCorrect => "already_correct",
            Self::Conflict(_) => "conflict",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandResult {
    code: i32,
    stdout: String,
    stderr: String,
}

impl CommandResult {
    fn success(&self) -> bool {
        self.code == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HealthObservation {
    Unavailable,
    NotReady,
    Ready { release_generation: String },
    PriorGeneration { release_generation: String },
    Foreign(String),
}

struct ActivationLock {
    path: PathBuf,
}

impl Drop for ActivationLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.path.join("owner"));
        let _ = std::fs::remove_dir(&self.path);
    }
}

pub fn default_install_root() -> Result<PathBuf, String> {
    expected_stable_install_root()
}

pub fn activate(options: ActivationOptions) -> Result<ActivationReceiptV1, String> {
    let (install_root, version_root) = validate_installed_root(&options.install_root)?;
    let product_root = install_root
        .parent()
        .ok_or_else(|| "stable installed path has no product root".to_string())?;
    let membrane = install_root.join(executable_name("membrane"));
    let tray = install_root.join(executable_name("membrane-tray"));
    let runtime_membrane = version_root.join(executable_name("membrane"));
    let runtime_tray = version_root.join(executable_name("membrane-tray"));
    require_file(&runtime_membrane, "resolved membrane executable")?;
    require_file(&runtime_tray, "resolved tray executable")?;
    let (workspace_root, port) = installed_runtime(product_root)?;
    let expected_generation = membrane_runtime::release_identity::release_generation();
    if !options.dry_run {
        std::fs::create_dir_all(&workspace_root).map_err(|error| {
            format!(
                "create installed runtime state {}: {error}",
                workspace_root.display()
            )
        })?;
    }
    // Inspection must be entirely non-mutating, including lock acquisition.
    let _lock = (!options.dry_run)
        .then(|| acquire_lock(product_root))
        .transpose()?;

    // A dry-run is an inspection receipt even when no resident exists yet. A
    // malformed or foreign listener is recorded in `state`/`reason` rather
    // than preventing callers from receiving usable JSON.
    let initial = match probe_health(port, &expected_generation) {
        Ok(observation) => observation,
        Err(error) => HealthObservation::Foreign(error),
    };
    let already_running = matches!(&initial, HealthObservation::Ready { .. });
    let (release_generation, service_state, service_reason) = if options.dry_run {
        match initial {
            HealthObservation::Ready { release_generation } => {
                (release_generation, "ready".to_string(), None)
            }
            HealthObservation::PriorGeneration { release_generation } => (
                release_generation,
                "stale_generation".to_string(),
                Some("resident release generation differs from installed current".to_string()),
            ),
            HealthObservation::Unavailable => (
                expected_generation.clone(),
                "unavailable".to_string(),
                Some("installed Membrane is not running".to_string()),
            ),
            HealthObservation::NotReady => (
                expected_generation.clone(),
                "not_ready".to_string(),
                Some("installed Membrane is not healthy".to_string()),
            ),
            HealthObservation::Foreign(reason) => {
                (expected_generation.clone(), "foreign".to_string(), Some(reason))
            }
        }
    } else if let HealthObservation::Ready { release_generation } = &initial {
        (release_generation.clone(), "ready".to_string(), None)
    } else {
        if matches!(&initial, HealthObservation::PriorGeneration { .. }) {
            request_resident_replacement(&tray, &workspace_root, port)?;
            wait_for_shutdown(port, &expected_generation, options.timeout)?;
        }
        launch_tray(&tray, &workspace_root, port)?;
        (
            wait_for_health(port, &expected_generation, options.timeout)?,
            "ready".to_string(),
            None,
        )
    };

    let clients = reconcile_clients(&membrane, &options.clients, options.dry_run, run_client)?;
    if !options.dry_run {
        ensure_user_path(&install_root)?;
        reconcile_claude_hooks(&install_root)?;
    }
    let receipt = ActivationReceiptV1 {
        schema_version: ACTIVATION_RECEIPT_SCHEMA_VERSION,
        runtime_origin: RuntimeOrigin::Installed,
        install_root: install_root.clone(),
        version_root,
        membrane_executable: membrane,
        tray_executable: tray,
        activated_at_unix_ms: now_unix_ms(),
        dry_run: options.dry_run,
        service: ServiceActivationReceipt {
            service_id: SERVICE_ID.to_string(),
            port,
            release_generation,
            already_running,
            state: service_state,
            reason: service_reason,
        },
        clients,
    };
    if !options.dry_run {
        persist_receipt(&workspace_root, &receipt)?;
    }
    Ok(receipt)
}

pub fn deactivate(options: ActivationOptions) -> Result<DeactivationReceiptV1, String> {
    let (install_root, version_root) = validate_installed_root(&options.install_root)?;
    let product_root = install_root
        .parent()
        .ok_or_else(|| "stable installed path has no product root".to_string())?;
    let membrane = install_root.join(executable_name("membrane"));
    let tray = install_root.join(executable_name("membrane-tray"));
    require_file(
        &version_root.join(executable_name("membrane")),
        "resolved membrane executable",
    )?;
    require_file(
        &version_root.join(executable_name("membrane-tray")),
        "resolved tray executable",
    )?;
    let (workspace_root, port) = installed_runtime(product_root)?;
    let expected_generation = membrane_runtime::release_identity::release_generation();
    let initial = match probe_health(port, &expected_generation) {
        Ok(observation) => observation,
        Err(error) => HealthObservation::Foreign(error),
    };
    let before = health_label(&initial).to_string();
    if !options.dry_run {
        if let HealthObservation::Foreign(reason) = &initial {
            return Err(format!(
                "refusing to deactivate unverified service on Membrane port: {reason}"
            ));
        }
    }
    let would_stop = matches!(
        &initial,
        HealthObservation::NotReady
            | HealthObservation::Ready { .. }
            | HealthObservation::PriorGeneration { .. }
    );

    let _lock = (!options.dry_run)
        .then(|| acquire_lock(product_root))
        .transpose()?;
    if !options.dry_run && would_stop {
        request_resident_replacement(&tray, &workspace_root, port)?;
        wait_for_shutdown(port, &expected_generation, options.timeout)?;
    }

    let clients = deactivate_clients(&membrane, &options.clients, options.dry_run, run_client)?;
    let claude_hooks_matched = remove_claude_hooks(&install_root, options.dry_run)?;
    let user_path_present = remove_user_path(&install_root, options.dry_run)?;
    let startup_entries_matched = remove_startup_entries(&tray, options.dry_run)?;
    let activation_receipt_matched = remove_activation_receipt(
        &workspace_root,
        &install_root,
        &version_root,
        &membrane,
        &tray,
        options.dry_run,
    )?;
    Ok(DeactivationReceiptV1 {
        schema_version: DEACTIVATION_RECEIPT_SCHEMA_VERSION,
        runtime_origin: RuntimeOrigin::Installed,
        install_root,
        version_root,
        membrane_executable: membrane,
        tray_executable: tray,
        deactivated_at_unix_ms: now_unix_ms(),
        dry_run: options.dry_run,
        service: ServiceDeactivationReceipt {
            service_id: SERVICE_ID.to_string(),
            port,
            before: before.clone(),
            after: if options.dry_run {
                if would_stop {
                    "would_stop".to_string()
                } else {
                    before.clone()
                }
            } else {
                "unavailable".to_string()
            },
            changed: !options.dry_run && would_stop,
        },
        clients,
        claude_hooks_matched,
        claude_hooks_removed: if options.dry_run {
            0
        } else {
            claude_hooks_matched
        },
        user_path_present,
        user_path_removed: user_path_present && !options.dry_run,
        startup_entries_matched,
        startup_entries_removed: if options.dry_run {
            0
        } else {
            startup_entries_matched
        },
        activation_receipt_matched,
        activation_receipt_removed: activation_receipt_matched && !options.dry_run,
    })
}

fn health_label(observation: &HealthObservation) -> &'static str {
    match observation {
        HealthObservation::Unavailable => "unavailable",
        HealthObservation::NotReady => "not_ready",
        HealthObservation::Ready { .. } => "ready",
        HealthObservation::PriorGeneration { .. } => "stale_generation",
        HealthObservation::Foreign(_) => "foreign",
    }
}

fn expected_stable_install_root() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA is unavailable".to_string())?;
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library/Application Support"))
        .ok_or_else(|| "HOME is unavailable".to_string())?;
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local/share"))
        })
        .ok_or_else(|| "user data root is unavailable".to_string())?;
    Ok(base.join("Orthic Labs").join("Membrane").join("current"))
}

fn validate_installed_root(requested: &Path) -> Result<(PathBuf, PathBuf), String> {
    if requested.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    }) {
        return Err("activation install root must be exact stable current path".to_string());
    }
    let requested = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("resolve activation working directory: {error}"))?
            .join(requested)
    };
    let stable = expected_stable_install_root()?;
    if !paths_equal(&requested.to_string_lossy(), &stable.to_string_lossy()) {
        return Err(format!(
            "activation install root must be stable installed path {}; repository, dist, target, node_modules, and version-specific roots are prohibited",
            stable.display()
        ));
    }
    let pointer_target = std::fs::read_link(&stable).map_err(|error| {
        format!(
            "stable installed path {} is not a readable version pointer: {error}",
            stable.display()
        )
    })?;
    let pointer_target = if pointer_target.is_absolute() {
        pointer_target
    } else {
        stable
            .parent()
            .map(|root| root.join(pointer_target))
            .ok_or_else(|| "stable installed path has no product root".to_string())?
    };
    let version_root = std::fs::canonicalize(&pointer_target).map_err(|error| {
        format!(
            "installed version target {} is unavailable: {error}",
            pointer_target.display()
        )
    })?;
    let versions = stable
        .parent()
        .map(|root| root.join("versions"))
        .ok_or_else(|| "stable installed path has no product root".to_string())?;
    let versions = std::fs::canonicalize(&versions).map_err(|error| {
        format!(
            "installed versions root {} is unavailable: {error}",
            versions.display()
        )
    })?;
    let parent = version_root
        .parent()
        .ok_or_else(|| "stable current target has no versions parent".to_string())?;
    if !paths_equal(&parent.to_string_lossy(), &versions.to_string_lossy()) {
        return Err("stable current path does not target one direct installed version".to_string());
    }
    Ok((stable, version_root))
}

fn require_current_health(
    observation: HealthObservation,
    expected_generation: &str,
) -> Result<String, String> {
    match observation {
        HealthObservation::Ready { release_generation } => Ok(release_generation),
        HealthObservation::PriorGeneration { release_generation } => Err(format!(
            "resident release generation {release_generation} does not match installed generation {expected_generation}"
        )),
        HealthObservation::Foreign(reason) => Err(reason),
        HealthObservation::Unavailable => Err("installed Membrane is not running".to_string()),
        HealthObservation::NotReady => Err("installed Membrane is not healthy".to_string()),
    }
}

fn executable_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

fn require_file(path: &Path, label: &str) -> Result<(), String> {
    path.is_file()
        .then_some(())
        .ok_or_else(|| format!("{label} missing at {}", path.display()))
}

fn acquire_lock(install_root: &Path) -> Result<ActivationLock, String> {
    let path = install_root.join(LOCK_DIR);
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match std::fs::create_dir(&path) {
            Ok(()) => {
                let owner = path.join("owner");
                let _ = std::fs::write(owner, format!("{}\n", std::process::id()));
                return Ok(ActivationLock { path });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = std::fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| modified.elapsed().ok())
                    .is_some_and(|age| age > LOCK_STALE_AFTER);
                if stale {
                    let _ = std::fs::remove_dir_all(&path);
                    continue;
                }
                if Instant::now() >= deadline {
                    return Err("activation startup lock remained busy".to_string());
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(error) => return Err(format!("create activation startup lock: {error}")),
        }
    }
}

fn launch_tray(tray: &Path, workspace_root: &Path, port: u16) -> Result<(), String> {
    launch_tray_with_mode(tray, workspace_root, port, "--activate")
}

fn request_resident_replacement(
    tray: &Path,
    workspace_root: &Path,
    port: u16,
) -> Result<(), String> {
    launch_tray_with_mode(tray, workspace_root, port, "--replace")
}

fn launch_tray_with_mode(
    tray: &Path,
    workspace_root: &Path,
    port: u16,
    mode: &str,
) -> Result<(), String> {
    let mut command = Command::new(tray);
    command
        .arg(mode)
        .env("MEMBRANE_RUNTIME_ORIGIN", "installed")
        .env_remove("MEMBRANE_CONFIG_ROOT")
        .env_remove("MEMBRANE_DATA_ROOT")
        .env_remove("MEMBRANE_CACHE_ROOT")
        .env_remove("MEMBRANE_LOG_ROOT")
        .env("MEMBRANE_STATE_ROOT", workspace_root)
        .env("MEMBRANE_PORT", port.to_string())
        .env("MEMBRANE_HTTP_PORT", port.to_string());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("launch installed tray {}: {error}", tray.display()))
}

fn wait_for_shutdown(
    port: u16,
    expected_generation: &str,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match probe_health(port, expected_generation)? {
            HealthObservation::Unavailable => return Ok(()),
            HealthObservation::Foreign(reason) => return Err(reason),
            HealthObservation::NotReady
            | HealthObservation::Ready { .. }
            | HealthObservation::PriorGeneration { .. } => {}
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "prior Membrane generation did not stop within {}ms",
                timeout.as_millis()
            ));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_health(
    port: u16,
    expected_generation: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match probe_health(port, expected_generation)? {
            HealthObservation::Ready { release_generation } => return Ok(release_generation),
            HealthObservation::Foreign(reason) => return Err(reason),
            HealthObservation::Unavailable
            | HealthObservation::NotReady
            | HealthObservation::PriorGeneration { .. } => {}
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "installed Membrane did not become healthy within {}ms",
                timeout.as_millis()
            ));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn probe_health(port: u16, expected_generation: &str) -> Result<HealthObservation, String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = match TcpStream::connect_timeout(&address, Duration::from_millis(400)) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::NotConnected
            ) =>
        {
            return Ok(HealthObservation::Unavailable)
        }
        Err(error) => return Err(format!("probe installed service: {error}")),
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| format!("set activation health read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| format!("set activation health write timeout: {error}"))?;
    stream
        .write_all(
            format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .map_err(|error| format!("write activation health request: {error}"))?;
    let mut raw = Vec::new();
    stream
        .take((MAX_HEALTH_BYTES + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|error| format!("read activation health response: {error}"))?;
    if raw.len() > MAX_HEALTH_BYTES {
        return Ok(HealthObservation::Foreign(
            "service on Membrane port returned oversized health response".to_string(),
        ));
    }
    parse_health_response(&raw, expected_generation)
}

fn parse_health_response(
    raw: &[u8],
    expected_generation: &str,
) -> Result<HealthObservation, String> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .or_else(|| {
            raw.windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| index + 2)
        })
        .ok_or_else(|| "service on Membrane port returned malformed HTTP".to_string())?;
    let header = std::str::from_utf8(&raw[..split])
        .map_err(|_| "service on Membrane port returned non-UTF8 HTTP header".to_string())?;
    let status = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "service on Membrane port returned malformed HTTP status".to_string())?;
    let body: serde_json::Value = match serde_json::from_slice(&raw[split..]) {
        Ok(value) => value,
        Err(_) => {
            return Ok(HealthObservation::Foreign(
                "service on Membrane port returned non-Membrane health JSON".to_string(),
            ))
        }
    };
    if body.get("serviceId").and_then(serde_json::Value::as_str) != Some(SERVICE_ID)
        || body.get("nativeOnly").and_then(serde_json::Value::as_bool) != Some(true)
    {
        return Ok(HealthObservation::Foreign(
            "service on Membrane port has foreign identity".to_string(),
        ));
    }
    let Some(release_generation) = body
        .get("releaseGeneration")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Ok(HealthObservation::Foreign(
            "Membrane health omitted release generation".to_string(),
        ));
    };
    let runtime_origin = body
        .get("runtimeOrigin")
        .and_then(serde_json::Value::as_str);
    if runtime_origin == Some("development") {
        return Ok(HealthObservation::Foreign(
            "service on Membrane port is a development runtime".to_string(),
        ));
    }
    if release_generation != expected_generation {
        return Ok(HealthObservation::PriorGeneration {
            release_generation: release_generation.to_string(),
        });
    }
    if runtime_origin != Some("installed") {
        return Ok(HealthObservation::Foreign(
            "Membrane health omitted installed runtime origin".to_string(),
        ));
    }
    if status != 200 || body.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Ok(HealthObservation::NotReady);
    }
    Ok(HealthObservation::Ready {
        release_generation: release_generation.to_string(),
    })
}

fn installed_runtime(product_root: &Path) -> Result<(PathBuf, u16), String> {
    if std::env::var("MEMBRANE_RUNTIME_ORIGIN").ok().as_deref() == Some("development") {
        return Err("development runtime cannot perform installed activation".to_string());
    }
    // Installed state is product-owned and deliberately independent from any
    // checkout, workspace config, or repository runtime manifest.
    Ok((product_root.join("state"), INSTALLED_PORT))
}

#[cfg(windows)]
fn ensure_user_path(install_root: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
    };
    let wide = |value: &std::ffi::OsStr| value.encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let key_name = wide(std::ffi::OsStr::new("Environment"));
    let value_name = wide(std::ffi::OsStr::new("Path"));
    let mut key: HKEY = std::ptr::null_mut();
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key_name.as_ptr(),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut key,
        )
    };
    if opened != 0 {
        return Err(format!("open user PATH registry key: {opened}"));
    }
    let mut kind: REG_VALUE_TYPE = REG_EXPAND_SZ;
    let mut byte_len = 0_u32;
    let queried = unsafe {
        RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut byte_len,
        )
    };
    let mut current = String::new();
    if queried == 0 && byte_len > 0 {
        let mut buffer = vec![0_u16; (byte_len as usize + 1) / 2];
        let read = unsafe {
            RegQueryValueExW(
                key,
                value_name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buffer.as_mut_ptr() as *mut u8,
                &mut byte_len,
            )
        };
        if read != 0 {
            unsafe { RegCloseKey(key) };
            return Err(format!("read user PATH: {read}"));
        }
        let end = buffer.iter().position(|value| *value == 0).unwrap_or(buffer.len());
        current = String::from_utf16_lossy(&buffer[..end]);
    } else if queried != 0 && queried != 2 {
        unsafe { RegCloseKey(key) };
        return Err(format!("query user PATH: {queried}"));
    }
    let stable = install_root.to_string_lossy().trim_end_matches(['\\', '/']).to_string();
    let legacy = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|root| root.join("Membrane Hub").to_string_lossy().to_string());
    let mut entries = current
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| {
            !legacy.as_ref().is_some_and(|legacy| value.trim_matches('"').eq_ignore_ascii_case(legacy))
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !entries.iter().any(|value| value.trim_matches('"').eq_ignore_ascii_case(&stable)) {
        entries.push(stable);
    }
    let updated = entries.join(";");
    if updated != current {
        let encoded = wide(std::ffi::OsStr::new(&updated));
        let value_kind = if kind == REG_SZ { REG_SZ } else { REG_EXPAND_SZ };
        let written = unsafe {
            RegSetValueExW(
                key,
                value_name.as_ptr(),
                0,
                value_kind,
                encoded.as_ptr() as *const u8,
                (encoded.len() * 2) as u32,
            )
        };
        if written != 0 {
            unsafe { RegCloseKey(key) };
            return Err(format!("write user PATH: {written}"));
        }
    }
    unsafe { RegCloseKey(key) };
    Ok(())
}

#[cfg(not(windows))]
fn ensure_user_path(_install_root: &Path) -> Result<(), String> {
    Ok(())
}

fn without_path_entry(current: &str, install_root: &Path) -> (String, bool) {
    let stable = install_root
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_string();
    let mut removed = false;
    let entries = current
        .split(';')
        .filter(|value| {
            let owned = paths_equal(value.trim().trim_matches('"'), &stable);
            removed |= owned;
            !owned
        })
        .collect::<Vec<_>>();
    (entries.join(";"), removed)
}

#[cfg(windows)]
fn remove_user_path(install_root: &Path, dry_run: bool) -> Result<bool, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
    };
    let wide = |value: &std::ffi::OsStr| value.encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let key_name = wide(std::ffi::OsStr::new("Environment"));
    let value_name = wide(std::ffi::OsStr::new("Path"));
    let mut key: HKEY = std::ptr::null_mut();
    let access = KEY_QUERY_VALUE | if dry_run { 0 } else { KEY_SET_VALUE };
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key_name.as_ptr(),
            0,
            access,
            &mut key,
        )
    };
    if opened != 0 {
        return Err(format!("open user PATH registry key: {opened}"));
    }
    let mut kind: REG_VALUE_TYPE = REG_EXPAND_SZ;
    let mut byte_len = 0_u32;
    let queried = unsafe {
        RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut byte_len,
        )
    };
    if queried == 2 {
        unsafe { RegCloseKey(key) };
        return Ok(false);
    }
    if queried != 0 {
        unsafe { RegCloseKey(key) };
        return Err(format!("query user PATH: {queried}"));
    }
    let mut buffer = vec![0_u16; (byte_len as usize + 1) / 2];
    let read = unsafe {
        RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            buffer.as_mut_ptr() as *mut u8,
            &mut byte_len,
        )
    };
    if read != 0 {
        unsafe { RegCloseKey(key) };
        return Err(format!("read user PATH: {read}"));
    }
    let end = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    let current = String::from_utf16_lossy(&buffer[..end]);
    let (updated, removed) = without_path_entry(&current, install_root);
    if removed && !dry_run {
        let encoded = wide(std::ffi::OsStr::new(&updated));
        let value_kind = if kind == REG_SZ { REG_SZ } else { REG_EXPAND_SZ };
        let written = unsafe {
            RegSetValueExW(
                key,
                value_name.as_ptr(),
                0,
                value_kind,
                encoded.as_ptr() as *const u8,
                (encoded.len() * 2) as u32,
            )
        };
        if written != 0 {
            unsafe { RegCloseKey(key) };
            return Err(format!("write user PATH: {written}"));
        }
    }
    unsafe { RegCloseKey(key) };
    Ok(removed)
}

#[cfg(not(windows))]
fn remove_user_path(_install_root: &Path, _dry_run: bool) -> Result<bool, String> {
    Ok(false)
}

fn startup_command(tray: &Path) -> String {
    format!("\"{}\" --login-launch", tray.display())
}

fn startup_value_owned(value: &str, tray: &Path) -> bool {
    if cfg!(windows) {
        value.eq_ignore_ascii_case(&startup_command(tray))
    } else {
        value == startup_command(tray)
    }
}

#[cfg(windows)]
fn remove_startup_entries(tray: &Path, dry_run: bool) -> Result<usize, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_QUERY_VALUE, KEY_SET_VALUE, REG_VALUE_TYPE,
    };
    let wide = |value: &std::ffi::OsStr| value.encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let key_name = wide(std::ffi::OsStr::new(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
    ));
    let mut key: HKEY = std::ptr::null_mut();
    let access = KEY_QUERY_VALUE | if dry_run { 0 } else { KEY_SET_VALUE };
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key_name.as_ptr(),
            0,
            access,
            &mut key,
        )
    };
    if opened == 2 {
        return Ok(0);
    }
    if opened != 0 {
        return Err(format!("open user startup registry key: {opened}"));
    }
    let mut matched = 0;
    for name in ["Membrane", "Membrane Tray"] {
        let value_name = wide(std::ffi::OsStr::new(name));
        let mut kind: REG_VALUE_TYPE = 0;
        let mut byte_len = 0_u32;
        let queried = unsafe {
            RegQueryValueExW(
                key,
                value_name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut byte_len,
            )
        };
        if queried == 2 {
            continue;
        }
        if queried != 0 {
            unsafe { RegCloseKey(key) };
            return Err(format!("query user startup value {name}: {queried}"));
        }
        let mut buffer = vec![0_u16; (byte_len as usize + 1) / 2];
        let read = unsafe {
            RegQueryValueExW(
                key,
                value_name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buffer.as_mut_ptr() as *mut u8,
                &mut byte_len,
            )
        };
        if read != 0 {
            unsafe { RegCloseKey(key) };
            return Err(format!("read user startup value {name}: {read}"));
        }
        let end = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        let current = String::from_utf16_lossy(&buffer[..end]);
        if !startup_value_owned(&current, tray) {
            continue;
        }
        matched += 1;
        if !dry_run {
            let deleted = unsafe { RegDeleteValueW(key, value_name.as_ptr()) };
            if deleted != 0 {
                unsafe { RegCloseKey(key) };
                return Err(format!("remove user startup value {name}: {deleted}"));
            }
        }
    }
    unsafe { RegCloseKey(key) };
    Ok(matched)
}

#[cfg(not(windows))]
fn remove_startup_entries(_tray: &Path, _dry_run: bool) -> Result<usize, String> {
    Ok(0)
}

fn activation_receipt_owned(
    receipt: &ActivationReceiptV1,
    install_root: &Path,
    version_root: &Path,
    membrane: &Path,
    tray: &Path,
) -> bool {
    receipt.schema_version == ACTIVATION_RECEIPT_SCHEMA_VERSION
        && receipt.runtime_origin == RuntimeOrigin::Installed
        && !receipt.dry_run
        && receipt.service.service_id == SERVICE_ID
        && paths_equal(
            &receipt.install_root.to_string_lossy(),
            &install_root.to_string_lossy(),
        )
        && paths_equal(
            &receipt.version_root.to_string_lossy(),
            &version_root.to_string_lossy(),
        )
        && paths_equal(
            &receipt.membrane_executable.to_string_lossy(),
            &membrane.to_string_lossy(),
        )
        && paths_equal(
            &receipt.tray_executable.to_string_lossy(),
            &tray.to_string_lossy(),
        )
}

fn remove_activation_receipt(
    workspace_root: &Path,
    install_root: &Path,
    version_root: &Path,
    membrane: &Path,
    tray: &Path,
    dry_run: bool,
) -> Result<bool, String> {
    let path = workspace_root.join(ACTIVATION_RECEIPT_FILE);
    if !path.is_file() {
        return Ok(false);
    }
    let original = std::fs::read(&path)
        .map_err(|error| format!("read activation receipt {}: {error}", path.display()))?;
    let Ok(receipt) = serde_json::from_slice::<ActivationReceiptV1>(&original) else {
        return Ok(false);
    };
    if !activation_receipt_owned(&receipt, install_root, version_root, membrane, tray) {
        return Ok(false);
    }
    if dry_run {
        return Ok(true);
    }
    let current = std::fs::read(&path)
        .map_err(|error| format!("re-read activation receipt {}: {error}", path.display()))?;
    if current != original {
        return Err("activation receipt changed during deactivation; it was preserved".into());
    }
    std::fs::remove_file(&path)
        .map_err(|error| format!("remove activation receipt {}: {error}", path.display()))?;
    Ok(true)
}

fn reconcile_claude_hooks(install_root: &Path) -> Result<(), String> {
    let profile = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .ok_or_else(|| "Claude settings profile is unavailable".to_string())?;
    let settings_path = profile.join(".claude").join("settings.json");
    let mut settings: serde_json::Value = if settings_path.is_file() {
        serde_json::from_slice(
            &std::fs::read(&settings_path)
                .map_err(|error| format!("read Claude settings {}: {error}", settings_path.display()))?,
        )
        .map_err(|error| format!("parse Claude settings {}: {error}", settings_path.display()))?
    } else {
        serde_json::json!({})
    };
    let node = install_root.join("runtime/blueprint/lib").join(executable_name("node"));
    let entrypoint = install_root.join("mcp/hooks/membrane-hook-entrypoint.mjs");
    require_file(&node, "installed hook Node runtime")?;
    require_file(&entrypoint, "installed Claude hook entrypoint")?;
    let command = installed_hook_command(install_root);
    let root = settings
        .as_object_mut()
        .ok_or_else(|| "Claude settings root must be an object".to_string())?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Claude settings hooks must be an object".to_string())?;
    const EVENTS: [&str; 10] = [
        "SessionStart", "UserPromptSubmit", "PreCompact", "PostCompact", "PreToolUse",
        "PostToolUse", "PostToolUseFailure", "Stop", "TaskCompleted", "SessionEnd",
    ];
    for event in EVENTS {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| format!("Claude hook {event} must be an array"))?;
        replace_legacy_hook_commands(entries, &command);
        let present = entries.iter().any(|entry| {
            entry
                .get("hooks")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|items| items.iter().any(|item| {
                    item.get("command").and_then(serde_json::Value::as_str) == Some(&command)
                }))
        });
        if !present {
            let mut projection = serde_json::json!({
                "hooks": [{"type": "command", "command": command.clone()}]
            });
            if matches!(event, "PreToolUse" | "PostToolUse" | "PostToolUseFailure") {
                projection["matcher"] = serde_json::Value::String(".*".to_string());
            }
            entries.push(projection);
        }
    }
    let parent = settings_path
        .parent()
        .ok_or_else(|| "Claude settings path has no parent".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create Claude settings directory: {error}"))?;
    let staged = settings_path.with_extension(format!("json.{}.partial", std::process::id()));
    std::fs::write(
        &staged,
        serde_json::to_vec_pretty(&settings)
            .map_err(|error| format!("serialize Claude settings: {error}"))?,
    )
    .map_err(|error| format!("stage Claude settings: {error}"))?;
    replace_file(&staged, &settings_path)
        .map_err(|error| format!("promote Claude settings: {error}"))
}

fn installed_hook_command(install_root: &Path) -> String {
    let node = install_root
        .join("runtime/blueprint/lib")
        .join(executable_name("node"));
    let entrypoint = install_root.join("mcp/hooks/membrane-hook-entrypoint.mjs");
    format!("\"{}\" \"{}\"", node.display(), entrypoint.display())
}

fn remove_exact_hook_items(settings: &mut serde_json::Value, expected: &str) -> usize {
    let Some(hooks) = settings
        .as_object_mut()
        .and_then(|root| root.get_mut("hooks"))
        .and_then(serde_json::Value::as_object_mut)
    else {
        return 0;
    };
    let mut removed = 0;
    for entries in hooks.values_mut().filter_map(serde_json::Value::as_array_mut) {
        entries.retain_mut(|entry| {
            let Some(items) = entry
                .get_mut("hooks")
                .and_then(serde_json::Value::as_array_mut)
            else {
                return true;
            };
            let before = items.len();
            items.retain(|item| {
                item.get("command").and_then(serde_json::Value::as_str) != Some(expected)
            });
            removed += before - items.len();
            if !items.is_empty() {
                return true;
            }
            !entry.as_object().is_some_and(|object| {
                object
                    .keys()
                    .all(|key| matches!(key.as_str(), "hooks" | "matcher"))
            })
        });
    }
    removed
}

fn remove_claude_hooks(
    install_root: &Path,
    dry_run: bool,
) -> Result<usize, String> {
    let profile = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .ok_or_else(|| "Claude settings profile is unavailable".to_string())?;
    let settings_path = profile.join(".claude").join("settings.json");
    if !settings_path.is_file() {
        return Ok(0);
    }
    let original = std::fs::read(&settings_path)
        .map_err(|error| format!("read Claude settings {}: {error}", settings_path.display()))?;
    let mut settings: serde_json::Value = serde_json::from_slice(&original)
        .map_err(|error| format!("parse Claude settings {}: {error}", settings_path.display()))?;
    let removed = remove_exact_hook_items(&mut settings, &installed_hook_command(install_root));
    if removed == 0 || dry_run {
        return Ok(removed);
    }
    let staged = settings_path.with_extension(format!("json.{}.partial", std::process::id()));
    std::fs::write(
        &staged,
        serde_json::to_vec_pretty(&settings)
            .map_err(|error| format!("serialize Claude settings: {error}"))?,
    )
    .map_err(|error| format!("stage Claude settings: {error}"))?;
    let current = std::fs::read(&settings_path)
        .map_err(|error| format!("re-read Claude settings {}: {error}", settings_path.display()))?;
    if current != original {
        let _ = std::fs::remove_file(&staged);
        return Err("Claude settings changed during deactivation; exact hooks were preserved".into());
    }
    replace_file(&staged, &settings_path)
        .map_err(|error| format!("promote Claude settings: {error}"))?;
    Ok(removed)
}

fn replace_legacy_hook_commands(entries: &mut [serde_json::Value], expected: &str) {
    for entry in entries {
        let Some(items) = entry.get_mut("hooks").and_then(serde_json::Value::as_array_mut) else {
            continue;
        };
        for item in items {
            let Some(command) = item.get_mut("command") else { continue };
            let owned = command.as_str().is_some_and(|value| {
                value.contains("membrane_host.py")
                    || (value.contains(".venv-tools") && value.to_ascii_lowercase().contains("membrane"))
            });
            if owned {
                *command = serde_json::Value::String(expected.to_string());
            }
        }
    }
}

fn reconcile_clients<F>(
    membrane: &Path,
    clients: &[HarnessClient],
    dry_run: bool,
    mut runner: F,
) -> Result<Vec<ClientActivationReceipt>, String>
where
    F: FnMut(HarnessClient, &[String]) -> CommandResult,
{
    let executable = membrane.to_string_lossy().into_owned();
    let mut inspections = Vec::with_capacity(clients.len());
    for &client in clients {
        let detected = runner(client, &["--version".to_string()]);
        let state = if !detected.success() {
            ClientState::NotInstalled
        } else {
            inspect_client(client, &executable, &mut runner)?
        };
        inspections.push((client, state));
    }

    if dry_run {
        return Ok(inspections
            .into_iter()
            .map(|(client, state)| ClientActivationReceipt {
                client,
                before: state.label().to_string(),
                after: state.label().to_string(),
                changed: false,
            })
            .collect());
    }

    let mut completed: Vec<(HarnessClient, ClientState)> = Vec::new();
    for (client, state) in inspections {
        if matches!(
            state,
            ClientState::NotInstalled | ClientState::AlreadyCorrect
        ) {
            completed.push((client, state));
            continue;
        }
        let result = (|| {
            if matches!(state, ClientState::Conflict(_)) {
                require_command_success(client, "remove", runner(client, &remove_args(client)))?;
            }
            require_command_success(
                client,
                "add",
                runner(
                    client,
                    &add_args(client, &executable, &["stdio-mcp".to_string()]),
                ),
            )?;
            match inspect_client(client, &executable, &mut runner)? {
                ClientState::AlreadyCorrect => Ok(()),
                _ => Err(format!("{} add verification failed", client.as_str())),
            }
        })();
        if let Err(error) = result {
            rollback_clients(client, &state, &completed, &mut runner);
            return Err(error);
        }
        completed.push((client, state));
    }

    Ok(completed
        .into_iter()
        .map(|(client, state)| {
            let changed = matches!(state, ClientState::Absent | ClientState::Conflict(_));
            ClientActivationReceipt {
                client,
                before: state.label().to_string(),
                after: if changed { "installed" } else { state.label() }.to_string(),
                changed,
            }
        })
        .collect())
}

fn deactivate_clients<F>(
    membrane: &Path,
    clients: &[HarnessClient],
    dry_run: bool,
    mut runner: F,
) -> Result<Vec<ClientActivationReceipt>, String>
where
    F: FnMut(HarnessClient, &[String]) -> CommandResult,
{
    let executable = membrane.to_string_lossy().into_owned();
    let mut receipts = Vec::with_capacity(clients.len());
    for &client in clients {
        if !runner(client, &["--version".to_string()]).success() {
            receipts.push(ClientActivationReceipt {
                client,
                before: "not_installed".to_string(),
                after: "not_installed".to_string(),
                changed: false,
            });
            continue;
        }
        let current = runner(client, &get_args(client));
        if !current.success() {
            receipts.push(ClientActivationReceipt {
                client,
                before: "absent".to_string(),
                after: "absent".to_string(),
                changed: false,
            });
            continue;
        }
        let owned = parse_prior_config(&current.stdout).is_some_and(|config| {
            paths_equal(&config.command, &executable) && config.args == ["stdio-mcp".to_string()]
        });
        if owned && !dry_run {
            require_command_success(client, "remove", runner(client, &remove_args(client)))?;
            if runner(client, &get_args(client)).success() {
                return Err(format!("{} remove verification failed", client.as_str()));
            }
        }
        receipts.push(ClientActivationReceipt {
            client,
            before: if owned { "owned" } else { "preserved" }.to_string(),
            after: if owned && !dry_run {
                "removed"
            } else if owned {
                "owned"
            } else {
                "preserved"
            }
            .to_string(),
            changed: owned && !dry_run,
        });
    }
    Ok(receipts)
}

fn inspect_client<F>(
    client: HarnessClient,
    executable: &str,
    runner: &mut F,
) -> Result<ClientState, String>
where
    F: FnMut(HarnessClient, &[String]) -> CommandResult,
{
    let current = runner(client, &get_args(client));
    if !current.success() {
        return Ok(ClientState::Absent);
    }
    if is_expected(&current.stdout, executable) {
        return Ok(ClientState::AlreadyCorrect);
    }
    parse_prior_config(&current.stdout)
        .map(ClientState::Conflict)
        .ok_or_else(|| {
            format!(
                "{} has conflicting membrane entry that cannot be safely restored",
                client.as_str()
            )
        })
}

fn rollback_clients<F>(
    active_client: HarnessClient,
    active_state: &ClientState,
    completed: &[(HarnessClient, ClientState)],
    runner: &mut F,
) where
    F: FnMut(HarnessClient, &[String]) -> CommandResult,
{
    let _ = runner(active_client, &remove_args(active_client));
    if let ClientState::Conflict(prior) = active_state {
        let _ = runner(
            active_client,
            &add_args(active_client, &prior.command, &prior.args),
        );
    }
    for (client, state) in completed.iter().rev() {
        if !matches!(state, ClientState::Absent | ClientState::Conflict(_)) {
            continue;
        }
        let _ = runner(*client, &remove_args(*client));
        if let ClientState::Conflict(prior) = state {
            let _ = runner(*client, &add_args(*client, &prior.command, &prior.args));
        }
    }
}

fn get_args(client: HarnessClient) -> Vec<String> {
    match client {
        HarnessClient::Codex => vec!["mcp", "get", "membrane", "--json"],
        HarnessClient::Claude => vec!["mcp", "get", "membrane", "-s", "user"],
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn remove_args(client: HarnessClient) -> Vec<String> {
    match client {
        HarnessClient::Codex => vec!["mcp", "remove", "membrane"],
        HarnessClient::Claude => vec!["mcp", "remove", "membrane", "-s", "user"],
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn add_args(client: HarnessClient, command: &str, args: &[String]) -> Vec<String> {
    let mut values = vec!["mcp".to_string(), "add".to_string()];
    if client == HarnessClient::Claude {
        values.extend(["--scope".to_string(), "user".to_string()]);
    }
    values.extend([
        "membrane".to_string(),
        "--".to_string(),
        command.to_string(),
    ]);
    values.extend(args.iter().cloned());
    values
}

fn require_command_success(
    client: HarnessClient,
    action: &str,
    result: CommandResult,
) -> Result<(), String> {
    result.success().then_some(()).ok_or_else(|| {
        let detail = if result.stderr.trim().is_empty() {
            result.stdout.trim()
        } else {
            result.stderr.trim()
        };
        format!("{} {action} failed: {detail}", client.as_str())
    })
}

fn parse_prior_config(stdout: &str) -> Option<ServerConfig> {
    let parsed = serde_json::from_str::<serde_json::Value>(stdout)
        .ok()
        .and_then(|parsed| {
            let config = parsed
                .get("server")
                .or_else(|| parsed.get("transport"))
                .unwrap_or(&parsed);
            let command = config
                .get("command")
                .or_else(|| config.get("commandOrUrl"))?
                .as_str()?
                .to_string();
            let args = match config
                .get("args")
                .or_else(|| config.get("arguments"))
                .and_then(serde_json::Value::as_array)
            {
                Some(values) => values
                    .iter()
                    .map(|value| value.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()?,
                None => Vec::new(),
            };
            Some(ServerConfig { command, args })
        })
        .or_else(|| parse_labeled_client_config(stdout))?;
    validate_server_config(parsed)
}

fn parse_labeled_client_config(stdout: &str) -> Option<ServerConfig> {
    let command = stdout.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Command:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })?;
    let args = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("Args:").map(str::trim))
        .map(|value| value.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    Some(ServerConfig { command, args })
}

fn validate_server_config(config: ServerConfig) -> Option<ServerConfig> {
    if std::iter::once(config.command.as_str())
        .chain(config.args.iter().map(String::as_str))
        .any(|value| {
            value
                .chars()
                .any(|character| matches!(character, '\r' | '\n' | '&' | '|' | '<' | '>' | '^'))
        })
    {
        return None;
    }
    Some(config)
}

fn is_expected(stdout: &str, executable: &str) -> bool {
    if let Some(config) = parse_prior_config(stdout) {
        return paths_equal(&config.command, executable)
            && config.args == ["stdio-mcp".to_string()];
    }
    let normalized = stdout.replace("\\\\", "\\");
    let expected = normalize_windows_path(executable);
    normalized
        .to_ascii_lowercase()
        .contains(&expected.to_ascii_lowercase())
        && normalized.contains("stdio-mcp")
}

fn paths_equal(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        normalize_windows_path(left).eq_ignore_ascii_case(&normalize_windows_path(right))
    } else {
        left == right
    }
}

fn normalize_windows_path(value: &str) -> String {
    let normalized = value.replace('/', "\\");
    if let Some(rest) = normalized.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    normalized
        .strip_prefix(r"\\?\")
        .unwrap_or(&normalized)
        .to_string()
}

fn run_client(client: HarnessClient, args: &[String]) -> CommandResult {
    let requested =
        std::env::var_os(client.binary_env()).unwrap_or_else(|| OsString::from(client.as_str()));
    let resolved = resolve_command(&requested).unwrap_or(PathBuf::from(&requested));
    let output = run_resolved_command(&resolved, args);
    match output {
        Ok(output) => output_result(output),
        Err(error) => CommandResult {
            code: 127,
            stdout: String::new(),
            stderr: error.to_string(),
        },
    }
}

fn resolve_command(requested: &std::ffi::OsStr) -> Option<PathBuf> {
    let requested_path = PathBuf::from(requested);
    if requested_path.components().count() > 1 {
        return requested_path.is_file().then_some(requested_path);
    }
    let path = std::env::var_os("PATH")?;
    let extensions: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    for directory in std::env::split_paths(&path) {
        for extension in extensions {
            let candidate = directory.join(format!("{}{}", requested.to_string_lossy(), extension));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn run_resolved_command(path: &Path, args: &[String]) -> std::io::Result<Output> {
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let mut command =
            if extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat") {
                let mut command = Command::new("cmd.exe");
                command.args(["/D", "/S", "/C"]).arg(path);
                command
            } else {
                Command::new(path)
            };
        command.creation_flags(CREATE_NO_WINDOW);
        command
    };
    #[cfg(not(windows))]
    let mut command = Command::new(path);
    command.args(args).output()
}

fn output_result(output: Output) -> CommandResult {
    CommandResult {
        code: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn persist_receipt(root: &Path, receipt: &ActivationReceiptV1) -> Result<(), String> {
    let destination = root.join(ACTIVATION_RECEIPT_FILE);
    let staged = root.join(format!(
        ".{ACTIVATION_RECEIPT_FILE}.{}.partial",
        std::process::id()
    ));
    let bytes = serde_json::to_vec_pretty(receipt)
        .map_err(|error| format!("serialize activation receipt: {error}"))?;
    std::fs::write(&staged, bytes)
        .map_err(|error| format!("stage activation receipt {}: {error}", staged.display()))?;
    if let Err(error) = replace_file(&staged, &destination) {
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    (result != 0).then_some(()).ok_or_else(|| {
        format!(
            "promote activation receipt: {}",
            std::io::Error::last_os_error()
        )
    })
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::rename(source, destination)
        .map_err(|error| format!("promote activation receipt: {error}"))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn result(code: i32, stdout: &str) -> CommandResult {
        CommandResult {
            code,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    #[test]
    fn codex_json_config_is_parsed_and_matched() {
        let body = r#"{"transport":{"type":"stdio","command":"C:\\Membrane\\membrane.exe","args":["stdio-mcp"]}}"#;
        assert_eq!(
            parse_prior_config(body),
            Some(ServerConfig {
                command: r"C:\Membrane\membrane.exe".to_string(),
                args: vec!["stdio-mcp".to_string()],
            })
        );
        assert!(is_expected(body, r"C:\Membrane\membrane.exe"));
        #[cfg(windows)]
        assert!(is_expected(
            "Command: C:\\Membrane\\membrane.exe\nArgs: stdio-mcp",
            r"\\?\C:\Membrane\membrane.exe"
        ));
    }

    #[test]
    fn claude_labeled_config_is_parsed_for_upgrade_and_rollback() {
        let body = "membrane:\n  Scope: User config\n  Status: Connected\n  Type: stdio\n  Command: C:\\Membrane Hub\\membrane.exe\n  Args: stdio-mcp\n  Environment:";
        assert_eq!(
            parse_prior_config(body),
            Some(ServerConfig {
                command: r"C:\Membrane Hub\membrane.exe".to_string(),
                args: vec!["stdio-mcp".to_string()],
            })
        );
        assert_eq!(
            get_args(HarnessClient::Claude),
            ["mcp", "get", "membrane", "-s", "user"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            remove_args(HarnessClient::Claude),
            ["mcp", "remove", "membrane", "-s", "user"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn registration_ports_native_cli_add_and_verification_flow() {
        let membrane = Path::new(r"C:\Membrane\membrane.exe");
        let expected =
            r#"{"transport":{"command":"C:\\Membrane\\membrane.exe","args":["stdio-mcp"]}}"#;
        let mut responses = VecDeque::from([
            result(0, "codex-cli"),
            result(1, ""),
            result(0, ""),
            result(0, expected),
        ]);
        let mut calls = Vec::new();
        let receipts =
            reconcile_clients(membrane, &[HarnessClient::Codex], false, |client, args| {
                calls.push((client, args.to_vec()));
                responses.pop_front().unwrap()
            })
            .unwrap();
        assert_eq!(receipts[0].before, "absent");
        assert_eq!(receipts[0].after, "installed");
        assert!(receipts[0].changed);
        assert_eq!(
            calls[2].1,
            add_args(
                HarnessClient::Codex,
                &membrane.to_string_lossy(),
                &["stdio-mcp".to_string()]
            )
        );
    }

    #[test]
    fn later_failure_restores_prior_binding() {
        let membrane = Path::new(r"C:\Membrane\membrane.exe");
        let prior = r#"{"transport":{"command":"node","args":["old.mjs"]}}"#;
        let expected =
            r#"{"transport":{"command":"C:\\Membrane\\membrane.exe","args":["stdio-mcp"]}}"#;
        let mut responses = VecDeque::from([
            result(0, "codex-cli"),
            result(0, prior),
            result(0, "claude"),
            result(1, ""),
            result(0, ""),
            result(0, ""),
            result(0, expected),
            result(1, "add failed"),
            result(0, ""),
            result(0, ""),
            result(0, ""),
        ]);
        let mut calls = Vec::new();
        let error = reconcile_clients(
            membrane,
            &[HarnessClient::Codex, HarnessClient::Claude],
            false,
            |client, args| {
                calls.push((client, args.to_vec()));
                responses.pop_front().unwrap()
            },
        )
        .unwrap_err();
        assert!(error.contains("claude add failed"));
        assert!(calls.iter().any(|(client, args)| {
            *client == HarnessClient::Codex
                && *args == add_args(HarnessClient::Codex, "node", &["old.mjs".to_string()])
        }));
    }

    #[test]
    fn deactivation_removes_only_exact_owned_client_binding() {
        let membrane = Path::new(r"C:\Membrane\current\membrane.exe");
        let exact = r#"{"transport":{"command":"C:\\Membrane\\current\\membrane.exe","args":["stdio-mcp"]}}"#;
        let foreign = r#"{"transport":{"command":"C:\\Membrane\\current\\membrane.exe","args":["stdio-mcp","--foreign"]}}"#;
        let mut responses = VecDeque::from([
            result(0, "codex-cli"),
            result(0, exact),
            result(0, ""),
            result(1, ""),
            result(0, "claude-cli"),
            result(0, foreign),
        ]);
        let mut calls = Vec::new();
        let receipts = deactivate_clients(
            membrane,
            &[HarnessClient::Codex, HarnessClient::Claude],
            false,
            |client, args| {
                calls.push((client, args.to_vec()));
                responses.pop_front().unwrap()
            },
        )
        .unwrap();
        assert_eq!(receipts[0].after, "removed");
        assert!(receipts[0].changed);
        assert_eq!(receipts[1].after, "preserved");
        assert!(!receipts[1].changed);
        assert!(calls.iter().any(|(client, args)| {
            *client == HarnessClient::Codex && *args == remove_args(HarnessClient::Codex)
        }));
        assert!(!calls.iter().any(|(client, args)| {
            *client == HarnessClient::Claude && *args == remove_args(HarnessClient::Claude)
        }));
    }

    #[test]
    fn deactivation_dry_run_plans_owned_binding_without_remove() {
        let membrane = Path::new(r"C:\Membrane\current\membrane.exe");
        let exact = r#"{"transport":{"command":"C:\\Membrane\\current\\membrane.exe","args":["stdio-mcp"]}}"#;
        let mut responses = VecDeque::from([result(0, "codex-cli"), result(0, exact)]);
        let mut calls = Vec::new();
        let receipts = deactivate_clients(
            membrane,
            &[HarnessClient::Codex],
            true,
            |client, args| {
                calls.push((client, args.to_vec()));
                responses.pop_front().unwrap()
            },
        )
        .unwrap();
        assert_eq!(receipts[0].before, "owned");
        assert_eq!(receipts[0].after, "owned");
        assert!(!receipts[0].changed);
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn deactivation_hook_removal_preserves_near_matches_and_unrelated_items() {
        let expected = r#""C:\Membrane\current\node.exe" "C:\Membrane\current\mcp\hooks\membrane-hook-entrypoint.mjs""#;
        let mut settings = serde_json::json!({
            "hooks": {
                "SessionStart": [
                    {"hooks": [
                        {"type": "command", "command": expected},
                        {"type": "command", "command": "keep-me"}
                    ]},
                    {"hooks": [{"type": "command", "command": format!("{expected} --extra")}]}
                ],
                "Stop": [{"hooks": [{"type": "command", "command": expected}]}],
                "Custom": [{"owner": "user", "hooks": [{"type": "command", "command": expected}]}],
                "Malformed": [{"other": true}]
            }
        });
        assert_eq!(remove_exact_hook_items(&mut settings, expected), 3);
        assert_eq!(settings["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
        assert_eq!(settings["hooks"]["SessionStart"][0]["hooks"][0]["command"], "keep-me");
        assert!(settings["hooks"]["Stop"].as_array().unwrap().is_empty());
        assert_eq!(settings["hooks"]["Custom"][0]["owner"], "user");
        assert!(settings["hooks"]["Custom"][0]["hooks"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(settings["hooks"]["Malformed"][0]["other"], true);
    }

    #[test]
    fn deactivation_path_removal_drops_only_exact_stable_current_entries() {
        let stable = Path::new(r"C:\Users\test\Orthic Labs\Membrane\current");
        let current = r#"C:\Windows; "C:\Users\test\Orthic Labs\Membrane\current" ;C:\Users\test\Orthic Labs\Membrane\current-tools;C:\Elsewhere"#;
        let (updated, removed) = without_path_entry(current, stable);
        assert!(removed);
        assert_eq!(
            updated,
            r#"C:\Windows;C:\Users\test\Orthic Labs\Membrane\current-tools;C:\Elsewhere"#
        );
        let (unchanged, removed) = without_path_entry(&updated, stable);
        assert!(!removed);
        assert_eq!(unchanged, updated);
    }

    #[test]
    fn deactivation_startup_match_requires_exact_owned_command() {
        let tray = Path::new(r"C:\Users\test\Orthic Labs\Membrane\current\membrane-tray.exe");
        let exact = startup_command(tray);
        assert!(startup_value_owned(&exact, tray));
        assert!(!startup_value_owned(
            &format!("{exact} --open-dashboard"),
            tray
        ));
        assert!(!startup_value_owned(
            r#""C:\Other\membrane-tray.exe" --login-launch"#,
            tray
        ));
    }

    #[test]
    fn deactivation_activation_receipt_match_is_exact() {
        let install_root = PathBuf::from(r"C:\Membrane\current");
        let version_root = PathBuf::from(r"C:\Membrane\versions\v1");
        let membrane = install_root.join("membrane.exe");
        let tray = install_root.join("membrane-tray.exe");
        let receipt = ActivationReceiptV1 {
            schema_version: ACTIVATION_RECEIPT_SCHEMA_VERSION,
            runtime_origin: RuntimeOrigin::Installed,
            install_root: install_root.clone(),
            version_root: version_root.clone(),
            membrane_executable: membrane.clone(),
            tray_executable: tray.clone(),
            activated_at_unix_ms: 1,
            dry_run: false,
            service: ServiceActivationReceipt {
                service_id: SERVICE_ID.to_string(),
                port: INSTALLED_PORT,
                release_generation: "sha256:test".to_string(),
                already_running: false,
                state: "ready".to_string(),
                reason: None,
            },
            clients: Vec::new(),
        };
        assert!(activation_receipt_owned(
            &receipt,
            &install_root,
            &version_root,
            &membrane,
            &tray
        ));
        let mut foreign = receipt.clone();
        foreign.membrane_executable = PathBuf::from(r"C:\Other\membrane.exe");
        assert!(!activation_receipt_owned(
            &foreign,
            &install_root,
            &version_root,
            &membrane,
            &tray
        ));
    }

    #[test]
    fn health_gate_rejects_foreign_identity_and_accepts_exact_generation() {
        let ready = b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true,\"serviceId\":\"membrane-hub\",\"nativeOnly\":true,\"runtimeOrigin\":\"installed\",\"releaseGeneration\":\"g1\"}";
        assert_eq!(
            parse_health_response(ready, "g1").unwrap(),
            HealthObservation::Ready {
                release_generation: "g1".to_string()
            }
        );
        let foreign = b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true,\"serviceId\":\"other\",\"nativeOnly\":true,\"releaseGeneration\":\"g1\"}";
        assert!(matches!(
            parse_health_response(foreign, "g1").unwrap(),
            HealthObservation::Foreign(_)
        ));
        assert_eq!(
            parse_health_response(ready, "g2").unwrap(),
            HealthObservation::PriorGeneration {
                release_generation: "g1".to_string()
            }
        );
        let development = b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true,\"serviceId\":\"membrane-hub\",\"nativeOnly\":true,\"runtimeOrigin\":\"development\",\"releaseGeneration\":\"g1\"}";
        assert!(matches!(
            parse_health_response(development, "g1").unwrap(),
            HealthObservation::Foreign(reason) if reason.contains("development")
        ));
        let legacy = b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true,\"serviceId\":\"membrane-hub\",\"nativeOnly\":true,\"releaseGeneration\":\"g0\"}";
        assert_eq!(
            parse_health_response(legacy, "g1").unwrap(),
            HealthObservation::PriorGeneration {
                release_generation: "g0".to_string()
            }
        );
    }

    #[test]
    fn activation_receipt_health_keys_are_camel_case() {
        let receipt = ActivationReceiptV1 {
            schema_version: ACTIVATION_RECEIPT_SCHEMA_VERSION,
            runtime_origin: RuntimeOrigin::Installed,
            install_root: PathBuf::from("current"),
            version_root: PathBuf::from("versions/v1"),
            membrane_executable: PathBuf::from("current/membrane"),
            tray_executable: PathBuf::from("current/membrane-tray"),
            activated_at_unix_ms: 1,
            dry_run: true,
            service: ServiceActivationReceipt {
                service_id: SERVICE_ID.to_string(),
                port: 43177,
                release_generation: "sha256:test".to_string(),
                already_running: true,
                state: "ready".to_string(),
                reason: None,
            },
            clients: Vec::new(),
        };
        let value = serde_json::to_value(receipt).unwrap();
        assert_eq!(value["schemaVersion"], ACTIVATION_RECEIPT_SCHEMA_VERSION);
        assert_eq!(value["runtimeOrigin"], "installed");
        assert_eq!(value["dryRun"], true);
        assert_eq!(value["service"]["serviceId"], SERVICE_ID);
        assert_eq!(value["service"]["releaseGeneration"], "sha256:test");
        assert!(value.get("schema_version").is_none());
        assert!(value.get("dry_run").is_none());
    }

    #[test]
    fn status_requires_exact_ready_resident_generation() {
        assert_eq!(
            require_current_health(
                HealthObservation::Ready {
                    release_generation: "g2".to_string()
                },
                "g2"
            )
            .unwrap(),
            "g2"
        );
        assert!(require_current_health(HealthObservation::Unavailable, "g2")
            .unwrap_err()
            .contains("not running"));
        assert!(require_current_health(
            HealthObservation::PriorGeneration {
                release_generation: "g1".to_string()
            },
            "g2"
        )
        .unwrap_err()
        .contains("does not match"));
    }

    #[test]
    fn installed_runtime_is_product_state_on_fixed_port() {
        let (root, port) = installed_runtime(Path::new(r"C:\Users\test\Orthic Labs\Membrane"))
            .expect("installed runtime layout");
        assert_eq!(
            root,
            Path::new(r"C:\Users\test\Orthic Labs\Membrane").join("state")
        );
        assert_eq!(port, INSTALLED_PORT);
    }

    #[test]
    fn dry_run_health_projection_is_non_ready_but_serializable() {
        let receipt = ActivationReceiptV1 {
            schema_version: ACTIVATION_RECEIPT_SCHEMA_VERSION,
            runtime_origin: RuntimeOrigin::Installed,
            install_root: PathBuf::from("current"),
            version_root: PathBuf::from("versions/v1"),
            membrane_executable: PathBuf::from("current/membrane.exe"),
            tray_executable: PathBuf::from("current/membrane-tray.exe"),
            activated_at_unix_ms: 1,
            dry_run: true,
            service: ServiceActivationReceipt {
                service_id: SERVICE_ID.to_string(),
                port: INSTALLED_PORT,
                release_generation: "sha256:test".to_string(),
                already_running: false,
                state: "unavailable".to_string(),
                reason: Some("installed Membrane is not running".to_string()),
            },
            clients: Vec::new(),
        };
        let value = serde_json::to_value(receipt).expect("inspection receipt JSON");
        assert_eq!(value["dryRun"], true);
        assert_eq!(value["service"]["state"], "unavailable");
        assert_eq!(value["service"]["port"], INSTALLED_PORT);
    }
}
