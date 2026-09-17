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
//! Native activation owns exact Membrane add/get/remove command shapes &
//! conflict-restoration contract. Claude Code & Codex use the installed
//! authenticated Streamable HTTP listener; only unsupported hosts retain the
//! transport-only stdio compatibility entrypoint.

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::Write as IoWrite,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const ACTIVATION_RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const DEACTIVATION_RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const ACTIVATION_RECEIPT_FILE: &str = "activation-receipt.json";
pub const INSTALLED_PORT: u16 = 47_851;
const MCP_HTTP_PATH: &str = "/mcp";
const MCP_TOKEN_ENV: &str = "MEMBRANE_BEARER_TOKEN";
const WORKSPACE_SCHEMA_VERSION: u32 = 3;
const WORKSPACE_MIGRATION_RECEIPT_SCHEMA_VERSION: u32 = 1;
const WORKSPACE_MIGRATION_NAME: &str = "workspace_config_v2_to_v3";
const LOCK_DIR: &str = ".activation.lock";
const LOCK_STALE_AFTER: Duration = Duration::from_secs(90);
const LOCK_WAIT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const SERVICE_ID: &str = "membrane-hub";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessClient {
    Codex,
    Claude,
    Cursor,
    Windsurf,
    Antigravity,
    Devin,
}

impl HarnessClient {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            "cursor" => Ok(Self::Cursor),
            "windsurf" => Ok(Self::Windsurf),
            "antigravity" => Ok(Self::Antigravity),
            "devin" => Ok(Self::Devin),
            _ => Err(format!(
                "unsupported harness `{value}`; expected codex, claude, cursor, windsurf, antigravity, or devin"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Cursor => "cursor",
            Self::Windsurf => "windsurf",
            Self::Antigravity => "antigravity",
            Self::Devin => "devin",
        }
    }

    fn binary_env(self) -> &'static str {
        match self {
            Self::Codex => "MEMBRANE_CODEX_BIN",
            Self::Claude => "MEMBRANE_CLAUDE_BIN",
            Self::Cursor => "MEMBRANE_CURSOR_BIN",
            Self::Windsurf => "MEMBRANE_WINDSURF_BIN",
            Self::Antigravity => "MEMBRANE_ANTIGRAVITY_BIN",
            Self::Devin => "MEMBRANE_DEVIN_BIN",
        }
    }

    /// Hosts with a native plugin surface that can own hooks & plugin-scoped
    /// MCP when the installed plugin is enabled.
    fn has_plugin_surface(self) -> bool {
        matches!(self, Self::Codex | Self::Claude)
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

/// Per-client native plugin projection state. `enabled` is the only state in
/// which the host's plugin surface (SessionStart hooks, plugin-scoped MCP
/// binding) is known to be projected; every other state names the exact
/// remaining step instead of implying readiness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginActivationReceipt {
    /// `enabled` | `installed_disabled` | `absent` | `action_required` |
    /// `conflict` | `not_applicable`; deactivation receipts may also report
    /// `detached`.
    pub state: String,
    /// `membrane@membrane` when a plugin surface exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    /// Observed plugin payload version inside the host cache, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Activation changed the host's plugin state this pass.
    #[serde(default)]
    pub changed: bool,
    /// Exact manual command or remaining requirement (never a readiness claim).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ClientActivationReceipt {
    pub client: HarnessClient,
    pub before: String,
    pub after: String,
    pub changed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginActivationReceipt>,
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
    /// `full` | `engine_only` | `bindings_only`. `service.state` is engine
    /// readiness only; host/plugin readiness lives in `clients[].plugin` and
    /// is never implied by an empty `clients` list (engine-only scope).
    #[serde(default = "default_activation_scope")]
    pub activation_scope: String,
    /// Optional so existing activation receipts remain readable.
    #[serde(default)]
    pub workspace_config_migration: Option<WorkspaceConfigMigrationReceipt>,
}

fn default_activation_scope() -> String {
    "full".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfigMigrationReceipt {
    pub schema_version: u32,
    pub migration: String,
    pub workspace_root: PathBuf,
    pub migrated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceConfigV3 {
    schema_version: u32,
    workspace_root: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceConfigV2 {
    schema_version: u32,
    workspace_root: PathBuf,
    python_executable: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceConfigV3Output<'a> {
    schema_version: u32,
    workspace_root: &'a Path,
}

/// Installer-owned v2→v3 migration. Runtime resolution remains strict v3.
/// Existing v3 is an idempotent no-op; v2 is staged beside config then
/// promoted with the existing Windows write-through replacement helper.
fn migrate_workspace_config(path: &Path) -> Result<WorkspaceConfigMigrationReceipt, String> {
    let bytes = std::fs::read(path).map_err(|_| "workspace_config_unreadable".to_string())?;
    if let Ok(config) = serde_json::from_slice::<WorkspaceConfigV3>(&bytes) {
        if config.schema_version != WORKSPACE_SCHEMA_VERSION {
            return Err("workspace_config_schema_unsupported".into());
        }
        let root = validated_workspace_root(config.workspace_root)?;
        return Ok(workspace_migration_receipt(root, false));
    }
    let legacy: WorkspaceConfigV2 = serde_json::from_slice(&bytes)
        .map_err(|_| "workspace_config_invalid".to_string())?;
    if legacy.schema_version != 2 || !legacy.python_executable.is_absolute() {
        return Err("workspace_config_schema_unsupported".into());
    }
    let root = validated_workspace_root(legacy.workspace_root)?;
    let encoded = serde_json::to_vec(&WorkspaceConfigV3Output {
        schema_version: WORKSPACE_SCHEMA_VERSION,
        workspace_root: &root,
    })
    .map_err(|_| "workspace_config_invalid".to_string())?;
    let parent = path.parent().ok_or_else(|| "workspace_config_invalid".to_string())?;
    let staged = parent.join(format!(".workspace-{}.tmp", std::process::id()));
    if let Err(error) = std::fs::write(&staged, encoded) {
        return Err(format!("stage workspace config migration: {error}"));
    }
    if let Err(error) = replace_file(&staged, path) {
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    Ok(workspace_migration_receipt(root, true))
}

fn validated_workspace_root(root: PathBuf) -> Result<PathBuf, String> {
    if !root.is_absolute() {
        return Err("workspace_root_invalid".into());
    }
    let canonical = std::fs::canonicalize(&root)
        .map_err(|_| "workspace_root_invalid".to_string())?;
    canonical.is_dir()
        .then_some(root)
        .ok_or_else(|| "workspace_root_invalid".to_string())
}

fn workspace_migration_receipt(root: PathBuf, migrated: bool) -> WorkspaceConfigMigrationReceipt {
    WorkspaceConfigMigrationReceipt {
        schema_version: WORKSPACE_MIGRATION_RECEIPT_SCHEMA_VERSION,
        migration: WORKSPACE_MIGRATION_NAME.to_string(),
        workspace_root: root,
        migrated,
    }
}

fn migration_config_path() -> Result<Option<PathBuf>, String> {
    let explicit = std::env::var_os("MEMBRANE_WORKSPACE_CONFIG")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty());
    if let Some(path) = explicit {
        if !path.is_absolute() {
            return Err("workspace_config_invalid".into());
        }
        return Ok(Some(path));
    }
    Ok({
        #[cfg(windows)]
        let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
        #[cfg(not(windows))]
        let home = std::env::var_os("HOME").map(PathBuf::from);
        home.map(|home| home.join(".config/membrane/workspace.json"))
    })
}

fn workspace_migration_for_activation(
    dry_run: bool,
) -> Result<Option<WorkspaceConfigMigrationReceipt>, String> {
    let Some(path) = migration_config_path()? else { return Ok(None) };
    workspace_migration_at_path(&path, dry_run)
}

fn workspace_migration_at_path(path: &Path, dry_run: bool) -> Result<Option<WorkspaceConfigMigrationReceipt>, String> {
    if !path.is_file() { return Ok(None) }
    if dry_run {
        // Inspection must never rewrite config. Report only already-v3 state;
        // a v2 file remains pending for a real activation.
        let bytes = std::fs::read(&path)
            .map_err(|_| "workspace_config_unreadable".to_string())?;
        if let Ok(config) = serde_json::from_slice::<WorkspaceConfigV3>(&bytes) {
            if config.schema_version != WORKSPACE_SCHEMA_VERSION {
                return Err("workspace_config_schema_unsupported".into());
            }
            return Ok(Some(workspace_migration_receipt(
                validated_workspace_root(config.workspace_root)?, false,
            )));
        }
        let legacy: WorkspaceConfigV2 = serde_json::from_slice(&bytes)
            .map_err(|_| "workspace_config_invalid".to_string())?;
        if legacy.schema_version != 2 || !legacy.python_executable.is_absolute() {
            return Err("workspace_config_schema_unsupported".into());
        }
        let _ = validated_workspace_root(legacy.workspace_root)?;
        return Ok(None);
    }
    migrate_workspace_config(&path).map(Some)
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
    /// The host's enabled native plugin owns the MCP binding; the global
    /// registration is absent (or was removed, when `removed` is true).
    PluginOwned { removed: bool },
}

impl ClientState {
    fn label(&self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::Absent => "absent",
            Self::AlreadyCorrect => "already_correct",
            Self::Conflict(_) => "conflict",
            Self::PluginOwned { .. } => "plugin_owned",
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
    NotReady { installation_id: String },
    Ready { release_generation: String, installation_id: String },
    PriorGeneration { release_generation: String, installation_id: String },
    Foreign(String),
}

struct ActivationLock {
    path: PathBuf,
    // Kernel-backed guard serializes stale-owner observation, quarantine, and
    // publication across concurrent activators. The guard file is permanent;
    // its lock lifetime is released by the OS on drop/crash.
    _guard: std::fs::File,
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

/// Verify caller identity against the installer-owned stable `current` tree.
/// Qualification controls must never turn a source checkout or synthetic
/// executable into installed evidence.
pub fn verified_installed_identity() -> Result<serde_json::Value, String> {
    let executable = std::env::current_exe().map_err(|e| format!("current executable: {e}"))?;
    let current = executable.parent().ok_or_else(|| "current executable has no parent".to_string())?;
    let stable = expected_stable_install_root()?;
    if !paths_equal(&current.to_string_lossy(), &stable.to_string_lossy()) {
        return Err("caller executable is not under installer-owned stable current".into());
    }
    let release_path = current.join("release.json");
    let release: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&release_path).map_err(|e| format!("read release identity: {e}"))?,
    ).map_err(|e| format!("parse release identity: {e}"))?;
    let version = release.get("version").and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "release identity lacks non-empty version".to_string())?;
    let files = release.get("files").and_then(serde_json::Value::as_object)
        .ok_or_else(|| "release identity lacks files map".to_string())?;
    let bytes = std::fs::read(&executable).map_err(|e| format!("read current executable: {e}"))?;
    let digest = hex::encode(sha2::Sha256::digest(&bytes));
    let expected = files.get("membrane.exe")
        .and_then(serde_json::Value::as_str).ok_or_else(|| "release identity lacks membrane.exe digest".to_string())?;
    if !expected.eq_ignore_ascii_case(&digest) {
        return Err("current executable digest does not match release identity".into());
    }
    let mut verified_files = Vec::with_capacity(files.len());
    for (relative, expected) in files {
        let relative_path = Path::new(relative);
        if relative_path.is_absolute() || relative_path.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
            return Err(format!("release identity contains unsafe file path {relative}"));
        }
        let path = current.join(relative_path);
        let bytes = std::fs::read(&path).map_err(|e| format!("read installed file {relative}: {e}"))?;
        let actual = hex::encode(sha2::Sha256::digest(&bytes));
        if expected.as_str().is_none_or(|value| !value.eq_ignore_ascii_case(&actual)) {
            return Err(format!("installed file digest does not match release identity: {relative}"));
        }
        verified_files.push(serde_json::json!({"path": path, "sha256": actual}));
    }
    Ok(serde_json::json!({"current":current,"executable":executable,"release":release_path,"version":version,"executableSha256":digest,"files":verified_files}))
}

pub fn activate(options: ActivationOptions) -> Result<ActivationReceiptV1, String> {
    activate_with_residency(options, true)
}

/// Reconcile installed explicit entry points without starting resident services.
pub fn activate_bindings(options: ActivationOptions) -> Result<ActivationReceiptV1, String> {
    activate_with_residency(options, false)
}

pub fn activate_engine(options: ActivationOptions) -> Result<ActivationReceiptV1, String> {
    activate_internal(options, true, false)
}

fn activate_with_residency(options: ActivationOptions, start_resident: bool) -> Result<ActivationReceiptV1, String> {
    activate_internal(options, start_resident, true)
}

fn activate_internal(options: ActivationOptions, start_resident: bool, reconcile_bindings: bool) -> Result<ActivationReceiptV1, String> {
    let (install_root, version_root) = validate_installed_root(&options.install_root)?;
    let product_root = install_root
        .parent()
        .ok_or_else(|| "stable installed path has no product root".to_string())?;
    let membrane = install_root.join(executable_name("membrane"));
    let membrane_client = install_root.join(executable_name("membrane-client"));
    let runtime_membrane = version_root.join(executable_name("membrane"));
    let runtime_client = version_root.join(executable_name("membrane-client"));
    require_file(&runtime_membrane, "resolved membrane executable")?;
    require_file(&runtime_client, "resolved membrane client executable")?;
    let (workspace_root, port) = installed_runtime(product_root)?;
    // Migration is installer/activation-owned and happens before state,
    // locking, health, client, or resident activation effects.
    let workspace_config_migration =
        workspace_migration_for_activation(options.dry_run)?;
    let expected_generation = membrane_runtime::release_identity::release_generation();
    if !options.dry_run {
        std::fs::create_dir_all(&workspace_root).map_err(|error| {
            format!(
                "create installed runtime state {}: {error}",
                workspace_root.display()
            )
        })?;
        // Installation identity & API credential are the installed engine's
        // canonical state. Activation may run before the engine's first start
        // on a fresh install, so prepare both through the same owner paths the
        // resident uses rather than assuming a prior run created them.
        let identity_path = membrane_runtime::installation_identity::InstallationPaths::defaults_for_workspace(&workspace_root).identity;
        if let Some(parent) = identity_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!("create installed identity directory {}: {error}", parent.display())
            })?;
        }
        membrane_runtime::installation_identity::load_or_create_installation(&identity_path, &[])
            .map_err(|error| format!("prepare installed identity {}: {error}", identity_path.display()))?;
        membrane_runtime::serve::prepare_installed_credential_for_exe(&membrane)?;
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
    let initial = constrain_health_to_existing_installation(initial, &workspace_root);
    let already_running = matches!(&initial, HealthObservation::Ready { .. });
    // Explicit installed access survives a failed resident startup. Bind MCP
    // clients & the CLI path before attempting any automatic Hub process, so
    // an unverified/foreign listener on the Membrane port cannot suppress
    // explicit binding reconciliation (docs/architecture/execution-lifecycle-boundary.md).
    //
    // Native plugin projection runs first: when a Codex/Claude plugin is
    // enabled it owns SessionStart hooks & plugin-scoped MCP, and the global
    // fallback bindings for that host are removed rather than duplicated.
    let plugin_states = if reconcile_bindings {
        reconcile_host_plugins(&install_root, &options.clients, options.dry_run, run_client)?
    } else {
        BTreeMap::new()
    };
    let mut clients = if reconcile_bindings { reconcile_clients(
        &membrane_client, &options.clients, options.dry_run, run_client, &plugin_states,
    )? } else { Vec::new() };
    for receipt in &mut clients {
        receipt.plugin = plugin_states.get(&receipt.client).cloned();
    }
    if reconcile_bindings && !options.dry_run {
        ensure_user_path(&install_root)?;
    }
    if start_resident && !options.dry_run && matches!(&initial, HealthObservation::Foreign(_)) {
        return Err(match initial {
            HealthObservation::Foreign(reason) => format!(
                "refusing to activate against unverified service on Membrane port: {reason}"
            ),
            _ => unreachable!(),
        });
    }
    let (release_generation, service_state, service_reason) = if options.dry_run || !start_resident {
        match initial {
            HealthObservation::Ready { release_generation, .. } => {
                (release_generation, "ready".to_string(), None)
            }
            HealthObservation::PriorGeneration { release_generation, .. } => (
                release_generation,
                "stale_generation".to_string(),
                Some("resident release generation differs from installed current".to_string()),
            ),
            HealthObservation::Unavailable => (
                expected_generation.clone(),
                "unavailable".to_string(),
                Some("installed Membrane is not running".to_string()),
            ),
            HealthObservation::NotReady { .. } => (
                expected_generation.clone(),
                "not_ready".to_string(),
                Some("installed Membrane is not healthy".to_string()),
            ),
            HealthObservation::Foreign(reason) => {
                (expected_generation.clone(), "foreign".to_string(), Some(reason))
            }
        }
    } else if let HealthObservation::Ready { release_generation, .. } = &initial {
        (release_generation.clone(), "ready".to_string(), None)
    } else {
        // The installed engine is the sole resident owner. Tray/Hub is only
        // an optional status surface and must never be part of startup.
        reset_supervision_before_resident_launch(product_root)?;
        launch_engine(&membrane, &workspace_root, port)?;
        (
            wait_for_health(port, &expected_generation, options.timeout)?,
            "ready".to_string(),
            None,
        )
    };

    if reconcile_bindings && !options.dry_run {
        provision_mcp_credential(product_root)?;
        // An enabled native plugin already projects SessionStart & lifecycle
        // hooks from the installed payload. In that case the settings-level
        // fallback hooks are removed so exactly one hook source exists; the
        // fallback stays when the plugin is not enabled.
        if plugin_state_is(&plugin_states, HarnessClient::Claude, "enabled") {
            remove_claude_hooks(&install_root, false)?;
        } else {
            reconcile_claude_hooks(&install_root)?;
        }
        if plugin_state_is(&plugin_states, HarnessClient::Codex, "enabled") {
            let _ = remove_codex_hooks(&install_root, false)?;
        } else {
            reconcile_codex_hooks(&install_root)?;
        }
    }
    let receipt = ActivationReceiptV1 {
        schema_version: ACTIVATION_RECEIPT_SCHEMA_VERSION,
        runtime_origin: RuntimeOrigin::Installed,
        install_root: install_root.clone(),
        version_root,
        membrane_executable: membrane,
        tray_executable: install_root.join(executable_name("membrane-tray")),
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
        activation_scope: match (start_resident, reconcile_bindings) {
            (true, true) => "full",
            (true, false) => "engine_only",
            (false, true) => "bindings_only",
            (false, false) => "engine_only",
        }
        .to_string(),
        workspace_config_migration,
    };
    if !options.dry_run {
        persist_receipt(&workspace_root, &receipt)?;
    }
    Ok(receipt)
}

