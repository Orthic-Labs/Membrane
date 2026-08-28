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
pub const ACTIVATION_RECEIPT_FILE: &str = "activation-receipt.json";
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationReceiptV1 {
    pub schema_version: u32,
    pub install_root: PathBuf,
    pub membrane_executable: PathBuf,
    pub tray_executable: PathBuf,
    pub activated_at_unix_ms: u64,
    pub dry_run: bool,
    pub service: ServiceActivationReceipt,
    pub clients: Vec<ClientActivationReceipt>,
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
    std::env::current_exe()
        .map_err(|error| format!("resolve activation executable: {error}"))?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "activation executable has no parent directory".to_string())
}

pub fn activate(options: ActivationOptions) -> Result<ActivationReceiptV1, String> {
    let install_root = std::fs::canonicalize(&options.install_root).map_err(|error| {
        format!(
            "activation install root {} is unavailable: {error}",
            options.install_root.display()
        )
    })?;
    let membrane = install_root.join(executable_name("membrane"));
    let tray = install_root.join(executable_name("membrane-tray"));
    require_file(&membrane, "membrane executable")?;
    require_file(&tray, "tray executable")?;
    let (workspace_root, port) = installed_runtime()?;
    let expected_generation = membrane_runtime::release_identity::release_generation();
    let _lock = acquire_lock(&install_root)?;

    let initial = probe_health(port, &expected_generation)?;
    let already_running = matches!(initial, HealthObservation::Ready { .. });
    let release_generation = if options.dry_run {
        match initial {
            HealthObservation::Ready { release_generation } => release_generation,
            HealthObservation::Foreign(reason) => return Err(reason),
            _ => expected_generation.clone(),
        }
    } else if let HealthObservation::Ready { release_generation } = initial {
        release_generation
    } else {
        launch_tray(&tray, &workspace_root, port)?;
        wait_for_health(port, &expected_generation, options.timeout)?
    };

    let clients = reconcile_clients(&membrane, &options.clients, options.dry_run, run_client)?;
    let receipt = ActivationReceiptV1 {
        schema_version: ACTIVATION_RECEIPT_SCHEMA_VERSION,
        install_root: install_root.clone(),
        membrane_executable: membrane,
        tray_executable: tray,
        activated_at_unix_ms: now_unix_ms(),
        dry_run: options.dry_run,
        service: ServiceActivationReceipt {
            service_id: SERVICE_ID.to_string(),
            port,
            release_generation,
            already_running,
        },
        clients,
    };
    if !options.dry_run {
        persist_receipt(&install_root, &receipt)?;
    }
    Ok(receipt)
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
    let mut command = Command::new(tray);
    command
        .arg("--activate")
        .env("MEMBRANE_WORKSPACE_ROOT", workspace_root)
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
            HealthObservation::Unavailable | HealthObservation::NotReady => {}
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
    if release_generation != expected_generation {
        return Ok(HealthObservation::Foreign(format!(
            "resident release generation {release_generation} does not match installed generation {expected_generation}"
        )));
    }
    if status != 200 || body.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Ok(HealthObservation::NotReady);
    }
    Ok(HealthObservation::Ready {
        release_generation: release_generation.to_string(),
    })
}

fn installed_runtime() -> Result<(PathBuf, u16), String> {
    if let Some(value) = std::env::var_os("MEMBRANE_PORT") {
        let port = value
            .to_string_lossy()
            .parse::<u16>()
            .ok()
            .filter(|port| *port >= 1024)
            .ok_or_else(|| "MEMBRANE_PORT is invalid".to_string())?;
        let workspace_root = std::env::var_os("MEMBRANE_WORKSPACE_ROOT")
            .map(PathBuf::from)
            .ok_or_else(|| "MEMBRANE_WORKSPACE_ROOT is required with MEMBRANE_PORT".to_string())?;
        return Ok((workspace_root, port));
    }
    let workspace_root = if let Some(value) = std::env::var_os("MEMBRANE_WORKSPACE_ROOT") {
        PathBuf::from(value)
    } else {
        let profile = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .ok_or_else(|| "workspace profile is unavailable".to_string())?;
        let path = profile.join(".config/membrane/workspace.json");
        let value: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&path)
                .map_err(|error| format!("read workspace config {}: {error}", path.display()))?,
        )
        .map_err(|error| format!("parse workspace config {}: {error}", path.display()))?;
        if value
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            != Some(3)
        {
            return Err("workspace config schema is unsupported".to_string());
        }
        value
            .get("workspaceRoot")
            .and_then(serde_json::Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| "workspace config root is missing".to_string())?
    };
    let runtime = workspace_root.join("tools/lib/memory/runtime.json");
    let value: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&runtime)
            .map_err(|error| format!("read runtime config {}: {error}", runtime.display()))?,
    )
    .map_err(|error| format!("parse runtime config {}: {error}", runtime.display()))?;
    if value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
        || value.get("serviceId").and_then(serde_json::Value::as_str) != Some("membrane-local-v1")
        || value.get("host").and_then(serde_json::Value::as_str) != Some("127.0.0.1")
    {
        return Err("runtime config identity is invalid".to_string());
    }
    let port = value
        .get("port")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|port| *port >= 1024)
        .ok_or_else(|| "runtime config port is invalid".to_string())?;
    Ok((workspace_root, port))
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
        HarnessClient::Claude => vec!["mcp", "get", "membrane"],
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn remove_args(_client: HarnessClient) -> Vec<String> {
    ["mcp", "remove", "membrane"]
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
    let parsed: serde_json::Value = serde_json::from_str(stdout).ok()?;
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
    if std::iter::once(command.as_str())
        .chain(args.iter().map(String::as_str))
        .any(|value| {
            value
                .chars()
                .any(|character| matches!(character, '\r' | '\n' | '&' | '|' | '<' | '>' | '^'))
        })
    {
        return None;
    }
    Some(ServerConfig { command, args })
}

fn is_expected(stdout: &str, executable: &str) -> bool {
    if let Some(config) = parse_prior_config(stdout) {
        return paths_equal(&config.command, executable)
            && config.args == ["stdio-mcp".to_string()];
    }
    let normalized = stdout.replace("\\\\", "\\");
    normalized.contains(executable) && normalized.contains("stdio-mcp")
}

fn paths_equal(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.replace('/', "\\")
            .eq_ignore_ascii_case(&right.replace('/', "\\"))
    } else {
        left == right
    }
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
    fn health_gate_rejects_foreign_identity_and_accepts_exact_generation() {
        let ready = b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true,\"serviceId\":\"membrane-hub\",\"nativeOnly\":true,\"releaseGeneration\":\"g1\"}";
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
    }
}