pub fn deactivate(options: ActivationOptions) -> Result<DeactivationReceiptV1, String> {
    deactivate_with_residency(options, true)
}

/// Reconcile installed explicit entry points without touching resident services.
pub fn deactivate_bindings(options: ActivationOptions) -> Result<DeactivationReceiptV1, String> {
    deactivate_with_residency(options, false)
}

fn deactivate_with_residency(
    options: ActivationOptions,
    stop_resident: bool,
) -> Result<DeactivationReceiptV1, String> {
    let (install_root, version_root) = validate_installed_root(&options.install_root)?;
    let product_root = install_root
        .parent()
        .ok_or_else(|| "stable installed path has no product root".to_string())?;
    let membrane = install_root.join(executable_name("membrane"));
    let membrane_client = install_root.join(executable_name("membrane-client"));
    let tray = install_root.join(executable_name("membrane-tray"));
    require_file(
        &version_root.join(executable_name("membrane")),
        "resolved membrane executable",
    )?;
    require_file(
        &version_root.join(executable_name("membrane-client")),
        "resolved membrane client executable",
    )?;
    require_file(
        &version_root.join(executable_name("membrane-tray")),
        "resolved tray executable",
    )?;
    let (workspace_root, port) = installed_runtime(product_root)?;
    let expected_generation = membrane_runtime::release_identity::release_generation();
    let initial = if stop_resident {
        match probe_health(port, &expected_generation) {
            Ok(observation) => observation,
            Err(error) => HealthObservation::Foreign(error),
        }
    } else {
        HealthObservation::Unavailable
    };
    let initial = constrain_health_to_existing_installation(initial, &workspace_root);
    let before = health_label(&initial).to_string();
    if stop_resident && !options.dry_run {
        if let HealthObservation::Foreign(reason) = &initial {
            return Err(format!(
                "refusing to deactivate unverified service on Membrane port: {reason}"
            ));
        }
    }
    let would_stop = stop_resident && matches!(
        &initial,
        HealthObservation::NotReady { .. }
            | HealthObservation::Ready { .. }
            | HealthObservation::PriorGeneration { .. }
    );

    let _lock = (stop_resident && !options.dry_run)
        .then(|| acquire_lock(product_root))
        .transpose()?;
    if !options.dry_run && would_stop {
        // Explicit-stop semantics: this operator-requested shutdown is not a
        // crash. Mark supervision clean before terminating the engine so the
        // next supervised start does not count it toward suppression.
        // Dry runs never touch supervision state.
        crate::supervision::mark_clean(product_root)
            .map_err(|error| format!("mark explicit stop clean: {error}"))?;
        request_resident_replacement(&tray, &workspace_root, port)?;
        wait_for_shutdown(port, &expected_generation, options.timeout)?;
        // Port-quiet is not handle-quiet: a dying resident closes its
        // listener before a large process image finishes tearing down, and
        // Windows refuses to replace an executable (or a loaded DLL) with
        // open handles. Wait for the installed tree to become replaceable
        // (bounded) so the installer's extract phase cannot race a lingering
        // image — and so rapid one-shot probes cannot hold files open
        // across the cutover either.
        wait_for_tree_release(&version_root, options.timeout)?;
    }

    let mut clients = deactivate_clients(&membrane_client, &options.clients, options.dry_run, run_client)?;
    // Detach native plugin projections we own (bounded to `membrane@membrane`
    // bound to this install root). Installed payloads & marketplace pointers
    // remain so re-activation is cheap; uninstall owns full removal.
    let plugin_states =
        deactivate_host_plugins(&install_root, &options.clients, options.dry_run, run_client)?;
    for receipt in &mut clients {
        receipt.plugin = plugin_states.get(&receipt.client).cloned();
    }
    let claude_hooks_matched = remove_claude_hooks(&install_root, options.dry_run)?;
    let _ = remove_codex_hooks(&install_root, options.dry_run)?;
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
        HealthObservation::NotReady { .. } => "not_ready",
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
        HealthObservation::Ready { release_generation, .. } => Ok(release_generation),
        HealthObservation::PriorGeneration { release_generation, .. } => Err(format!(
            "resident release generation {release_generation} does not match installed generation {expected_generation}"
        )),
        HealthObservation::Foreign(reason) => Err(reason),
        HealthObservation::Unavailable => Err("installed Membrane is not running".to_string()),
        HealthObservation::NotReady { .. } => Err("installed Membrane is not healthy".to_string()),
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
    acquire_lock_with_wait(install_root, LOCK_WAIT)
}

fn acquire_lock_with_wait(install_root: &Path, wait: Duration) -> Result<ActivationLock, String> {
    acquire_lock_with_policy(install_root, wait, LOCK_STALE_AFTER)
}

/// Acquire activation lock with an explicit stale threshold.
///
/// Production callers use [`LOCK_STALE_AFTER`] through
/// `acquire_lock_with_wait`.  Keeping threshold injection here lets the
/// installed qualification exercise stale-owner recovery without waiting
/// ninety seconds or weakening production policy.
fn acquire_lock_with_policy(
    install_root: &Path,
    wait: Duration,
    stale_after: Duration,
) -> Result<ActivationLock, String> {
    let path = install_root.join(LOCK_DIR);
    let deadline = Instant::now() + wait;
    let guard_path = install_root.join(".activation.guard");
    let guard = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&guard_path)
        .map_err(|error| format!("open activation guard: {error}"))?;
    loop {
        if lock_activation_guard(&guard).is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            return Err("activation startup lock remained busy".to_string());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    loop {
        match std::fs::create_dir(&path) {
            Ok(()) => {
                // The owner pid decides whether a later activation may break
                // this lock, so a write that fails must not leave the lock
                // looking ownerless — that is the state that gets it broken.
                let owner = path.join("owner");
                if let Err(error) = std::fs::write(&owner, format!("{}\n", std::process::id())) {
                    let _ = std::fs::remove_dir_all(&path);
                    return Err(format!("record activation lock owner: {error}"));
                }
                return Ok(ActivationLock { path, _guard: guard });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = std::fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| modified.elapsed().ok())
                    .is_some_and(|age| age > stale_after);
                // Age alone is not evidence that the holder is gone. Breaking
                // on age meant a slow-but-live activation had its lock taken
                // away and a second one ran against the same install — and
                // activations do exceed this window: one measured on Windows
                // ran for minutes. The owner pid was already recorded here and
                // never consulted; consult it, and fall back to age only when
                // there is no owner to ask about.
                // A recorded, dead owner is definitive even when its lock is
                // young.  An absent owner is different: it can be the small
                // publication window between mkdir and owner write, so age
                // remains required before reclaiming that state.
                let reclaim = match activation_owner_state(&path) {
                    Some(alive) => !alive,
                    None => stale,
                };
                if reclaim {
                    // Rename first: remove_dir_all(path) could erase a new
                    // owner's replacement lock between observation and delete.
                    let quarantine = install_root.join(format!(
                        "{LOCK_DIR}.reclaim-{}-{}",
                        std::process::id(), now_unix_ms()
                    ));
                    if std::fs::rename(&path, &quarantine).is_ok() {
                        let _ = std::fs::remove_dir_all(quarantine);
                        continue;
                    }
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

fn reset_supervision_before_resident_launch(product_root: &Path) -> Result<(), String> {
    crate::supervision::reset(product_root)
        .map_err(|error| format!("reset supervision before resident launch: {error}"))
}

#[cfg(unix)]
fn lock_activation_guard(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let result = unsafe { flock(file.as_raw_fd(), 2 | 4) };
    (result == 0).then_some(()).ok_or_else(std::io::Error::last_os_error)
}

#[cfg(unix)]
unsafe extern "C" {
    fn flock(fd: std::os::raw::c_int, operation: std::os::raw::c_int) -> std::os::raw::c_int;
}

#[cfg(windows)]
fn lock_activation_guard(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    let mut overlapped = std::mem::MaybeUninit::<OVERLAPPED>::zeroed();
    let ok = unsafe { LockFileEx(file.as_raw_handle() as _, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, u32::MAX, u32::MAX, overlapped.as_mut_ptr()) };
    (ok != 0).then_some(()).ok_or_else(std::io::Error::last_os_error)
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
    fn LockFileEx(file: *mut std::ffi::c_void, flags: u32, reserved: u32, low: u32, high: u32, overlapped: *mut OVERLAPPED) -> i32;
}

/// Is the process that recorded itself as this lock's owner still running?
///
/// Missing metadata is unknown. A process-query denial is conservatively live;
/// only a missing or exited process proves that a recorded owner is dead.
fn activation_owner_state(lock: &Path) -> Option<bool> {
    let Ok(text) = std::fs::read_to_string(lock.join("owner")) else {
        return None;
    };
    let Ok(pid) = text.trim().parse::<u32>() else {
        return None;
    };
    if pid == 0 {
        return None;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER};
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return Some(unsafe { GetLastError() } != ERROR_INVALID_PARAMETER);
        }
        let mut exit_code = 0;
        let queried = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
        unsafe { CloseHandle(handle) };
        Some(queried == 0 || exit_code == 259) // STILL_ACTIVE
    }
    #[cfg(not(windows))]
    {
        Some(std::path::Path::new(&format!("/proc/{pid}")).exists())
    }
}

fn launch_engine(
    engine: &Path,
    workspace_root: &Path,
    port: u16,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        return launch_engine_detached(engine, workspace_root, port);
    }
    #[cfg(not(windows))]
    {
    let mut command = Command::new(engine);
    command
        .env("MEMBRANE_RUNTIME_ORIGIN", "installed")
        .env_remove("MEMBRANE_CONFIG_ROOT")
        .env_remove("MEMBRANE_DATA_ROOT")
        .env_remove("MEMBRANE_CACHE_ROOT")
        .env_remove("MEMBRANE_LOG_ROOT")
        .env("MEMBRANE_STATE_ROOT", workspace_root)
        .env("MEMBRANE_PORT", port.to_string())
        .env("MEMBRANE_HTTP_PORT", port.to_string());
    // DETACHED_PROCESS drops console but not inherited stdio handles. Redirect
    // engine output to installer log so activation never waits on child pipe.
    command
        .stdin(Stdio::null())
        .stdout(engine_log_target())
        .stderr(engine_log_target());
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("launch installed engine {}: {error}", engine.display()))
    }
}

#[cfg(windows)]
/// Start the installed engine directly, detached from the activation caller.
/// Lifetime ownership (decisions 21/24, 2026-09-13): there is no independent
/// autostart and no per-minute restart task. The engine runs while a Hub/tray
/// owner or an authorized harness holder keeps it alive; crash recovery
/// belongs to a surviving owner (tray child supervision, holder reactivation)
/// and never to the OS scheduler. `--os-supervised` marks this parented launch
/// (console detach + supervision-state bookkeeping) without implying that any
/// OS-level restart lane exists.
fn launch_engine_detached(engine: &Path, workspace_root: &Path, port: u16) -> Result<(), String> {
    let mut command = Command::new(engine);
    command
        .arg("--os-supervised")
        .env("MEMBRANE_RUNTIME_ORIGIN", "installed")
        .env_remove("MEMBRANE_CONFIG_ROOT")
        .env_remove("MEMBRANE_DATA_ROOT")
        .env_remove("MEMBRANE_CACHE_ROOT")
        .env_remove("MEMBRANE_LOG_ROOT")
        .env("MEMBRANE_STATE_ROOT", workspace_root)
        .env("MEMBRANE_PORT", port.to_string())
        .env("MEMBRANE_HTTP_PORT", port.to_string());
    // Detached console with diagnostics to the engine log so activation never
    // waits on a child pipe and failures stay diagnosable from disk.
    command
        .stdin(Stdio::null())
        .stdout(engine_log_target())
        .stderr(engine_log_target());
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP.
    command.creation_flags(0x0800_0000 | 0x0000_0200 | 0x0000_0008);
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("launch installed engine {}: {error}", engine.display()))
}

/// Best-effort authenticated stop of the running installed engine through its
/// own service. Used by explicit deactivation after the scheduled-task lane is
/// retired: an ownerless `/End` cannot address a tray- or activation-launched
/// engine, and the operator's verified health pre-check owns foreign refusal.
fn stop_installed_engine_service() -> Result<(), String> {
    use membrane_client::{
        build_loopback_request_headers, LoopbackAuthSigner, LoopbackIdentityFields,
    };
    use membrane_protocol::{
        ResidentControllerIdentityV1, ResidentHolderCredentialV1, ResidentHolderOperationV1,
        ResidentHolderRequestV1, ResidentHolderResponseV1, RESIDENT_HOLDER_SCHEMA_VERSION,
    };

    let port = INSTALLED_PORT;
    let probe = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(500),
    );
    if probe.is_err() {
        // Nothing is listening: the engine is already stopped.
        return Ok(());
    }
    // Engine exe -> `current` -> product root (same derivation as supervision).
    let executable = std::env::current_exe().map_err(|e| format!("current executable: {e}"))?;
    let install_root = executable
        .parent()
        .ok_or_else(|| "current executable has no parent".to_string())?;
    let product_root = install_root
        .parent()
        .ok_or_else(|| "install root has no product root".to_string())?;
    let token_path = product_root.join("state/tools/.cache/memory/api-token");
    let token = std::fs::read_to_string(&token_path)
        .map_err(|error| format!("read installed credential: {error}"))?
        .trim()
        .to_string();
    // Identity comes from the engine's own signed health exchange; the engine
    // re-verifies controller identity server-side, so a mismatched request is
    // rejected rather than trusted.
    let health = membrane_runtime::installed_health::probe_installed(
        port,
        &token,
        Duration::from_secs(3),
        &token_path,
    )
    .map_err(|error| format!("engine identity unavailable: {error}"))?;
    let identity = LoopbackIdentityFields {
        installation_id: health.identity.installation_id.clone(),
        cortex_store_id: health.identity.cortex_store_id.clone(),
        release_generation: health.identity.release_generation.clone(),
        startup_generation: health.identity.startup_generation,
        stable_install_root: health.identity.stable_install_root.clone(),
    };
    let controller = ResidentControllerIdentityV1 {
        installation_id: identity.installation_id.clone(),
        cortex_store_id: identity.cortex_store_id.clone(),
        release_generation: identity.release_generation.clone(),
        startup_generation: identity.startup_generation,
        stable_current: identity.stable_install_root.clone(),
    };
    let holder = ResidentHolderCredentialV1 {
        holder_kind: "hub".to_string(),
        holder_id: "membrane-deactivate".to_string(),
        credential_id: "membrane-deactivate-credential".to_string(),
    };
    // A holderless Release cannot stop an ownerless engine: the registry has
    // nothing to release. Deactivation therefore acquires its own bounded
    // lifetime holder first (existing V1 semantics), then releases it as the
    // final owner so the engine drains and exits.
    let acquire = ResidentHolderRequestV1 {
        schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
        operation: ResidentHolderOperationV1::Acquire,
        controller: controller.clone(),
        holder: Some(holder.clone()),
        expires_at_unix_ms: Some(now_unix_ms().saturating_add(5_000)),
        observed_at_unix_ms: now_unix_ms(),
        loss_cursor: None,
    };
    let acquire_body = serde_json::to_vec(&acquire)
        .map_err(|error| format!("serialize holder acquire: {error}"))?;
    let outcome = signed_post(
        port,
        &identity,
        &token,
        "/resident-holder",
        &acquire_body,
        Duration::from_secs(5),
    )?;
    let acquire_response: ResidentHolderResponseV1 = serde_json::from_slice(&outcome.body)
        .map_err(|error| format!("resident-holder acquire response invalid: {error}"))?;
    if !acquire_response.status.controller_active {
        return Err("resident-holder acquire did not establish controller".to_string());
    }
    let release = ResidentHolderRequestV1 {
        schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
        operation: ResidentHolderOperationV1::Release,
        controller,
        holder: Some(holder),
        expires_at_unix_ms: None,
        observed_at_unix_ms: now_unix_ms(),
        loss_cursor: None,
    };
    let release_body = serde_json::to_vec(&release)
        .map_err(|error| format!("serialize holder release: {error}"))?;
    let outcome = signed_post(
        port,
        &identity,
        &token,
        "/resident-holder",
        &release_body,
        Duration::from_secs(5),
    )?;
    let release_response: ResidentHolderResponseV1 = serde_json::from_slice(&outcome.body)
        .map_err(|error| format!("resident-holder release response invalid: {error}"))?;
    if release_response.status.controller_active {
        // A surviving lifetime owner legitimately holds the engine; the
        // caller's wait_for_shutdown is authoritative on whether the
        // listener actually went quiet.
        return Err("engine still held by a surviving owner".to_string());
    }
    Ok(())
}

/// One signed loopback POST. Response signatures are not verified here: this
/// helper only decides whether to proceed toward the caller's authoritative
/// `wait_for_shutdown` port check, and every request is signed with the
/// installed credential.
fn signed_post(
    port: u16,
    identity: &membrane_client::LoopbackIdentityFields,
    token: &str,
    path: &str,
    body: &[u8],
    timeout: Duration,
) -> Result<SignedPostOutcome, String> {
    use membrane_client::build_loopback_request_headers;
    let signer = membrane_client::LoopbackAuthSigner::from_hex_token(token)
        .map_err(|_| "loopback auth signer invalid".to_string())?;
    let now = now_unix_ms() / 1000;
    let expiry = membrane_client::LoopbackAuthSigner::bounded_expiry(now, 10);
    let nonce = membrane_client::LoopbackAuthSigner::generate_nonce()
        .map_err(|_| "loopback nonce unavailable".to_string())?;
    let headers = build_loopback_request_headers(
        &signer, identity, "POST", path, "127.0.0.1", "application/json", body, nonce, expiry,
    )
    .map_err(|error| format!("sign request: {error}"))?;
    let mut request = format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str(&format!("content-length: {}\r\n\r\n", body.len()));
    let mut stream = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        timeout,
    )
    .map_err(|error| format!("connect engine: {error}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| format!("read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| format!("write timeout: {error}"))?;
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("write request: {error}"))?;
    stream
        .write_all(body)
        .map_err(|error| format!("write body: {error}"))?;
    let mut raw = Vec::new();
    let _ = std::io::Read::read_to_end(&mut stream, &mut raw);
    let text = String::from_utf8_lossy(&raw);
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "engine response invalid".to_string())?;
    let body_start = text
        .find("\r\n\r\n")
        .map(|index| index + 4)
        .unwrap_or(raw.len());
    Ok(SignedPostOutcome {
        success: (200..300).contains(&status),
        status,
        body: raw[body_start.min(raw.len())..].to_vec(),
    })
}

struct SignedPostOutcome {
    success: bool,
    status: u16,
    body: Vec<u8>,
}

#[cfg(windows)]
fn provision_mcp_credential(product_root: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ,
    };
    let token_path = product_root.join("state/tools/.cache/memory/api-token");
    let token = std::fs::read_to_string(&token_path)
        .map_err(|error| format!("read installed MCP credential: {error}"))?;
    let token = token.trim();
    if token.is_empty() { return Err("installed MCP credential is empty".into()); }
    let wide = |value: &std::ffi::OsStr| value.encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let key_name = wide(std::ffi::OsStr::new("Environment"));
    let value_name = wide(std::ffi::OsStr::new(MCP_TOKEN_ENV));
    let value = wide(std::ffi::OsStr::new(token));
    let mut key: HKEY = std::ptr::null_mut();
    let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key_name.as_ptr(), 0, KEY_SET_VALUE, &mut key) };
    if opened != 0 { return Err(format!("open user MCP credential store: {opened}")); }
    let written = unsafe { RegSetValueExW(key, value_name.as_ptr(), 0, REG_SZ, value.as_ptr() as *const u8, (value.len() * 2) as u32) };
    let legacy_name = wide(std::ffi::OsStr::new("MEMBRANE_API_TOKEN"));
    unsafe { RegDeleteValueW(key, legacy_name.as_ptr()) };
    unsafe { RegCloseKey(key) };
    if written != 0 { return Err(format!("write user MCP credential: {written}")); }
    Ok(())
}

/// Login-shell env file hosts inherit: `~/.zshenv` on macOS (every zsh,
/// including non-interactive host spawns), `~/.profile` elsewhere. Writes a
/// managed block so foreign content is preserved and rewrites are idempotent.
#[cfg(not(windows))]
fn posix_env_file() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("MEMBRANE_TEST_USER_BINDINGS_ROOT")
        .filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(root).join("profile"));
    }
    let name = if cfg!(target_os = "macos") { ".zshenv" } else { ".profile" };
    Ok(host_home()?.join(name))
}

#[cfg(not(windows))]
fn managed_block_bounds(name: &str) -> (String, String) {
    (
        format!("# >>> membrane {name} >>>"),
        format!("# <<< membrane {name} <<<"),
    )
}

#[cfg(not(windows))]
fn write_managed_block(path: &Path, name: &str, body: &str) -> Result<bool, String> {
    let (begin, end) = managed_block_bounds(name);
    let block = format!("{begin}\n{body}\n{end}\n");
    let current = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    let next = match (current.find(&begin), current.find(&end)) {
        (Some(start), Some(stop)) if start <= stop => {
            let after = stop + end.len();
            let mut merged = String::with_capacity(current.len() + block.len());
            merged.push_str(&current[..start]);
            merged.push_str(&block);
            merged.push_str(current[after..].strip_prefix('\n').unwrap_or(&current[after..]));
            merged
        }
        _ => {
            let mut merged = current.clone();
            if !merged.is_empty() && !merged.ends_with('\n') {
                merged.push('\n');
            }
            merged.push_str(&block);
            merged
        }
    };
    if next == current {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    std::fs::write(path, &next).map_err(|error| format!("write {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(true)
}

#[cfg(not(windows))]
fn remove_managed_block(path: &Path, name: &str, dry_run: bool) -> Result<bool, String> {
    let (begin, end) = managed_block_bounds(name);
    let current = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    let (Some(start), Some(stop)) = (current.find(&begin), current.find(&end)) else {
        return Ok(false);
    };
    if start > stop {
        return Ok(false);
    }
    if dry_run {
        return Ok(true);
    }
    let after = stop + end.len();
    let mut next = String::with_capacity(current.len());
    next.push_str(&current[..start]);
    next.push_str(current[after..].strip_prefix('\n').unwrap_or(&current[after..]));
    std::fs::write(path, next).map_err(|error| format!("write {}: {error}", path.display()))?;
    Ok(true)
}

#[cfg(not(windows))]
fn provision_mcp_credential(product_root: &Path) -> Result<(), String> {
    let token_path = product_root.join("state/tools/.cache/memory/api-token");
    let token = std::fs::read_to_string(&token_path)
        .map_err(|error| format!("read installed MCP credential: {error}"))?;
    let token = token.trim();
    if token.is_empty() {
        return Err("installed MCP credential is empty".into());
    }
    #[cfg(target_os = "macos")]
    {
        // Durable store: user login keychain. `-w` would place the token in
        // argv, so the value is supplied on stdin to the interactive prompt.
        let account = std::env::var("USER").unwrap_or_else(|_| "membrane".to_string());
        let mut child = Command::new("security")
            .args(["add-generic-password", "-U", "-s", "membrane-mcp-token", "-a"])
            .arg(&account)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("spawn security credential store: {error}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(format!("{token}\n{token}\n").as_bytes());
        }
        let status = child
            .wait()
            .map_err(|error| format!("wait security credential store: {error}"))?;
        if !status.success() {
            return Err("write user keychain MCP credential".to_string());
        }
    }
    // Shell-visible export so hosts launched from a login shell resolve the
    // token on next sign-in. The value is written to the user-owned env file
    // (mode 0600), never to argv, logs, or receipts.
    let profile = posix_env_file()?;
    write_managed_block(
        &profile,
        "mcp credential",
        &format!("export {MCP_TOKEN_ENV}=\"{token}\""),
    )?;
    Ok(())
}

/// Legacy stop request used during explicit deactivation. Normal startup never
/// launches tray; installed tray remains a bounded lifecycle control surface.
fn request_resident_replacement(
    tray: &Path,
    workspace_root: &Path,
    port: u16,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = (tray, workspace_root, port);
        // Decisions 21/24: no per-minute engine task exists anymore. Stop the
        // running engine through its own authenticated service (ownerless
        // `/End` cannot address a tray- or activation-launched engine), then
        // remove any legacy task left by pre-cutover installs. Both steps are
        // best effort: the caller's verified health pre-check owns foreign
        // refusal, and the task may simply not exist.
        let _ = stop_installed_engine_service();
        use std::os::windows::process::CommandExt;
        let _ = Command::new("schtasks.exe")
            .args(["/Delete", "/TN", "Membrane Engine", "/F"])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        return Ok(());
    }
    #[cfg(not(windows))]
    {
    let mut command = Command::new(tray);
    command
        .arg("--replace")
        .env("MEMBRANE_RUNTIME_ORIGIN", "installed")
        .env_remove("MEMBRANE_CONFIG_ROOT")
        .env_remove("MEMBRANE_DATA_ROOT")
        .env_remove("MEMBRANE_CACHE_ROOT")
        .env_remove("MEMBRANE_LOG_ROOT")
        .env("MEMBRANE_STATE_ROOT", workspace_root)
        .env("MEMBRANE_PORT", port.to_string())
        .env("MEMBRANE_HTTP_PORT", port.to_string())
        .stdin(Stdio::null())
        .stdout(engine_log_target())
        .stderr(engine_log_target());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000 | 0x0000_0200 | 0x0000_0008);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("request resident stop through tray {}: {error}", tray.display()))
    }
}

/// Append resident engine output to `membrane-engine.log` under Windows log
/// root. Fall back to discarded stream if log setup fails.
fn engine_log_target() -> Stdio {
    let Some(root) = std::env::var_os("MEMBRANE_LOG_ROOT")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("Membrane"))
        })
    else {
        return Stdio::null();
    };
    if std::fs::create_dir_all(&root).is_err() {
        return Stdio::null();
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("membrane-engine.log"))
        .map(Stdio::from)
        .unwrap_or(Stdio::null())
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
            HealthObservation::NotReady { .. }
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

/// Wait until every file under the installed version tree can be opened for
/// writing (bounded), i.e. no lingering process — resident or rapid one-shot
/// probe — still holds an image or DLL handle that would make the
/// installer's extract phase fail. Fails closed on timeout so deactivation
/// never reports a stop it did not fully achieve. Missing files are already
/// replaceable; running images and loaded DLLs deny write sharing on
/// Windows, which is exactly the signal probed here.
fn wait_for_tree_release(root: &Path, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match first_locked_file(root) {
            None => return Ok(()),
            Some(path) if Instant::now() >= deadline => {
                return Err(format!(
                    "installed tree still locked after {}ms: {}",
                    timeout.as_millis(),
                    path.display()
                ));
            }
            _ => std::thread::sleep(POLL_INTERVAL),
        }
    }
}

fn first_locked_file(root: &Path) -> Option<PathBuf> {
    let current_image = std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::canonicalize(path).ok());
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(metadata) = std::fs::symlink_metadata(&current) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let Ok(entries) = std::fs::read_dir(&current) else {
                continue;
            };
            stack.extend(entries.filter_map(|entry| entry.ok().map(|entry| entry.path())));
            continue;
        }
        if metadata.is_file() && std::fs::OpenOptions::new().write(true).open(&current).is_err() {
            let is_self_image = current_image
                .as_ref()
                .and_then(|image| std::fs::canonicalize(&current).ok().map(|path| (image, path)))
                .is_some_and(|(image, path)| paths_equal(&image.to_string_lossy(), &path.to_string_lossy()));
            if !(is_self_image && current_process_is_only_image_holder(&current)) {
                return Some(current);
            }
        }
    }
    None
}

#[cfg(windows)]
fn current_process_is_only_image_holder(path: &Path) -> bool {
    use std::os::windows::process::CommandExt;
    let escaped = path.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$p='{}'; @(Get-CimInstance Win32_Process | Where-Object {{$_.ExecutablePath -eq $p}} | Select-Object -ExpandProperty ProcessId)",
        escaped
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000)
        .output();
    let Ok(output) = output else { return false; };
    if !output.status.success() { return false; }
    let ids: Vec<u32> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect();
    only_current_process_image_holder(&ids, std::process::id())
}

#[cfg(not(windows))]
fn current_process_is_only_image_holder(_path: &Path) -> bool { false }

fn only_current_process_image_holder(process_ids: &[u32], current_pid: u32) -> bool {
    process_ids == [current_pid]
}

fn wait_for_health(
    port: u16,
    expected_generation: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!(
                "installed Membrane did not become healthy within {}ms",
                timeout.as_millis()
            ));
        }
        match probe_health_with_timeout(port, expected_generation, remaining)? {
            HealthObservation::Ready { release_generation, .. } => return Ok(release_generation),
            HealthObservation::Foreign(reason) => return Err(reason),
            HealthObservation::Unavailable
            | HealthObservation::NotReady { .. }
            | HealthObservation::PriorGeneration { .. } => {}
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn probe_health(port: u16, expected_generation: &str) -> Result<HealthObservation, String> {
    probe_health_with_timeout(port, expected_generation, Duration::from_secs(2))
}

fn probe_health_with_timeout(
    port: u16,
    expected_generation: &str,
    timeout: Duration,
) -> Result<HealthObservation, String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + timeout;
    let connect_timeout = timeout.min(Duration::from_millis(400));
    let stream = match TcpStream::connect_timeout(&address, connect_timeout) {
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
    drop(stream);
    let current = expected_stable_install_root()?;
    let product_root = current.parent().ok_or("installed current has no product root")?;
    let token_path = product_root.join("state/tools/.cache/memory/api-token");
    let token = std::fs::read_to_string(&token_path)
        .map_err(|_| "installed health credential is unavailable".to_string())?;
    let remaining = deadline.checked_duration_since(Instant::now())
        .ok_or_else(|| "installed health deadline expired".to_string())?;
    let response = membrane_runtime::installed_health::probe_installed(port, token.trim(), remaining, &token_path)
        .map_err(|reason| format!("installed health proof failed: {reason}"))?;
    let mut raw = format!("HTTP/1.1 {} Health\r\nContent-Length: {}\r\n\r\n",
        response.status, response.body.len()).into_bytes();
    raw.extend_from_slice(&response.body);
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
    let Some(installation_id) = body
        .get("installationId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Ok(HealthObservation::Foreign(
            "Membrane health omitted installation identity".to_string(),
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
            installation_id: installation_id.to_string(),
        });
    }
    if runtime_origin != Some("installed") {
        return Ok(HealthObservation::Foreign(
            "Membrane health omitted installed runtime origin".to_string(),
        ));
    }
    let database_status = body
        .pointer("/database/status")
        .and_then(serde_json::Value::as_str);
    let catalog_ok = body
        .get("catalog")
        .and_then(serde_json::Value::as_object)
        .and_then(|catalog| catalog.get("status"))
        .and_then(serde_json::Value::as_str)
        == Some("ok");
    let enrolled_repo_count = body
        .get("enrolledRepoCount")
        .and_then(serde_json::Value::as_u64);
    let watcher_running = body
        .get("watcherRunning")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let blueprint_unconfigured = body
        .pointer("/blueprintWatcher/watcherState")
        .and_then(serde_json::Value::as_str)
        == Some("not_configured")
        && enrolled_repo_count == Some(0)
        && body
            .pointer("/blueprintWatcher/watcherDetail")
            .is_none_or(serde_json::Value::is_null);
    let blueprint_running = watcher_running
        && enrolled_repo_count.is_some_and(|count| count > 0)
        && body.pointer("/blueprintWatcher/watcherState").and_then(serde_json::Value::as_str) == Some("running")
        && body.pointer("/blueprintWatcher/watcherReady").and_then(serde_json::Value::as_bool) == Some(true)
        && body.pointer("/blueprintWatcher/watcherDetail").is_some_and(serde_json::Value::is_null);
    let watcher_partial_coverage = enrolled_repo_count.is_some_and(|count| count > 0)
        && body.pointer("/blueprintWatcher/watcherState").and_then(serde_json::Value::as_str) == Some("running")
        && body.pointer("/blueprintWatcher/watcherDetail").is_some_and(serde_json::Value::is_null);
    let background_not_required = body
        .pointer("/backgroundAuthority/active")
        .and_then(serde_json::Value::as_bool)
        == Some(false)
        && (!watcher_running || watcher_partial_coverage)
        && body.pointer("/blueprintWatcher/watcherDetail").is_none_or(serde_json::Value::is_null);
    let database_usable = matches!(database_status, Some("ok" | "empty"));
    let activation_ready_degraded = status == 503
        && body.get("ok").and_then(serde_json::Value::as_bool) == Some(false)
        && catalog_ok
        && database_usable
        && (blueprint_running || blueprint_unconfigured || background_not_required);
    if (status != 200 && !activation_ready_degraded)
        || (body.get("ok").and_then(serde_json::Value::as_bool) != Some(true)
            && !activation_ready_degraded)
    {
        return Ok(HealthObservation::NotReady {
            installation_id: installation_id.to_string(),
        });
    }
    Ok(HealthObservation::Ready {
        release_generation: release_generation.to_string(),
        installation_id: installation_id.to_string(),
    })
}

fn constrain_health_to_existing_installation(
    observation: HealthObservation,
    workspace_root: &Path,
) -> HealthObservation {
    let observed = match &observation {
        HealthObservation::NotReady { installation_id }
        | HealthObservation::Ready { installation_id, .. }
        | HealthObservation::PriorGeneration { installation_id, .. } => installation_id,
        HealthObservation::Unavailable | HealthObservation::Foreign(_) => return observation,
    };
    let paths = membrane_runtime::installation_identity::InstallationPaths::for_workspace(workspace_root);
    let expected = match std::fs::read(&paths.identity)
        .map_err(|error| format!("read installation identity {}: {error}", paths.identity.display()))
        .and_then(|bytes| {
            serde_json::from_slice::<membrane_runtime::installation_identity::InstallationIdentity>(&bytes)
                .map_err(|error| format!("parse installation identity {}: {error}", paths.identity.display()))
        })
    {
        Ok(identity) if !identity.installation_id.trim().is_empty() => identity.installation_id,
        Ok(_) => return HealthObservation::Foreign("installed installation identity is empty".to_string()),
        Err(error) => return HealthObservation::Foreign(error),
    };
    if observed == &expected {
        observation
    } else {
        HealthObservation::Foreign("service installation identity does not match installed identity".to_string())
    }
}

fn installed_runtime(product_root: &Path) -> Result<(PathBuf, u16), String> {
    if std::env::var("MEMBRANE_RUNTIME_ORIGIN").ok().as_deref() == Some("development") {
        return Err("development runtime cannot perform installed activation".to_string());
    }
    // Installed state is product-owned and deliberately independent from any
    // checkout, workspace config, or repository runtime manifest.
    let port = if cfg!(debug_assertions) {
        match std::env::var("MEMBRANE_TEST_INSTALLED_PORT") {
            Ok(value) => value
                .parse::<u16>()
                .ok()
                .filter(|port| *port != 0)
                .ok_or_else(|| "MEMBRANE_TEST_INSTALLED_PORT must be a nonzero u16".to_string())?,
            Err(_) => INSTALLED_PORT,
        }
    } else {
        INSTALLED_PORT
    };
    Ok((product_root.join("state"), port))
}

#[cfg(windows)]
fn isolated_user_bindings_root() -> Option<PathBuf> {
    cfg!(debug_assertions)
        .then(|| std::env::var_os("MEMBRANE_TEST_USER_BINDINGS_ROOT"))
        .flatten()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(windows)]
fn ensure_isolated_user_path(root: &Path, install_root: &Path) -> Result<(), String> {
    let path = root.join("Environment.Path");
    let current = match std::fs::read_to_string(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("read isolated user PATH {}: {error}", path.display())),
    };
    let stable = install_root.to_string_lossy().trim_end_matches(['\\', '/']).to_string();
    let legacy = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|root| root.join("Membrane Hub").to_string_lossy().to_string());
    let mut entries = current
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| !legacy.as_ref().is_some_and(|legacy| value.trim_matches('"').eq_ignore_ascii_case(legacy)))
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !entries.iter().any(|value| value.trim_matches('"').eq_ignore_ascii_case(&stable)) {
        entries.push(stable);
    }
    let updated = entries.join(";");
    if updated != current {
        std::fs::create_dir_all(root)
            .map_err(|error| format!("create isolated user bindings {}: {error}", root.display()))?;
        std::fs::write(&path, updated)
            .map_err(|error| format!("write isolated user PATH {}: {error}", path.display()))?;
    }
    Ok(())
}

#[cfg(windows)]
fn ensure_user_path(install_root: &Path) -> Result<(), String> {
    if let Some(root) = isolated_user_bindings_root() {
        return ensure_isolated_user_path(&root, install_root);
    }
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
fn ensure_user_path(install_root: &Path) -> Result<(), String> {
    let profile = posix_env_file()?;
    let stable = install_root
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_string();
    write_managed_block(
        &profile,
        "path",
        &format!("export PATH=\"{stable}\":\"$PATH\""),
    )?;
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
    if let Some(root) = isolated_user_bindings_root() {
        let path = root.join("Environment.Path");
        let current = match std::fs::read_to_string(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("read isolated user PATH {}: {error}", path.display())),
        };
        let (updated, removed) = without_path_entry(&current, install_root);
        if removed && !dry_run {
            std::fs::write(&path, updated)
                .map_err(|error| format!("write isolated user PATH {}: {error}", path.display()))?;
        }
        return Ok(removed);
    }
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
fn remove_user_path(_install_root: &Path, dry_run: bool) -> Result<bool, String> {
    let profile = posix_env_file()?;
    remove_managed_block(&profile, "path", dry_run)
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
    if isolated_user_bindings_root().is_some() {
        return Ok(0);
    }
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
    reconcile_claude_hooks_at(&settings_path, install_root)
}

/// Reconcile Codex's native hook projection independently from its MCP
/// registration.  Only exact Membrane commands are replaced/removed; all
/// foreign hook entries remain byte-for-byte represented in the merged tree.
fn codex_hooks_path() -> Result<PathBuf, String> {
    if let Some(home) = std::env::var_os("CODEX_HOME").filter(|home| !home.is_empty()) {
        return Ok(PathBuf::from(home).join("hooks.json"));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from).ok_or_else(|| "Codex settings profile is unavailable".to_string())?;
    Ok(home.join(".codex").join("hooks.json"))
}

fn reconcile_codex_hooks(install_root: &Path) -> Result<(), String> {
    reconcile_codex_hooks_at(&codex_hooks_path()?, install_root)
}

// Verified native lifecycle contracts: Claude Code hooks reference & Codex
// hooks reference, 2026-09-12. Unsupported host events are never projected.
const CLAUDE_HOOK_EVENTS: &[&str] = &[
    "SessionStart", "UserPromptSubmit", "PreCompact", "PostCompact", "PreToolUse",
    "PostToolUse", "PostToolUseFailure", "Stop", "TaskCompleted", "SessionEnd",
];
const CODEX_HOOK_EVENTS: &[&str] = &[
    "SessionStart", "UserPromptSubmit", "PreCompact", "PostCompact", "PreToolUse",
    "PostToolUse", "Stop", "SessionEnd",
];

fn reconcile_owned_hook_events(
    hooks: &mut serde_json::Map<String, serde_json::Value>,
    events: &[&str], command: &str, legacy_command: &str, codex: bool,
) -> Result<(), String> {
    for &event in events {
        let entries = hooks.entry(event).or_insert_with(|| serde_json::json!([])).as_array_mut()
            .ok_or_else(|| format!("{event} hooks must be an array"))?;
        replace_legacy_hook_commands(entries, command);
        // Remove the current installed command (including stale
        // matchers/timeouts) and the obsolete engine-direct command from the
        // transport migration. Recreate one complete owned group; foreign
        // items/metadata survive.
        entries.retain_mut(|entry| {
            if let Some(items) = entry.get_mut("hooks").and_then(serde_json::Value::as_array_mut) {
                let before = items.len();
                items.retain(|item| {
                    let current = item.get("command").and_then(serde_json::Value::as_str);
                    current != Some(command) && current != Some(legacy_command)
                });
                if before != items.len() && items.is_empty() {
                    return !entry.as_object().is_some_and(|object| object.keys().all(|key| matches!(key.as_str(), "hooks" | "matcher")));
                }
            }
            true
        });
        let mut handler = serde_json::json!({"type":"command", "command":command,
            "timeout": if event == "SessionEnd" { 3 } else { 20 }});
        if codex && matches!(event, "SessionStart" | "UserPromptSubmit" | "PreToolUse" | "PostToolUse") {
            handler["additionalContextLimit"] = serde_json::json!(2500);
        }
        entries.push(serde_json::json!({"hooks":[handler]}));
    }
    Ok(())
}

fn reconcile_codex_hooks_at(path: &Path, install_root: &Path) -> Result<(), String> {
    require_file(&install_root.join(executable_name("membrane-client")), "installed transport Codex hook executable")?;
    let command = installed_hook_command(install_root);
    let legacy = legacy_engine_hook_command(install_root);
    let mut config: serde_json::Value = if path.is_file() {
        serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("read Codex hooks: {e}"))?)
            .map_err(|e| format!("parse Codex hooks: {e}"))?
    } else { serde_json::json!({}) };
    let root = config.as_object_mut().ok_or_else(|| "Codex hooks root must be an object".to_string())?;
    let hooks = root.entry("hooks").or_insert_with(|| serde_json::json!({})).as_object_mut()
        .ok_or_else(|| "Codex hooks must be an object".to_string())?;
    reconcile_owned_hook_events(hooks, CODEX_HOOK_EVENTS, &command, &legacy, true)?;
    let parent = path.parent().ok_or_else(|| "Codex hooks path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create Codex hooks directory: {e}"))?;
    let staged = path.with_extension(format!("json.{}.partial", std::process::id()));
    std::fs::write(&staged, serde_json::to_vec_pretty(&config).map_err(|e| format!("serialize Codex hooks: {e}"))?)
        .map_err(|e| format!("stage Codex hooks: {e}"))?;
    replace_file(&staged, path).map_err(|e| format!("promote Codex hooks: {e}"))
}

fn remove_codex_hooks(install_root: &Path, dry_run: bool) -> Result<usize, String> {
    let path = codex_hooks_path()?;
    if !path.is_file() { return Ok(0); }
    let original = std::fs::read(&path).map_err(|e| format!("read Codex hooks: {e}"))?;
    let mut config: serde_json::Value = serde_json::from_slice(&original).map_err(|e| format!("parse Codex hooks: {e}"))?;
    let removed = remove_exact_hook_items(&mut config, &installed_hook_command(install_root))
        + remove_exact_hook_items(&mut config, &legacy_engine_hook_command(install_root));
    if removed == 0 || dry_run { return Ok(removed); }
    let staged = path.with_extension(format!("json.{}.partial", std::process::id()));
    std::fs::write(&staged, serde_json::to_vec_pretty(&config).map_err(|e| format!("serialize Codex hooks: {e}"))?)
        .map_err(|e| format!("stage Codex hooks: {e}"))?;
    replace_file(&staged, &path).map_err(|e| format!("promote Codex hooks: {e}"))?;
    Ok(removed)
}

/// Reconcile native Claude hooks at an explicit settings path.
///
/// The production path supplies the user's real Claude settings path through
/// `reconcile_claude_hooks`; LC-06 supplies an isolated path so it can verify
/// the same read, merge, atomic-write, and read-back behavior without touching
/// operator configuration.
fn reconcile_claude_hooks_at(settings_path: &Path, install_root: &Path) -> Result<(), String> {
    let mut settings: serde_json::Value = if settings_path.is_file() {
        serde_json::from_slice(
            &std::fs::read(&settings_path)
                .map_err(|error| format!("read Claude settings {}: {error}", settings_path.display()))?,
        )
        .map_err(|error| format!("parse Claude settings {}: {error}", settings_path.display()))?
    } else {
        serde_json::json!({})
    };
    require_file(
        &install_root.join(executable_name("membrane-client")),
        "installed transport Claude hook executable",
    )?;
    let command = installed_hook_command(install_root);
    let legacy = legacy_engine_hook_command(install_root);
    let root = settings
        .as_object_mut()
        .ok_or_else(|| "Claude settings root must be an object".to_string())?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Claude settings hooks must be an object".to_string())?;
    reconcile_owned_hook_events(hooks, CLAUDE_HOOK_EVENTS, &command, &legacy, false)?;
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

/// Ordinary hook dispatch lives in the resident engine behind `/hook`; hosts
/// invoke it through the transport-only `membrane-client`, which forwards one
/// HookHost envelope per process and never constructs runtime state. The
/// engine binary is never a host hook binding.
fn installed_hook_command(install_root: &Path) -> String {
    let client = install_root.join(executable_name("membrane-client"));
    format!("\"{}\" hook", client.display())
}

/// Obsolete host binding from before hook dispatch moved behind the resident
/// `/hook` route. Reconcile removes it wherever the current client command is
/// installed; deactivation removes both.
fn legacy_engine_hook_command(install_root: &Path) -> String {
    let membrane = install_root.join(executable_name("membrane"));
    format!("\"{}\" hook", membrane.display())
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
    remove_claude_hooks_at(&settings_path, install_root, dry_run)
}

fn remove_claude_hooks_at(
    settings_path: &Path,
    install_root: &Path,
    dry_run: bool,
) -> Result<usize, String> {
    if !settings_path.is_file() {
        return Ok(0);
    }
    let original = std::fs::read(&settings_path)
        .map_err(|error| format!("read Claude settings {}: {error}", settings_path.display()))?;
    let mut settings: serde_json::Value = serde_json::from_slice(&original)
        .map_err(|error| format!("parse Claude settings {}: {error}", settings_path.display()))?;
    let removed = remove_exact_hook_items(&mut settings, &installed_hook_command(install_root))
        + remove_exact_hook_items(&mut settings, &legacy_engine_hook_command(install_root));
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
                    || value.contains("membrane-hook-entrypoint.mjs")
                    || (value.contains(".venv-tools") && value.to_ascii_lowercase().contains("membrane"))
            });
            if owned {
                *command = serde_json::Value::String(expected.to_string());
            }
        }
    }
}

fn uses_config_file(client: HarnessClient) -> bool {
    matches!(
        client,
        HarnessClient::Cursor
            | HarnessClient::Windsurf
            | HarnessClient::Antigravity
            | HarnessClient::Devin
    )
}

fn uses_http_transport(client: HarnessClient) -> bool {
    matches!(client, HarnessClient::Claude | HarnessClient::Codex)
}

fn installed_mcp_url() -> String {
    let port = std::env::var("MEMBRANE_TEST_INSTALLED_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port >= 1024)
        .unwrap_or(INSTALLED_PORT);
    format!("http://127.0.0.1:{port}{MCP_HTTP_PATH}")
}

fn expected_client_config(client: HarnessClient, executable: &str) -> ServerConfig {
    if uses_http_transport(client) {
        ServerConfig {
            command: installed_mcp_url(),
            args: Vec::new(),
        }
    } else {
        ServerConfig {
            command: executable.to_string(),
            args: vec!["stdio-mcp".to_string()],
        }
    }
}

fn config_matches_expected(client: HarnessClient, config: &ServerConfig, executable: &str) -> bool {
    let expected = expected_client_config(client, executable);
    paths_equal(&config.command, &expected.command) && config.args == expected.args
}

fn registration_command(client: HarnessClient, executable: &str) -> String {
    if uses_http_transport(client) {
        installed_mcp_url()
    } else {
        executable.to_string()
    }
}

fn client_config_path(client: HarnessClient) -> Result<PathBuf, String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| "user home directory is unavailable".to_string())?;
    match client {
        HarnessClient::Cursor => Ok(home.join(".cursor").join("mcp.json")),
        HarnessClient::Windsurf => Ok(home.join(".codeium").join("windsurf").join("mcp_config.json")),
        HarnessClient::Antigravity => Ok(home.join(".gemini").join("config").join("mcp_config.json")),
        HarnessClient::Devin => {
            // Devin reads its user-scoped MCP registry from
            // %APPDATA%\devin\mcp_config.json on Windows and
            // ~/.config/devin/mcp_config.json elsewhere. The `devin mcp add`
            // CLI need not exist, so the config file is the owned surface.
            #[cfg(windows)]
            {
                std::env::var_os("APPDATA")
                    .map(|base| PathBuf::from(base).join("devin").join("mcp_config.json"))
                    .ok_or_else(|| "APPDATA is unavailable".to_string())
            }
            #[cfg(not(windows))]
            {
                Ok(home.join(".config").join("devin").join("mcp_config.json"))
            }
        }
        HarnessClient::Codex | HarnessClient::Claude => Err("command-managed client has no config path".to_string()),
    }
}

fn read_client_config(path: &Path) -> Result<(Option<Vec<u8>>, serde_json::Value), String> {
    if !path.exists() {
        return Ok((None, serde_json::json!({ "mcpServers": {} })));
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("inspect client config {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("client config {} is not a regular file", path.display()));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("read client config {}: {error}", path.display()))?;
    if std::str::from_utf8(&bytes).is_ok_and(|text| text.trim().is_empty()) {
        return Ok((Some(bytes), serde_json::json!({})));
    }
    let value = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|error| format!("parse client config {}: {error}", path.display()))?;
    if !value.is_object() {
        return Err(format!("client config {} must be an object", path.display()));
    }
    Ok((Some(bytes), value))
}

fn config_client_state(value: &serde_json::Value, executable: &str) -> Result<ClientState, String> {
    let Some(servers) = value.get("mcpServers") else {
        return Ok(ClientState::Absent);
    };
    let servers = servers.as_object().ok_or_else(|| "client mcpServers must be an object".to_string())?;
    let Some(entry) = servers.get("membrane") else {
        return Ok(ClientState::Absent);
    };
    let config = parse_prior_config(&entry.to_string())
        .ok_or_else(|| "client has a membrane entry that cannot be safely reconciled".to_string())?;
    if paths_equal(&config.command, executable) && config.args == ["stdio-mcp".to_string()] {
        Ok(ClientState::AlreadyCorrect)
    } else {
        Ok(ClientState::Conflict(config))
    }
}

fn config_with_membrane(mut value: serde_json::Value, executable: &str) -> Result<serde_json::Value, String> {
    let object = value.as_object_mut().ok_or_else(|| "client config must be an object".to_string())?;
    let servers = object.entry("mcpServers").or_insert_with(|| serde_json::json!({}))
        .as_object_mut().ok_or_else(|| "client mcpServers must be an object".to_string())?;
    servers.insert("membrane".to_string(), serde_json::json!({ "command": executable, "args": ["stdio-mcp"] }));
    Ok(value)
}

fn config_without_owned_membrane(mut value: serde_json::Value, executable: &str) -> Result<(serde_json::Value, bool), String> {
    let Some(servers) = value.get_mut("mcpServers").and_then(serde_json::Value::as_object_mut) else {
        return Ok((value, false));
    };
    let owned = servers.get("membrane").is_some_and(|entry| {
        parse_prior_config(&entry.to_string()).is_some_and(|config| {
            paths_equal(&config.command, executable) && config.args == ["stdio-mcp".to_string()]
        })
    });
    if owned { servers.remove("membrane"); }
    Ok((value, owned))
}

fn write_client_config(path: &Path, original: Option<&[u8]>, value: &serde_json::Value) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "client config has no parent".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create client config directory {}: {error}", parent.display()))?;
    let current = if path.exists() {
        Some(std::fs::read(path).map_err(|error| format!("re-read client config {}: {error}", path.display()))?)
    } else { None };
    if current.as_deref() != original {
        return Err(format!("client config {} changed during activation", path.display()));
    }
    let staged = path.with_extension(format!("json.{}.partial", std::process::id()));
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| format!("serialize client config: {error}"))?;
    bytes.push(b'\n');
    std::fs::write(&staged, bytes)
        .map_err(|error| format!("stage client config {}: {error}", staged.display()))?;
    replace_file(&staged, path).map_err(|error| format!("promote client config {}: {error}", path.display()))
}

/// Put one client config back the way it was, reporting whether it worked.
///
/// A rollback that fails leaves the operator's editor configuration modified
/// by an activation that then reported only the original error — the config
/// was changed and nothing said so.
fn restore_client_config(path: &Path, original: Option<&[u8]>) -> Result<(), String> {
    match original {
        Some(bytes) => {
            let staged = path.with_extension(format!("json.{}.rollback", std::process::id()));
            std::fs::write(&staged, bytes)
                .map_err(|error| format!("stage rollback for {}: {error}", path.display()))?;
            replace_file(&staged, path)
        }
        None => std::fs::remove_file(path)
            .map_err(|error| format!("remove {}: {error}", path.display())),
    }
}

fn reconcile_config_clients(executable: &str, clients: &[HarnessClient], dry_run: bool) -> Result<Vec<ClientActivationReceipt>, String> {
    let mut inspected = Vec::new();
    for &client in clients {
        let path = client_config_path(client)?;
        let (original, value) = read_client_config(&path)?;
        let state = config_client_state(&value, executable)?;
        inspected.push((client, path, original, value, state));
    }
    if dry_run {
        return Ok(inspected.into_iter().map(|(client, _, _, _, state)| ClientActivationReceipt {
            client, before: state.label().to_string(), after: state.label().to_string(), changed: false, plugin: None,
        }).collect());
    }
    let mut completed: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    let mut receipts = Vec::new();
    for (client, path, original, value, state) in inspected {
        let changed = matches!(state, ClientState::Absent | ClientState::Conflict(_));
        if changed {
            let next = config_with_membrane(value, executable)?;
            if let Err(error) = write_client_config(&path, original.as_deref(), &next) {
                let mut unrestored = Vec::new();
                for (done_path, done_original) in completed.iter().rev() {
                    if let Err(failure) = restore_client_config(done_path, done_original.as_deref())
                    {
                        unrestored.push(failure);
                    }
                }
                if unrestored.is_empty() {
                    return Err(error);
                }
                return Err(format!(
                    "{error}; and rollback left client configuration modified: {}",
                    unrestored.join("; ")
                ));
            }
            completed.push((path, original));
        }
        receipts.push(ClientActivationReceipt {
            client, before: state.label().to_string(), after: if changed { "installed" } else { state.label() }.to_string(), changed, plugin: None,
        });
    }
    Ok(receipts)
}

fn deactivate_config_clients(executable: &str, clients: &[HarnessClient], dry_run: bool) -> Result<Vec<ClientActivationReceipt>, String> {
    let mut receipts = Vec::new();
    for &client in clients {
        let path = client_config_path(client)?;
        let (original, value) = read_client_config(&path)?;
        let (next, owned) = config_without_owned_membrane(value, executable)?;
        if owned && !dry_run { write_client_config(&path, original.as_deref(), &next)?; }
        receipts.push(ClientActivationReceipt {
            client,
            before: if owned { "owned" } else { "preserved" }.to_string(),
            after: if owned && !dry_run { "removed" } else if owned { "owned" } else { "preserved" }.to_string(),
            changed: owned && !dry_run,
            plugin: None,
        });
    }
    Ok(receipts)
}

// ---------------------------------------------------------------------------
// Native plugin projection (Codex & Claude).
//
// When a host's native plugin surface is enabled it owns SessionStart and
// lifecycle hooks (`hooks/hooks.json` inside the plugin payload) plus its
// plugin-scoped MCP binding (`.mcp.json` for Codex). Activation reconciles
// plugin state through the host CLI first and then re-reads host state files,
// so receipts report what the host recorded rather than what commands ran.
// `enabled` is the only state where the plugin projection is proven current;
// every other state names the remaining step.
// ---------------------------------------------------------------------------

const PLUGIN_ID: &str = "membrane@membrane";

#[derive(Debug, Clone, PartialEq, Eq)]
enum PluginProjection {
    /// Installed, enabled, and the host cache holds the plugin payload at the
    /// installed payload version.
    Enabled { version: String },
    /// Host records the plugin but it is not provably enabled at the current
    /// payload version (disabled, stale cache, or unverifiable payload).
    Disabled { detail: Option<String> },
    Absent,
    /// A `membrane` plugin or marketplace id exists but is bound to a source
    /// other than this install root; never mutate it.
    Conflict(String),
}

fn plugin_state_is(
    states: &BTreeMap<HarnessClient, PluginActivationReceipt>,
    client: HarnessClient,
    state: &str,
) -> bool {
    states
        .get(&client)
        .is_some_and(|receipt| receipt.state == state)
}

fn plugin_receipt(
    client: HarnessClient,
    state: &str,
    version: Option<String>,
    changed: bool,
    detail: Option<String>,
) -> PluginActivationReceipt {
    PluginActivationReceipt {
        state: state.to_string(),
        plugin_id: if client.has_plugin_surface() {
            Some(PLUGIN_ID.to_string())
        } else {
            None
        },
        version,
        changed,
        detail,
    }
}

fn host_home() -> Result<PathBuf, String> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .ok_or_else(|| "user home directory is unavailable".to_string())
}

fn codex_home_dir() -> Result<PathBuf, String> {
    if let Some(home) = std::env::var_os("CODEX_HOME").filter(|home| !home.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    Ok(host_home()?.join(".codex"))
}

fn claude_config_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    Ok(host_home()?.join(".claude"))
}

/// Plugin payload version stamped into the installed plugin manifests.
fn payload_plugin_version(install_root: &Path) -> Option<String> {
    for rel in [
        ".codex-plugin/plugin.json",
        ".claude-plugin/plugin.json",
    ] {
        if let Ok(bytes) = std::fs::read(install_root.join(rel)) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(version) = value.get("version").and_then(|v| v.as_str()) {
                    if !version.is_empty() {
                        return Some(version.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Read the first `key = value` inside a `[table]` of a small TOML document.
/// Bounded to the Codex plugin-state layout; nested tables under the header
/// are treated as part of the table.
fn toml_table_value(document: &str, table: &str, key: &str) -> Option<String> {
    let header = format!("[{table}]");
    let mut in_table = false;
    for line in document.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_table = line == header || line.starts_with(&format!("{header}."));
            continue;
        }
        if !in_table || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim().trim_matches('"').trim_matches('\'') == key {
                return Some(v.trim().trim_matches('"').trim_matches('\'').to_string());
            }
        }
    }
    None
}

/// A plugin marketplace source path matches this install root after
/// normalization; a git/URL source or foreign path is a conflict.
fn plugin_source_matches(source: &str, install_root: &Path) -> bool {
    let root = install_root.to_string_lossy();
    paths_equal(source, &root)
        || paths_equal(
            source.trim_end_matches(['/', '\\']),
            root.trim_end_matches(['/', '\\']),
        )
}

fn codex_plugin_state(install_root: &Path, payload_version: Option<&str>) -> PluginProjection {
    let Ok(codex_home) = codex_home_dir() else {
        return PluginProjection::Absent;
    };
    let Ok(document) = std::fs::read_to_string(codex_home.join("config.toml")) else {
        return PluginProjection::Absent;
    };
    let marketplace = toml_table_value(&document, "marketplaces.membrane", "source").or_else(|| {
        toml_table_value(&document, "marketplaces.\"membrane\"", "source")
    });
    if let Some(source) = &marketplace {
        if !plugin_source_matches(source, install_root) {
            return PluginProjection::Conflict(format!(
                "codex marketplace `membrane` is bound to {source}"
            ));
        }
    }
    let enabled = toml_table_value(&document, "plugins.\"membrane@membrane\"", "enabled")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"));
    if !enabled {
        return if marketplace.is_some()
            || codex_home
                .join("plugins/cache/membrane/membrane")
                .is_dir()
        {
            PluginProjection::Disabled {
                detail: Some("plugin recorded but not enabled".to_string()),
            }
        } else {
            PluginProjection::Absent
        };
    }
    let Some(version) = payload_version else {
        return PluginProjection::Disabled {
            detail: Some("installed payload plugin version is unreadable".to_string()),
        };
    };
    let cache = codex_home
        .join("plugins")
        .join("cache")
        .join("membrane")
        .join("membrane")
        .join(version);
    if !cache.join(".codex-plugin").join("plugin.json").is_file() {
        return PluginProjection::Disabled {
            detail: Some(format!(
                "plugin enabled but host cache has no payload at version {version}"
            )),
        };
    }
    PluginProjection::Enabled {
        version: version.to_string(),
    }
}

fn claude_plugin_state(install_root: &Path, payload_version: Option<&str>) -> PluginProjection {
    let Ok(claude_dir) = claude_config_dir() else {
        return PluginProjection::Absent;
    };
    // A `membrane` marketplace bound elsewhere must never be touched.
    for relative in [
        "plugins/known_marketplaces.json",
        "settings.json",
    ] {
        let Ok(bytes) = std::fs::read(claude_dir.join(relative)) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let candidates = [
            value.pointer("/membrane/source"),
            value.pointer("/extraKnownMarketplaces/membrane/source"),
        ];
        for source in candidates.into_iter().flatten() {
            let path = source
                .get("path")
                .and_then(|p| p.as_str())
                .or_else(|| source.as_str());
            if let Some(path) = path {
                if !plugin_source_matches(path, install_root) {
                    return PluginProjection::Conflict(format!(
                        "claude marketplace `membrane` is bound to {path}"
                    ));
                }
            }
        }
    }
    let installed_path = claude_dir.join("plugins").join("installed_plugins.json");
    let Ok(bytes) = std::fs::read(&installed_path) else {
        return PluginProjection::Absent;
    };
    let Ok(installed) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return PluginProjection::Conflict(
            "installed_plugins.json is not parseable".to_string(),
        );
    };
    let entries = installed
        .get("plugins")
        .and_then(|plugins| plugins.get(PLUGIN_ID))
        .and_then(|entries| entries.as_array());
    let Some(entries) = entries.filter(|entries| !entries.is_empty()) else {
        return PluginProjection::Absent;
    };
    let entry = entries
        .iter()
        .find(|entry| {
            payload_version
                .is_none_or(|v| entry.get("version").and_then(|x| x.as_str()) == Some(v))
        })
        .or_else(|| entries.last());
    let Some(entry) = entry else {
        return PluginProjection::Absent;
    };
    let version = entry
        .get("version")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let payload_ok = entry
        .get("installPath")
        .and_then(|p| p.as_str())
        .map(PathBuf::from)
        .is_some_and(|p| p.join(".claude-plugin").join("plugin.json").is_file());
    let settings = std::fs::read(claude_dir.join("settings.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let enabled = settings
        .as_ref()
        .and_then(|s| s.get("enabledPlugins"))
        .and_then(|e| e.get(PLUGIN_ID))
        .is_some_and(|v| v.as_bool() == Some(true));
    if !enabled {
        return PluginProjection::Disabled {
            detail: Some("plugin recorded but not enabled".to_string()),
        };
    }
    match (payload_ok, version.as_deref(), payload_version) {
        (true, Some(version), Some(expected)) if version == expected => {
            PluginProjection::Enabled {
                version: version.to_string(),
            }
        }
        (true, Some(version), _) => PluginProjection::Disabled {
            detail: Some(format!(
                "plugin cache is at version {version}; reactivation required"
            )),
        },
        _ => PluginProjection::Disabled {
            detail: Some("plugin enabled but host payload is not verifiable".to_string()),
        },
    }
}

fn plugin_projection(
    client: HarnessClient,
    install_root: &Path,
    payload_version: Option<&str>,
) -> PluginProjection {
    match client {
        HarnessClient::Codex => codex_plugin_state(install_root, payload_version),
        HarnessClient::Claude => claude_plugin_state(install_root, payload_version),
        _ => PluginProjection::Absent,
    }
}

fn plugin_install_steps(client: HarnessClient, install_root: &Path) -> Vec<Vec<String>> {
    let root = install_root.to_string_lossy().into_owned();
    match client {
        HarnessClient::Codex => vec![
            vec![
                "plugin".to_string(),
                "marketplace".to_string(),
                "add".to_string(),
                root,
            ],
            vec![
                "plugin".to_string(),
                "add".to_string(),
                PLUGIN_ID.to_string(),
                "--json".to_string(),
            ],
        ],
        HarnessClient::Claude => vec![
            vec![
                "plugin".to_string(),
                "marketplace".to_string(),
                "add".to_string(),
                root,
            ],
            vec![
                "plugin".to_string(),
                "install".to_string(),
                PLUGIN_ID.to_string(),
                "--scope".to_string(),
                "user".to_string(),
                "--yes".to_string(),
            ],
            vec![
                "plugin".to_string(),
                "enable".to_string(),
                PLUGIN_ID.to_string(),
            ],
        ],
        _ => Vec::new(),
    }
}

fn plugin_disable_args(client: HarnessClient) -> Vec<String> {
    match client {
        HarnessClient::Codex => {
            vec!["plugin", "remove", PLUGIN_ID]
                .into_iter()
                .map(str::to_string)
                .collect()
        }
        HarnessClient::Claude => {
            vec!["plugin", "disable", PLUGIN_ID]
                .into_iter()
                .map(str::to_string)
                .collect()
        }
        _ => Vec::new(),
    }
}

fn plugin_manual_steps(client: HarnessClient, install_root: &Path) -> String {
    plugin_install_steps(client, install_root)
        .iter()
        .map(|args| format!("{} {}", client.as_str(), args.join(" ")))
        .collect::<Vec<_>>()
        .join(" && ")
}

fn plugin_marketplace_manifest(install_root: &Path, client: HarnessClient) -> PathBuf {
    match client {
        HarnessClient::Codex => install_root
            .join(".agents")
            .join("plugins")
            .join("marketplace.json"),
        _ => install_root
            .join(".claude-plugin")
            .join("marketplace.json"),
    }
}

/// Run each plugin CLI step, tolerating idempotent "already" outcomes, and
/// return the first substantive failure detail.
fn run_plugin_steps<F>(client: HarnessClient, steps: &[Vec<String>], runner: &mut F) -> Option<String>
where
    F: FnMut(HarnessClient, &[String]) -> CommandResult,
{
    for args in steps {
        let result = runner(client, args);
        if result.success() {
            continue;
        }
        let output = format!("{}\n{}", result.stdout, result.stderr).to_ascii_lowercase();
        if output.contains("already") {
            continue;
        }
        let detail = if result.stderr.trim().is_empty() {
            result.stdout.trim()
        } else {
            result.stderr.trim()
        };
        return Some(format!(
            "{} {} failed: {detail}",
            client.as_str(),
            args.join(" ")
        ));
    }
    None
}

fn reconcile_host_plugins<F>(
    install_root: &Path,
    clients: &[HarnessClient],
    dry_run: bool,
    mut runner: F,
) -> Result<BTreeMap<HarnessClient, PluginActivationReceipt>, String>
where
    F: FnMut(HarnessClient, &[String]) -> CommandResult,
{
    let mut states = BTreeMap::new();
    let payload_version = payload_plugin_version(install_root);
    for &client in clients {
        if !client.has_plugin_surface() {
            states.insert(
                client,
                plugin_receipt(client, "not_applicable", None, false, None),
            );
            continue;
        }
        let observed = |p: &PluginProjection| -> (String, Option<String>, Option<String>) {
            match p {
                PluginProjection::Enabled { version } => {
                    ("enabled".to_string(), Some(version.clone()), None)
                }
                PluginProjection::Disabled { detail } => (
                    "installed_disabled".to_string(),
                    payload_version.clone(),
                    detail.clone(),
                ),
                PluginProjection::Absent => ("absent".to_string(), None, None),
                PluginProjection::Conflict(detail) => {
                    ("conflict".to_string(), None, Some(detail.clone()))
                }
            }
        };
        let projection = plugin_projection(client, install_root, payload_version.as_deref());
        if dry_run {
            let (state, version, detail) = observed(&projection);
            let detail = if state == "absent" {
                Some(format!("would run {}", plugin_manual_steps(client, install_root)))
            } else {
                detail
            };
            states.insert(client, plugin_receipt(client, &state, version, false, detail));
            continue;
        }
        match &projection {
            PluginProjection::Enabled { .. } | PluginProjection::Conflict(_) => {
                let (state, version, detail) = observed(&projection);
                states.insert(client, plugin_receipt(client, &state, version, false, detail));
                continue;
            }
            _ => {}
        }
        if !runner(client, &["--version".to_string()]).success() {
            states.insert(
                client,
                plugin_receipt(
                    client,
                    "action_required",
                    payload_version.clone(),
                    false,
                    Some(format!(
                        "{} CLI unavailable; run {}",
                        client.as_str(),
                        plugin_manual_steps(client, install_root)
                    )),
                ),
            );
            continue;
        }
        if !plugin_marketplace_manifest(install_root, client).is_file() {
            states.insert(
                client,
                plugin_receipt(
                    client,
                    "action_required",
                    payload_version.clone(),
                    false,
                    Some(format!(
                        "installed payload lacks {}",
                        plugin_marketplace_manifest(install_root, client).display()
                    )),
                ),
            );
            continue;
        }
        let steps = plugin_install_steps(client, install_root);
        let failure = run_plugin_steps(client, &steps, &mut runner);
        let after = plugin_projection(client, install_root, payload_version.as_deref());
        let (state, version, mut detail) = observed(&after);
        let state = match state.as_str() {
            "enabled" | "conflict" => state,
            _ => "action_required".to_string(),
        };
        if state == "action_required" {
            detail = failure.or(detail).or_else(|| {
                Some(format!(
                    "run {}",
                    plugin_manual_steps(client, install_root)
                ))
            });
        }
        states.insert(
            client,
            plugin_receipt(client, &state, version, state == "enabled", detail),
        );
    }
    Ok(states)
}

/// Detach owned native plugin projections during deactivation. CLI-first;
/// when the CLI is unavailable the owned host-state files are updated
/// directly (Codex config table, Claude enabledPlugins flag). Foreign
/// `membrane` bindings are never touched.
fn deactivate_host_plugins<F>(
    install_root: &Path,
    clients: &[HarnessClient],
    dry_run: bool,
    mut runner: F,
) -> Result<BTreeMap<HarnessClient, PluginActivationReceipt>, String>
where
    F: FnMut(HarnessClient, &[String]) -> CommandResult,
{
    let mut states = BTreeMap::new();
    let payload_version = payload_plugin_version(install_root);
    for &client in clients {
        if !client.has_plugin_surface() {
            states.insert(
                client,
                plugin_receipt(client, "not_applicable", None, false, None),
            );
            continue;
        }
        let projection = plugin_projection(client, install_root, payload_version.as_deref());
        match &projection {
            PluginProjection::Absent => {
                states.insert(client, plugin_receipt(client, "absent", None, false, None));
                continue;
            }
            PluginProjection::Conflict(detail) => {
                states.insert(
                    client,
                    plugin_receipt(client, "conflict", None, false, Some(detail.clone())),
                );
                continue;
            }
            _ => {}
        }
        if dry_run {
            states.insert(
                client,
                plugin_receipt(
                    client,
                    "installed_disabled",
                    payload_version.clone(),
                    false,
                    Some(format!(
                        "would run {} {}",
                        client.as_str(),
                        plugin_disable_args(client).join(" ")
                    )),
                ),
            );
            continue;
        }
        let cli_ok = runner(client, &["--version".to_string()]).success()
            && run_plugin_steps(client, &[plugin_disable_args(client)], &mut runner).is_none();
        if !cli_ok {
            // Bounded file-level detach when the CLI cannot do it.
            match client {
                HarnessClient::Codex => {
                    let config = codex_home_dir()?.join("config.toml");
                    if let Ok(document) = std::fs::read_to_string(&config) {
                        let next = remove_toml_table(&document, "plugins.\"membrane@membrane\"");
                        if next != document {
                            std::fs::write(&config, next).map_err(|error| {
                                format!("detach codex plugin state {}: {error}", config.display())
                            })?;
                        }
                    }
                }
                HarnessClient::Claude => {
                    let settings_path = claude_config_dir()?.join("settings.json");
                    let (original, mut value) = read_client_config(&settings_path)?;
                    if let Some(enabled) = value
                        .get_mut("enabledPlugins")
                        .and_then(|e| e.as_object_mut())
                    {
                        if enabled.contains_key(PLUGIN_ID) {
                            enabled.insert(PLUGIN_ID.to_string(), serde_json::json!(false));
                            write_client_config(&settings_path, original.as_deref(), &value)?;
                        }
                    }
                }
                _ => {}
            }
        }
        let after = plugin_projection(client, install_root, payload_version.as_deref());
        let detached = matches!(after, PluginProjection::Absent | PluginProjection::Disabled { .. });
        states.insert(
            client,
            plugin_receipt(
                client,
                if detached { "detached" } else { "action_required" },
                payload_version.clone(),
                detached,
                if detached {
                    None
                } else {
                    Some(format!(
                        "run {} {}",
                        client.as_str(),
                        plugin_disable_args(client).join(" ")
                    ))
                },
            ),
        );
    }
    Ok(states)
}

/// Remove a `[table]` (and its keys) from a small TOML document; nested
/// tables under the header are removed with it.
fn remove_toml_table(document: &str, table: &str) -> String {
    let header = format!("[{table}]");
    let mut out = String::new();
    let mut skipping = false;
    for line in document.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            skipping = trimmed == header || trimmed.starts_with(&format!("{header}."));
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Returns true when a `membrane` entry reported by `mcp get` is one we own:
/// either the exact expected registration or a legacy entry whose command
/// resolves inside the installed payload directory.
fn owned_command_entry(
    config: &ServerConfig,
    client: HarnessClient,
    executable: &str,
) -> bool {
    if config_matches_expected(client, config, executable) {
        return true;
    }
    Path::new(&config.command)
        .parent()
        .and_then(|dir| dir.to_str())
        .is_some_and(|dir| {
            Path::new(executable)
                .parent()
                .and_then(|expected_dir| expected_dir.to_str())
                .is_some_and(|expected_dir| paths_equal(dir, expected_dir))
        })
}

fn reconcile_clients<F>(
    membrane: &Path,
    clients: &[HarnessClient],
    dry_run: bool,
    mut runner: F,
    plugin_states: &BTreeMap<HarnessClient, PluginActivationReceipt>,
) -> Result<Vec<ClientActivationReceipt>, String>
where
    F: FnMut(HarnessClient, &[String]) -> CommandResult,
{
    let executable = membrane.to_string_lossy().into_owned();
    let command_clients = clients.iter().copied().filter(|client| !uses_config_file(*client)).collect::<Vec<_>>();
    let config_clients = clients.iter().copied().filter(|client| uses_config_file(*client)).collect::<Vec<_>>();
    let mut inspections = Vec::with_capacity(command_clients.len());
    for client in command_clients {
        // An enabled native plugin owns this host's MCP binding; the global
        // registration must not coexist with it.
        let plugin_owned = plugin_state_is(plugin_states, client, "enabled");
        let detected = runner(client, &["--version".to_string()]);
        let state = if !detected.success() {
            ClientState::NotInstalled
        } else {
            inspect_client(client, &executable, &mut runner)?
        };
        inspections.push((client, state, plugin_owned));
    }

    if dry_run {
        let mut receipts = inspections
            .into_iter()
            .map(|(client, state, plugin_owned)| ClientActivationReceipt {
                client,
                before: state.label().to_string(),
                after: if plugin_owned { "plugin_owned".to_string() } else { state.label().to_string() },
                changed: false,
                plugin: None,
            })
            .collect::<Vec<_>>();
        receipts.extend(reconcile_config_clients(&executable, &config_clients, true)?);
        receipts.sort_by_key(|receipt| clients.iter().position(|client| *client == receipt.client).unwrap_or(usize::MAX));
        return Ok(receipts);
    }

    let mut completed: Vec<(HarnessClient, ClientState)> = Vec::new();
    for (client, state, plugin_owned) in inspections {
        if plugin_owned {
            // Plugin owns the binding. Remove an owned global entry so exactly
            // one registration exists; leave foreign entries untouched.
            let removed = match &state {
                ClientState::Absent | ClientState::NotInstalled => false,
                ClientState::AlreadyCorrect => true,
                // Plugin-owned inspection state carries no global entry to
                // remove; dedup already ran for it.
                ClientState::PluginOwned { .. } => false,
                ClientState::Conflict(config) => {
                    if owned_command_entry(config, client, &executable) {
                        true
                    } else {
                        completed.push((client, ClientState::Conflict(config.clone())));
                        continue;
                    }
                }
            };
            if removed {
                require_command_success(client, "remove", runner(client, &remove_args(client)))?;
                let verify = runner(client, &get_args(client));
                if verify.success() && parse_prior_config(&verify.stdout).is_some() {
                    return Err(format!("{} global remove verification failed", client.as_str()));
                }
            }
            completed.push((client, ClientState::PluginOwned { removed }));
            continue;
        }
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
            let transport_args = if uses_http_transport(client) {
                Vec::new()
            } else {
                vec!["stdio-mcp".to_string()]
            };
            require_command_success(
                client,
                "add",
                runner(
                    client,
                    &add_args(
                        client,
                        &registration_command(client, &executable),
                        &transport_args,
                    ),
                ),
            )?;
            match inspect_client(client, &executable, &mut runner)? {
                ClientState::AlreadyCorrect => Ok(()),
                _ => Err(format!("{} add verification failed", client.as_str())),
            }
        })();
        if let Err(error) = result {
            let unrestored = rollback_clients(client, &state, &completed, &mut runner);
            if unrestored.is_empty() {
                return Err(error);
            }
            return Err(format!(
                "{error}; and rollback left these clients registered: {}",
                unrestored.join(", ")
            ));
        }
        completed.push((client, state));
    }

    let mut receipts = completed
        .iter()
        .map(|(client, state)| {
            let changed = matches!(state, ClientState::Absent | ClientState::Conflict(_));
            let (after, changed) = match state {
                ClientState::PluginOwned { removed } => ("plugin_owned", *removed),
                _ => (if changed { "installed" } else { state.label() }, changed),
            };
            ClientActivationReceipt {
                client: *client,
                before: state.label().to_string(),
                after: after.to_string(),
                changed,
                plugin: None,
            }
        })
        .collect::<Vec<_>>();
    let config_receipts = match reconcile_config_clients(&executable, &config_clients, false) {
        Ok(receipts) => receipts,
        Err(error) => {
            for (client, state) in completed.iter().rev() {
                if !matches!(state, ClientState::Absent | ClientState::Conflict(_)) { continue; }
                let _ = runner(*client, &remove_args(*client));
                if let ClientState::Conflict(prior) = state {
                    let _ = runner(*client, &prior_add_args(*client, prior));
                }
            }
            return Err(error);
        }
    };
    receipts.extend(config_receipts);
    receipts.sort_by_key(|receipt| clients.iter().position(|client| *client == receipt.client).unwrap_or(usize::MAX));
    Ok(receipts)
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
    for &client in clients.iter().filter(|client| !uses_config_file(**client)) {
        if !runner(client, &["--version".to_string()]).success() {
            receipts.push(ClientActivationReceipt {
                client,
                before: "not_installed".to_string(),
                after: "not_installed".to_string(),
                changed: false,
                plugin: None,
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
                plugin: None,
            });
            continue;
        }
        let owned = parse_prior_config(&current.stdout)
            .is_some_and(|config| config_matches_expected(client, &config, &executable));
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
            plugin: None,
        });
    }
    let config_clients = clients.iter().copied().filter(|client| uses_config_file(*client)).collect::<Vec<_>>();
    receipts.extend(deactivate_config_clients(&executable, &config_clients, dry_run)?);
    receipts.sort_by_key(|receipt| clients.iter().position(|client| *client == receipt.client).unwrap_or(usize::MAX));
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
    if is_expected(client, &current.stdout, executable) {
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

/// Undo the client registrations this activation made, naming any it could
/// not undo.
///
/// Every step discarded its result, so an activation that failed halfway
/// could leave a client registered against a Membrane that is not running,
/// and report only the original error.
fn rollback_clients<F>(
    active_client: HarnessClient,
    active_state: &ClientState,
    completed: &[(HarnessClient, ClientState)],
    runner: &mut F,
) -> Vec<String>
where
    F: FnMut(HarnessClient, &[String]) -> CommandResult,
{
    let mut unrestored = Vec::new();
    let mut step = |client: HarnessClient, args: &[String], runner: &mut F| {
        if !runner(client, args).success() {
            unrestored.push(client.as_str().to_string());
        }
    };
    step(active_client, &remove_args(active_client), runner);
    if let ClientState::Conflict(prior) = active_state {
        step(active_client, &prior_add_args(active_client, prior), runner);
    }
    for (client, state) in completed.iter().rev() {
        if !matches!(state, ClientState::Absent | ClientState::Conflict(_)) {
            continue;
        }
        step(*client, &remove_args(*client), runner);
        if let ClientState::Conflict(prior) = state {
            step(*client, &prior_add_args(*client, prior), runner);
        }
    }
    unrestored.sort();
    unrestored.dedup();
    unrestored
}

fn get_args(client: HarnessClient) -> Vec<String> {
    match client {
        HarnessClient::Codex => vec!["mcp", "get", "membrane", "--json"],
        HarnessClient::Claude => vec!["mcp", "get", "membrane"],
        HarnessClient::Cursor
        | HarnessClient::Windsurf
        | HarnessClient::Antigravity
        | HarnessClient::Devin => unreachable!("config-managed client"),
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn remove_args(client: HarnessClient) -> Vec<String> {
    match client {
        HarnessClient::Codex => vec!["mcp", "remove", "membrane"],
        HarnessClient::Claude => vec!["mcp", "remove", "membrane", "-s", "user"],
        HarnessClient::Cursor
        | HarnessClient::Windsurf
        | HarnessClient::Antigravity
        | HarnessClient::Devin => unreachable!("config-managed client"),
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn add_args(client: HarnessClient, command: &str, args: &[String]) -> Vec<String> {
    let mut values = vec!["mcp".to_string(), "add".to_string()];
    match client {
        // Both supported native MCP hosts attach to the resident listener. The
        // token is resolved by each host from its normal credential environment;
        // no bearer value is placed in argv, logs, or receipts.
        HarnessClient::Codex => {
            values.extend([
                "membrane".to_string(),
                "--url".to_string(),
                command.to_string(),
                "--bearer-token-env-var".to_string(),
                MCP_TOKEN_ENV.to_string(),
            ]);
        }
        HarnessClient::Claude => {
            values.extend([
                "--scope".to_string(),
                "user".to_string(),
                "--transport".to_string(),
                "http".to_string(),
                "membrane".to_string(),
                command.to_string(),
                "--header".to_string(),
                format!("Authorization: Bearer ${{{MCP_TOKEN_ENV}}}"),
            ]);
        }
        HarnessClient::Cursor
        | HarnessClient::Windsurf
        | HarnessClient::Antigravity
        | HarnessClient::Devin => {
            values.extend([
                "membrane".to_string(),
                "--".to_string(),
                command.to_string(),
            ]);
            values.extend(args.iter().cloned());
        }
    }
    values
}

fn prior_add_args(client: HarnessClient, prior: &ServerConfig) -> Vec<String> {
    // Rollback must reproduce an existing registration exactly. In particular,
    // a legacy stdio entry must not be reinterpreted as a new HTTP URL.
    let mut values = vec!["mcp".to_string(), "add".to_string()];
    if client == HarnessClient::Claude {
        values.extend(["--scope".to_string(), "user".to_string()]);
    }
    values.extend(["membrane".to_string(), "--".to_string(), prior.command.clone()]);
    values.extend(prior.args.iter().cloned());
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
                .or_else(|| config.get("url"))
                .or_else(|| config.get("serverUrl"))
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
            .or_else(|| line.trim().strip_prefix("URL:"))
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

fn is_expected(client: HarnessClient, stdout: &str, executable: &str) -> bool {
    if let Some(config) = parse_prior_config(stdout) {
        return config_matches_expected(client, &config, executable);
    }
    if uses_http_transport(client) {
        let normalized = stdout.replace("\\\\", "\\");
        return normalized
            .to_ascii_lowercase()
            .contains(&installed_mcp_url().to_ascii_lowercase());
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

/// Run isolated LC-06 controls through the same lock, promotion, and hook
/// containment helpers used by activation.  The dispatcher never resolves or
/// mutates the installed `current` root.
pub fn run_lc06_scenario(name: &str) -> serde_json::Value {
    let identity = match verified_installed_identity() {
        Ok(identity) => identity,
        Err(reason) => return serde_json::json!({"status":"failed", "reason":reason}),
    };
    let install_root = match identity
        .get("current")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
    {
        Some(path) => path,
        None => return serde_json::json!({"status":"failed", "reason":"installed identity omitted current root"}),
    };
    let unique = format!("membrane-lc06-{}-{}", std::process::id(), now_unix_ms());
    let root = std::env::temp_dir().join(unique);
    let result = match name {
        "startup-lock" => {
            let outcome = (|| -> Result<serde_json::Value, String> {
                std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
                let held = acquire_lock_with_wait(&root, Duration::from_millis(100))?;
                // Age never overrides a live owner: even an artificially stale
                // lock must reject a second owner while this process holds it.
                let contention_refused =
                    acquire_lock_with_policy(&root, Duration::from_millis(50), Duration::ZERO)
                        .is_err();
                drop(held);
                // Simulate an abandoned owner with an invalid PID, then use
                // the production stale-owner path with an injected threshold
                // so this installed control stays bounded and deterministic.
                let lock = root.join(LOCK_DIR);
                std::fs::create_dir_all(&lock).map_err(|e| e.to_string())?;
                std::fs::write(lock.join("owner"), b"4294967294\n")
                    .map_err(|e| e.to_string())?;
                std::thread::sleep(Duration::from_millis(5));
                let recovered = match acquire_lock_with_policy(
                    &root,
                    Duration::from_millis(100),
                    Duration::ZERO,
                ) {
                    Ok(lock) => {
                        drop(lock);
                        true
                    }
                    Err(_) => false,
                };
                if !contention_refused || !recovered {
                    return Err("startup lock contention/stale recovery invariant failed".into());
                }
                Ok(serde_json::json!({"status":"passed", "nativeEvidence":true,
                    "contentionRefused":true,"staleOwnerRejected":true,"recovered":true,
                    "reason":"native activation lock refused live contention, rejected stale owner, then recovered"}))
            })();
            outcome.unwrap_or_else(|reason| serde_json::json!({"status":"failed", "reason":reason}))
        }
        "atomic-promotion" => {
            let outcome = (|| -> Result<serde_json::Value, String> {
                std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
                let staged = root.join("candidate"); let target = root.join("current");
                std::fs::write(&target, b"old").map_err(|e| e.to_string())?;
                std::fs::write(&staged, b"new-native").map_err(|e| e.to_string())?;
                replace_file(&staged, &target)?;
                let promoted = std::fs::read(&target).map_err(|e| e.to_string())?;
                let before = promoted.clone();
                let failed = replace_file(&root.join("missing"), &target).is_err();
                let unchanged = std::fs::read(&target).map_err(|e| e.to_string())? == before;
                let read_back_hash = format!("sha256:{}", hex::encode(sha2::Sha256::digest(&promoted)));
                if promoted != b"new-native" || !failed || !unchanged {
                    return Err("atomic promotion invariant failed".into());
                }
                Ok(serde_json::json!({"status":"passed","nativeEvidence":true,
                    "existingCurrent":true,"failedInputNonreplacement":true,
                    "readBackHash":read_back_hash,"contentSha256":read_back_hash.clone(),
                    "reason":"native staged candidate atomically replaced existing current; failed input left current unchanged"}))
            })();
            outcome.unwrap_or_else(|reason| serde_json::json!({"status":"failed", "reason":reason}))
        }
        "hook-containment" => {
            let outcome = (|| -> Result<serde_json::Value, String> {
                std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
                let settings_path = root.join("claude-settings.json");
                let expected = installed_hook_command(&install_root);
                let near_match = format!("{expected} --near-match");
                let unrelated = r"C:\other\tool.exe";
                std::fs::write(&settings_path, serde_json::to_vec_pretty(&serde_json::json!({"hooks":{"PreToolUse":[
                    {"hooks":[{"type":"command","command":expected},
                        {"type":"command","command":near_match},
                        {"type":"command","command":unrelated}]}
                ]}})).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                reconcile_claude_hooks_at(&settings_path, &install_root)?;
                let after_activation: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(&settings_path).map_err(|e| e.to_string())?,
                ).map_err(|e| e.to_string())?;
                let commands = after_activation["hooks"]["PreToolUse"].as_array()
                    .ok_or_else(|| "native hook roundtrip omitted hook groups".to_string())?
                    .iter().flat_map(|group| group["hooks"].as_array().into_iter().flatten()).collect::<Vec<_>>();
                let exact = commands.iter().any(|item| item["command"] == expected);
                let near = commands.iter().any(|item| item["command"] == near_match);
                let other = commands.iter().any(|item| item["command"] == unrelated);
                let removed = remove_claude_hooks_at(&settings_path, &install_root, false)?;
                let after_removal: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(&settings_path).map_err(|e| e.to_string())?,
                ).map_err(|e| e.to_string())?;
                let remaining = after_removal["hooks"]["PreToolUse"][0]["hooks"]
                    .as_array().ok_or_else(|| "native hook removal omitted near matches".to_string())?;
                let exact_removed = !remaining.iter().any(|item| item["command"] == expected);
                let near_preserved = remaining.iter().any(|item| item["command"] == near_match);
                let other_preserved = remaining.iter().any(|item| item["command"] == unrelated);
                if !(exact && near && other && removed >= 1 && exact_removed && near_preserved && other_preserved) {
                    return Err("native hook config roundtrip did not preserve exact/near-match boundaries".into());
                }
                Ok(serde_json::json!({"status":"passed","nativeEvidence":true,
                    "nativeCommand":expected,"roundTrip":true,"removed":removed,
                    "exactRemoved":true,"nearMatchPreserved":true,"unrelatedPreserved":true,
                    "reason":"native activation/deactivation config roundtrip removed only exact installed hook command"}))
            })();
            outcome.unwrap_or_else(|reason| serde_json::json!({"status":"failed", "reason":reason}))
        }
        _ => serde_json::json!({"status":"invalid", "reason":"unknown LC-06 scenario"}),
    };
    let mut result = result;
    if let Some(object) = result.as_object_mut() { object.insert("identity".into(), identity); }
    let _ = std::fs::remove_dir_all(root);
    result
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

    #[test]
    fn authorized_resident_launch_reset_clears_suppression_failures() {
        let product_root = tempfile::tempdir().unwrap();
        let state_path = product_root.path().join("state/tools/.cache/memory/engine-supervision.json");
        std::fs::create_dir_all(state_path.parent().unwrap()).unwrap();
        std::fs::write(&state_path, r#"{"schemaVersion":1,"clean":false,"failures":3,"observedAtUnixMs":0}"#).unwrap();
        reset_supervision_before_resident_launch(product_root.path()).unwrap();
        let state = std::fs::read_to_string(
            product_root
                .path()
                .join("state/tools/.cache/memory/engine-supervision.json"),
        )
        .unwrap();
        let state: serde_json::Value = serde_json::from_str(&state).unwrap();
        assert_eq!(state["clean"], true);
        assert_eq!(state["failures"], 0);
    }

    #[test]
    fn locked_self_image_is_ignored_only_for_sole_current_process() {
        let pid = 41;
        assert!(only_current_process_image_holder(&[pid], pid));
        assert!(!only_current_process_image_holder(&[pid, 42], pid));
        assert!(!only_current_process_image_holder(&[], pid));
    }

    fn result(code: i32, stdout: &str) -> CommandResult {
        CommandResult {
            code,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    #[test]
    fn tree_release_treats_missing_and_idle_trees_as_replaceable() {
        // Missing trees are vacuously replaceable.
        wait_for_tree_release(
            Path::new(r"C:\__membrane_no_such_tree__"),
            Duration::from_millis(50),
        )
        .expect("missing tree must be replaceable");
        // An idle temp tree with plain files opens for writing everywhere.
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("a.txt"), b"x").unwrap();
        std::fs::create_dir(directory.path().join("sub")).unwrap();
        std::fs::write(directory.path().join("sub/b.txt"), b"y").unwrap();
        wait_for_tree_release(directory.path(), Duration::from_millis(500))
            .expect("idle tree must be replaceable");
        assert!(first_locked_file(directory.path()).is_none());
    }

    #[test]
    fn startup_lock_rejects_live_owner_and_recovers_abandoned_owner() {
        let root = std::env::temp_dir().join(format!(
            "membrane-activation-lock-test-{}-{}",
            std::process::id(),
            now_unix_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let held = acquire_lock_with_wait(&root, Duration::from_millis(100)).unwrap();
        assert!(acquire_lock_with_policy(&root, Duration::from_millis(20), Duration::ZERO).is_err());
        drop(held);
        let lock = root.join(LOCK_DIR);
        std::fs::create_dir_all(&lock).unwrap();
        std::fs::write(lock.join("owner"), b"4294967294\n").unwrap();
        // A known-dead owner is reclaimable immediately, even while lock
        // metadata is young; this is the installer-killed activation case.
        let recovered = acquire_lock_with_policy(&root, Duration::from_millis(100), Duration::from_secs(3600)).unwrap();
        drop(recovered);
        assert!(!lock.exists());
        std::fs::create_dir_all(&lock).unwrap();
        // Missing owner metadata may be in its publication window and must
        // not be reclaimed solely because the directory is busy.
        assert!(acquire_lock_with_policy(&root, Duration::from_millis(20), Duration::from_secs(3600)).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn startup_lock_reclaim_serializes_concurrent_contenders() {
        let root = std::env::temp_dir().join(format!(
            "membrane-activation-lock-contention-{}-{}",
            std::process::id(), now_unix_ms()
        ));
        std::fs::create_dir_all(root.join(LOCK_DIR)).unwrap();
        std::fs::write(root.join(LOCK_DIR).join("owner"), b"4294967294\n").unwrap();
        let active = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let maximum = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let root = root.clone();
            let active = active.clone();
            let maximum = maximum.clone();
            workers.push(std::thread::spawn(move || {
                let _lock = acquire_lock_with_policy(&root, Duration::from_secs(2), Duration::from_secs(3600)).unwrap();
                let current = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                maximum.fetch_max(current, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(5));
                active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }));
        }
        for worker in workers { worker.join().unwrap(); }
        assert_eq!(maximum.load(std::sync::atomic::Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn codex_json_config_is_parsed_and_matched() {
        let body = r#"{"transport":{"type":"streamable_http","url":"http://127.0.0.1:47851/mcp"}}"#;
        assert_eq!(
            parse_prior_config(body),
            Some(ServerConfig {
                command: "http://127.0.0.1:47851/mcp".to_string(),
                args: vec![],
            })
        );
        assert!(is_expected(HarnessClient::Codex, body, r"C:\Membrane\membrane.exe"));
        #[cfg(windows)]
        assert!(is_expected(
            HarnessClient::Codex,
            "URL: http://127.0.0.1:47851/mcp",
            r"\\?\C:\Membrane\membrane.exe"
        ));
    }

    #[test]
    fn codex_hook_projection_is_owned_idempotent_and_foreign_safe() {
        let directory = tempfile::tempdir().unwrap();
        let install = directory.path().join("current");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join(executable_name("membrane-client")), b"native").unwrap();
        let path = directory.path().join("hooks.json");
        let foreign = r#"C:\Other\hook.exe hook"#;
        let legacy = legacy_engine_hook_command(&install);
        let config = serde_json::json!({"hooks":{"UserPromptSubmit":[
            {"hooks":[{"type":"command","command":foreign}]},
            {"hooks":[{"type":"command","command":legacy}]},
            {"hooks":[{"type":"command","command":installed_hook_command(&install)}]},
            {"hooks":[{"type":"command","command":installed_hook_command(&install)}]}
        ]}});
        std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        reconcile_codex_hooks_at(&path, &install).unwrap();
        reconcile_codex_hooks_at(&path, &install).unwrap();
        let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let entries = after["hooks"]["UserPromptSubmit"].as_array().unwrap();
        let owned = entries.iter().flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
            .filter(|item| item["command"] == installed_hook_command(&install)).count();
        assert_eq!(owned, 1);
        // The obsolete engine-direct hook binding is migrated away, never kept.
        assert!(!entries.iter().flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
            .any(|item| item["command"] == legacy));
        assert!(entries.iter().any(|entry| entry["hooks"].as_array().unwrap().iter().any(|item| item["command"] == foreign)));
        for &event in CODEX_HOOK_EVENTS {
            let owned: Vec<_> = after["hooks"][event].as_array().unwrap().iter()
                .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
                .filter(|item| item["command"] == installed_hook_command(&install)).collect();
            assert_eq!(owned.len(), 1, "{event}");
            assert_eq!(owned[0]["timeout"], if event == "SessionEnd" { 3 } else { 20 });
        }
        assert!(after["hooks"].get("TaskCompleted").is_none());
        let mut removable = after;
        assert_eq!(remove_exact_hook_items(&mut removable, &installed_hook_command(&install)), CODEX_HOOK_EVENTS.len());
    }

    #[test]
    fn claude_session_start_projection_is_native_idempotent_and_foreign_safe() {
        let directory = tempfile::tempdir().unwrap();
        let install = directory.path().join("current");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join(executable_name("membrane-client")), b"native").unwrap();
        let path = directory.path().join("settings.json");
        let foreign = r#"C:\Other\hook.exe hook"#;
        let command = installed_hook_command(&install);
        let config = serde_json::json!({"hooks":{"SessionStart":[
            {"hooks":[{"type":"command","command":foreign}]},
            {"hooks":[{"type":"command", "command":command}]},
            {"hooks":[{"type":"command", "command":command}]}
        ]}});
        std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        reconcile_claude_hooks_at(&path, &install).unwrap();
        reconcile_claude_hooks_at(&path, &install).unwrap();
        let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let entries = after["hooks"]["SessionStart"].as_array().unwrap();
        let owned: Vec<_> = entries.iter().flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
            .filter(|item| item["command"] == command).collect();
        assert_eq!(owned.len(), 1);
        assert_eq!(owned[0]["timeout"], 20);
        assert!(owned[0].get("additionalContextLimit").is_none());
        assert!(entries.iter().any(|entry| entry["hooks"].as_array().unwrap().iter().any(|item| item["command"] == foreign)));
        for &event in CLAUDE_HOOK_EVENTS {
            let owned: Vec<_> = after["hooks"][event].as_array().unwrap().iter()
                .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
                .filter(|item| item["command"] == command).collect();
            assert_eq!(owned.len(), 1, "{event}");
            assert_eq!(owned[0]["timeout"], if event == "SessionEnd" { 3 } else { 20 });
        }
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
            ["mcp", "get", "membrane"]
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
        let expected = r#"{"transport":{"type":"streamable_http","url":"http://127.0.0.1:47851/mcp"}}"#;
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
            }, &BTreeMap::new())
            .unwrap();
        assert_eq!(receipts[0].before, "absent");
        assert_eq!(receipts[0].after, "installed");
        assert!(receipts[0].changed);
        assert_eq!(
            calls[2].1,
            add_args(
                HarnessClient::Codex,
                &installed_mcp_url(),
                &[]
            )
        );
    }

    #[test]
    fn later_failure_restores_prior_binding() {
        let membrane = Path::new(r"C:\Membrane\membrane.exe");
        let prior = r#"{"transport":{"command":"node","args":["old.mjs"]}}"#;
        let expected = r#"{"transport":{"type":"streamable_http","url":"http://127.0.0.1:47851/mcp"}}"#;
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
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(error.contains("claude add failed"));
        assert!(calls.iter().any(|(client, args)| {
            *client == HarnessClient::Codex
                && *args == prior_add_args(HarnessClient::Codex, &ServerConfig { command: "node".into(), args: vec!["old.mjs".into()] })
        }));
    }

    #[test]
    fn additional_clients_use_documented_global_config_files() {
        assert!(client_config_path(HarnessClient::Cursor).unwrap().ends_with(".cursor/mcp.json"));
        assert!(client_config_path(HarnessClient::Windsurf).unwrap().ends_with(".codeium/windsurf/mcp_config.json"));
        assert!(client_config_path(HarnessClient::Antigravity).unwrap().ends_with(".gemini/config/mcp_config.json"));
        assert!(uses_config_file(HarnessClient::Cursor));
        assert!(!uses_config_file(HarnessClient::Codex));
    }

    #[test]
    fn config_client_merge_preserves_other_servers_and_replaces_conflict() {
        let executable = r"C:\Membrane\current\membrane.exe";
        let original = serde_json::json!({
            "mcpServers": {
                "other": { "command": "other", "args": [] },
                "membrane": { "command": "node", "args": ["old.mjs"] }
            },
            "unrelated": true
        });
        assert!(matches!(config_client_state(&original, executable).unwrap(), ClientState::Conflict(_)));
        let merged = config_with_membrane(original, executable).unwrap();
        assert_eq!(merged["mcpServers"]["other"]["command"], "other");
        assert_eq!(merged["mcpServers"]["membrane"]["command"], executable);
        assert_eq!(merged["mcpServers"]["membrane"]["args"], serde_json::json!(["stdio-mcp"]));
        assert!(matches!(config_client_state(&merged, executable).unwrap(), ClientState::AlreadyCorrect));
    }

    #[test]
    fn read_client_config_treats_empty_file_as_empty_object() {
        let dir = std::env::temp_dir().join(format!("membrane-activation-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mcp_config.json");
        std::fs::write(&path, b"").unwrap();
        let (bytes, value) = read_client_config(&path).unwrap();
        assert_eq!(bytes.as_deref(), Some(&b""[..]));
        assert_eq!(value, serde_json::json!({}));

        std::fs::write(&path, b"   \n\t  ").unwrap();
        let (bytes, value) = read_client_config(&path).unwrap();
        assert_eq!(bytes.as_deref(), Some(&b"   \n\t  "[..]));
        assert_eq!(value, serde_json::json!({}));

        std::fs::write(&path, b"not json").unwrap();
        assert!(read_client_config(&path).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_client_deactivation_removes_only_exact_owned_entry() {
        let executable = r"C:\Membrane\current\membrane.exe";
        let owned = config_with_membrane(serde_json::json!({ "mcpServers": { "other": { "command": "x" } } }), executable).unwrap();
        let (removed, changed) = config_without_owned_membrane(owned, executable).unwrap();
        assert!(changed);
        assert!(removed["mcpServers"].get("membrane").is_none());
        assert_eq!(removed["mcpServers"]["other"]["command"], "x");
        let foreign = serde_json::json!({ "mcpServers": { "membrane": { "command": "node", "args": ["foreign.mjs"] } } });
        let (preserved, changed) = config_without_owned_membrane(foreign.clone(), executable).unwrap();
        assert!(!changed);
        assert_eq!(preserved, foreign);
    }

    #[test]
    fn deactivation_removes_only_exact_owned_client_binding() {
        let membrane = Path::new(r"C:\Membrane\current\membrane.exe");
        let exact = r#"{"transport":{"type":"streamable_http","url":"http://127.0.0.1:47851/mcp"}}"#;
        let foreign = r#"{"transport":{"type":"streamable_http","url":"http://127.0.0.1:47852/mcp"}}"#;
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
        let exact = r#"{"transport":{"type":"streamable_http","url":"http://127.0.0.1:47851/mcp"}}"#;
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
        let expected = r#""C:\Membrane\current\membrane.exe" hook"#;
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
    fn legacy_node_hook_projection_upgrades_to_native_command() {
        let expected = r#""C:\Membrane\current\membrane.exe" hook"#;
        let mut entries = vec![serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": r#""C:\Membrane\current\runtime\blueprint\lib\node.exe" "C:\Membrane\current\mcp\hooks\membrane-hook-entrypoint.mjs""#
            }]
        })];
        replace_legacy_hook_commands(&mut entries, expected);
        assert_eq!(entries[0]["hooks"][0]["command"], expected);
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
            activation_scope: "full".to_string(),
            workspace_config_migration: None,
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
    fn activation_optional_readiness_never_masks_real_component_failures() {
        let base = serde_json::json!({
            "ok": false, "serviceId": "membrane-hub", "nativeOnly": true,
            "runtimeOrigin": "installed", "releaseGeneration": "g1",
            "installationId": "install-1", "database": {"status": "empty"},
            "catalog": {"status": "ok"}, "enrolledRepoCount": 0,
            "blueprintWatcher": {"watcherState": "not_configured", "watcherRunning": false, "watcherDetail": null},
            "backgroundAuthority": {"active": false}
        });
        let observe = |body: &serde_json::Value| {
            parse_health_response(format!("HTTP/1.1 503 Service Unavailable\r\n\r\n{body}").as_bytes(), "g1").unwrap()
        };
        assert!(matches!(observe(&base), HealthObservation::Ready { .. }));
        let mut broken = base.clone();
        broken["enrolledRepoCount"] = serde_json::json!(1);
        broken["blueprintWatcher"]["watcherState"] = serde_json::json!("watcher_unavailable");
        broken["watcherRunning"] = serde_json::json!(true);
        broken["blueprintWatcher"]["watcherReady"] = serde_json::json!(false);
        assert!(matches!(observe(&broken), HealthObservation::NotReady { .. }));
        let mut harness_only = base.clone();
        harness_only["enrolledRepoCount"] = serde_json::json!(1);
        harness_only["blueprintWatcher"]["watcherState"] = serde_json::json!("watcher_unavailable");
        harness_only["blueprintWatcher"]["watcherRunning"] = serde_json::json!(false);
        assert!(matches!(observe(&harness_only), HealthObservation::Ready { .. }));
        let mut partial = base.clone();
        partial["enrolledRepoCount"] = serde_json::json!(1);
        partial["watcherRunning"] = serde_json::json!(true);
        partial["blueprintWatcher"]["watcherState"] = serde_json::json!("running");
        partial["blueprintWatcher"]["watcherReady"] = serde_json::json!(false);
        assert!(matches!(observe(&partial), HealthObservation::Ready { .. }));
        let mut corrupt = base.clone();
        corrupt["blueprintWatcher"]["watcherDetail"] = serde_json::json!("registry corrupt");
        assert!(matches!(observe(&corrupt), HealthObservation::NotReady { .. }));
        let mut unknown = base.clone();
        unknown["catalog"] = serde_json::Value::Null;
        assert!(matches!(observe(&unknown), HealthObservation::NotReady { .. }));
        let mut failed_store = base;
        failed_store["database"]["status"] = serde_json::json!("error");
        assert!(matches!(observe(&failed_store), HealthObservation::NotReady { .. }));
    }

    #[test]
    fn health_gate_rejects_foreign_identity_and_accepts_exact_generation() {
        let ready = b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true,\"serviceId\":\"membrane-hub\",\"nativeOnly\":true,\"runtimeOrigin\":\"installed\",\"releaseGeneration\":\"g1\",\"installationId\":\"install-1\"}";
        assert_eq!(
            parse_health_response(ready, "g1").unwrap(),
            HealthObservation::Ready {
                release_generation: "g1".to_string(),
                installation_id: "install-1".to_string(),
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
                release_generation: "g1".to_string(),
                installation_id: "install-1".to_string(),
            }
        );
        let development = b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true,\"serviceId\":\"membrane-hub\",\"nativeOnly\":true,\"runtimeOrigin\":\"development\",\"releaseGeneration\":\"g1\",\"installationId\":\"install-1\"}";
        assert!(matches!(
            parse_health_response(development, "g1").unwrap(),
            HealthObservation::Foreign(reason) if reason.contains("development")
        ));
        let legacy = b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true,\"serviceId\":\"membrane-hub\",\"nativeOnly\":true,\"runtimeOrigin\":\"installed\",\"releaseGeneration\":\"g0\"}";
        assert!(matches!(
            parse_health_response(legacy, "g1").unwrap(),
            HealthObservation::Foreign(reason) if reason.contains("installation identity")
        ));
    }

    #[test]
    fn health_ownership_requires_existing_matching_installation_identity() {
        let workspace = tempfile::tempdir().unwrap();
        let identity = membrane_runtime::installation_identity::InstallationPaths::for_workspace(workspace.path()).identity;
        std::fs::create_dir_all(identity.parent().unwrap()).unwrap();
        std::fs::write(
            &identity,
            r#"{"schema_version":2,"installation_id":"install-1","created_at":"now","startup_generation":0,"legacy_labels":[],"lineage":[],"current_service_instance_id":null,"current_claimed_at":null}"#,
        )
        .unwrap();
        let ready = || HealthObservation::Ready {
            release_generation: "g1".to_string(),
            installation_id: "install-1".to_string(),
        };
        assert_eq!(
            constrain_health_to_existing_installation(ready(), workspace.path()),
            ready()
        );
        let foreign = constrain_health_to_existing_installation(
            HealthObservation::Ready {
                release_generation: "g1".to_string(),
                installation_id: "other-installation".to_string(),
            },
            workspace.path(),
        );
        assert!(matches!(foreign, HealthObservation::Foreign(reason) if reason.contains("does not match")));
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
            activation_scope: "full".to_string(),
            workspace_config_migration: None,
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
                    release_generation: "g2".to_string(),
                    installation_id: "install-1".to_string(),
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
                release_generation: "g1".to_string(),
                installation_id: "install-1".to_string(),
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
            activation_scope: "full".to_string(),
            workspace_config_migration: None,
        };
        let value = serde_json::to_value(receipt).expect("inspection receipt JSON");
        assert_eq!(value["dryRun"], true);
        assert_eq!(value["service"]["state"], "unavailable");
        assert_eq!(value["service"]["port"], INSTALLED_PORT);
    }

    #[test]
    fn workspace_v2_migration_preserves_root_spelling_and_strips_python() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        let configured_root = root.join(".");
        let path = directory.path().join("workspace.json");
        std::fs::write(&path, serde_json::json!({
            "schemaVersion": 2,
            "workspaceRoot": configured_root,
            "pythonExecutable": "C:\\Python\\python.exe"
        }).to_string()).unwrap();

        let receipt = migrate_workspace_config(&path).unwrap();
        assert!(receipt.migrated);
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["schemaVersion"], 3);
        assert_eq!(value["workspaceRoot"], configured_root.to_string_lossy().as_ref());
        assert!(value.get("pythonExecutable").is_none());
    }

    #[test]
    fn workspace_v2_migration_is_idempotent_byte_for_byte() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        let path = directory.path().join("workspace.json");
        std::fs::write(&path, serde_json::json!({
            "schemaVersion": 2,
            "workspaceRoot": root,
            "pythonExecutable": "C:\\Python\\python.exe"
        }).to_string()).unwrap();
        assert!(migrate_workspace_config(&path).unwrap().migrated);
        let bytes = std::fs::read(&path).unwrap();
        assert!(!migrate_workspace_config(&path).unwrap().migrated);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn workspace_migration_rejects_invalid_schema_and_root() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("workspace.json");
        std::fs::write(&path, r#"{"schemaVersion": 9, "workspaceRoot": "C:\\missing"}"#)
            .unwrap();
        assert!(migrate_workspace_config(&path)
            .unwrap_err()
            .contains("workspace_config_schema_unsupported"));
        std::fs::write(&path, serde_json::json!({
            "schemaVersion": 2,
            "workspaceRoot": "relative",
            "pythonExecutable": "C:\\Python\\python.exe"
        }).to_string()).unwrap();
        assert!(migrate_workspace_config(&path)
            .unwrap_err()
            .contains("workspace_root_invalid"));
    }

    #[test]
    fn workspace_v2_dry_run_does_not_write() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        let path = directory.path().join("workspace.json");
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "workspaceRoot": root,
            "pythonExecutable": "C:\\Python\\python.exe"
        })).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(workspace_migration_at_path(&path, true).unwrap().is_none());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
