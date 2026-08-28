#![cfg_attr(windows, windows_subsystem = "windows")]

//! Console-free Windows entrypoint for Task Scheduler. The scheduler owns this final process
//! directly; no shell wrapper or visible console host sits between it and the resident service.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};

const LIFECYCLE_READY_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Ephemeral capability received from Membrane lifecycle channel.
/// It is copied into process memory at startup, never persisted.
#[derive(Clone)]
pub struct LifecycleControl {
    snapshot_capability: Option<Arc<str>>,
    admission_open: Arc<AtomicBool>,
    shutdown_requested: Arc<AtomicBool>,
    ready: Arc<(Mutex<Option<u16>>, Condvar)>,
    command: Arc<Mutex<Option<String>>>,
    failure: Arc<Mutex<Option<String>>>,
}

impl Default for LifecycleControl {
    fn default() -> Self {
        Self {
            snapshot_capability: None,
            admission_open: Arc::new(AtomicBool::new(true)),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
            ready: Arc::new((Mutex::new(None), Condvar::new())),
            command: Arc::new(Mutex::new(None)),
            failure: Arc::new(Mutex::new(None)),
        }
    }
}

impl LifecycleControl {
    /// Bind a capability received from a validated lifecycle hello frame.
    /// The caller must retain no copy after this handoff.
    pub fn from_lifecycle_capability(capability: &str) -> Result<Self, String> {
        if capability.len() != 64
            || !capability
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err("lifecycle snapshot capability invalid".into());
        }
        Ok(Self {
            snapshot_capability: Some(Arc::<str>::from(capability)),
            ..Self::default()
        })
    }

    pub fn snapshot_authorized(&self, supplied: Option<&str>) -> bool {
        let (Some(expected), Some(actual)) = (&self.snapshot_capability, supplied) else {
            return false;
        };
        if expected.len() != actual.len() {
            return false;
        }
        expected
            .as_bytes()
            .iter()
            .zip(actual.as_bytes())
            .fold(0u8, |difference, (left, right)| difference | (left ^ right))
            == 0
    }

    fn hub_bound(&self) -> bool {
        self.snapshot_capability.is_some()
    }

    pub fn admission_open(&self) -> bool {
        self.admission_open.load(Ordering::Acquire)
    }

    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::Acquire)
    }

    pub fn request_drain(&self, command: Option<&str>) {
        if let Some(command) = command {
            if let Ok(mut current) = self.command.lock() {
                if current.is_none() {
                    *current = Some(command.to_string());
                }
            }
        }
        self.admission_open.store(false, Ordering::Release);
        self.shutdown_requested.store(true, Ordering::Release);
        self.ready.1.notify_all();
    }

    pub fn fail(&self, reason: impl Into<String>) {
        if let Ok(mut failure) = self.failure.lock() {
            if failure.is_none() {
                *failure = Some(reason.into());
            }
        }
        self.request_drain(None);
    }

    pub fn failure(&self) -> Option<String> {
        self.failure.lock().ok().and_then(|value| value.clone())
    }

    pub fn command(&self) -> Option<String> {
        self.command.lock().ok().and_then(|value| value.clone())
    }

    pub(crate) fn mark_ready(&self, port: u16) {
        if let Ok(mut ready) = self.ready.0.lock() {
            *ready = Some(port);
            self.ready.1.notify_all();
        }
    }

    pub fn wait_until_ready(&self) -> Result<u16, String> {
        let ready = self
            .ready
            .0
            .lock()
            .map_err(|_| "lifecycle ready state unavailable".to_string())?;
        let (ready, timeout) = self
            .ready
            .1
            .wait_timeout_while(ready, LIFECYCLE_READY_WAIT, |port| {
                port.is_none() && !self.shutdown_requested()
            })
            .map_err(|_| "lifecycle ready state unavailable".to_string())?;
        if self.shutdown_requested() {
            return Err(self
                .failure()
                .unwrap_or_else(|| "lifecycle startup stopped before ready".to_string()));
        }
        if let Some(port) = *ready {
            return Ok(port);
        }
        if timeout.timed_out() {
            return Err("lifecycle startup timeout".to_string());
        }
        Err(self
            .failure()
            .unwrap_or_else(|| "lifecycle startup stopped before ready".to_string()))
    }
}

static LIFECYCLE_CONTROL: OnceLock<RwLock<LifecycleControl>> = OnceLock::new();
static HUB_RUNTIME_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn install_lifecycle_control(control: LifecycleControl) -> Result<(), String> {
    let slot = LIFECYCLE_CONTROL.get_or_init(|| RwLock::new(LifecycleControl::default()));
    *slot
        .write()
        .map_err(|_| "lifecycle control unavailable".to_string())? = control;
    Ok(())
}

pub fn lifecycle_control() -> LifecycleControl {
    LIFECYCLE_CONTROL
        .get_or_init(|| RwLock::new(LifecycleControl::default()))
        .read()
        .map(|control| control.clone())
        .unwrap_or_default()
}

struct HubRuntimeClaim;

impl HubRuntimeClaim {
    fn acquire() -> Result<Self, String> {
        HUB_RUNTIME_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "membrane_hub_runtime_already_active".to_string())
    }
}

impl Drop for HubRuntimeClaim {
    fn drop(&mut self) {
        HUB_RUNTIME_ACTIVE.store(false, Ordering::Release);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeConfig {
    schema_version: u32,
    service_id: String,
    host: String,
    port: u16,
}

pub(crate) struct Runtime {
    pub(crate) workspace_root: PathBuf,
    pub(crate) db: PathBuf,
    pub(crate) token: PathBuf,
    pub(crate) ort: PathBuf,
    pub(crate) hf_home: PathBuf,
    pub(crate) port: u16,
    pub(crate) origin: &'static str,
    pub(crate) stable_current: Option<PathBuf>,
    pub(crate) version_root: Option<PathBuf>,
}

fn build_info() -> serde_json::Value {
    serde_json::json!({
        "product_version": env!("CARGO_PKG_VERSION"),
        "membrane_source_commit": option_env!("MEMBRANE_SOURCE_COMMIT").unwrap_or("unknown"),
        "source_tree_sha256": option_env!("MEMBRANE_SOURCE_TREE_SHA256").unwrap_or("unknown"),
        "release_generation": crate::release_identity::release_generation(),
        "target": crate::release_identity::target_triple(),
    })
}

pub(crate) fn prepare_runtime_identity(
    runtime: &Runtime,
) -> Result<
    (
        crate::installation_identity::InstallationIdentity,
        crate::installation_identity::StartupClaim,
    ),
    String,
> {
    crate::installation_identity::prepare_service_start(&runtime.workspace_root)
        .map_err(|error| format!("prepare installation identity: {error}"))
}

fn runtime_from_exe_at_workspace(
    exe: &Path,
    workspace_root: Option<&Path>,
    allow_hub_bundle: bool,
) -> Result<Runtime, String> {
    if workspace_root.is_none() {
        if let Ok(runtime) = runtime_from_installed_exe(exe) {
            return Ok(runtime);
        }
    }
    let direct_bin = exe
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "bin"))
        .filter(|path| {
            path.parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "tools")
        });
    let linked_bin = workspace_root.and_then(|root| {
        if !root.is_absolute() {
            return None;
        }
        let bin = root.join("tools/bin");
        let service_name = if cfg!(windows) {
            "membrane.exe"
        } else {
            "membrane"
        };
        let actual = std::fs::canonicalize(exe).ok()?;
        let service = bin.join(service_name);
        let metadata = std::fs::symlink_metadata(&service).ok()?;
        if !metadata.file_type().is_symlink() {
            return None;
        }
        let linked = std::fs::canonicalize(service).ok()?;
        (linked == actual).then_some(bin)
    });
    let bundled_bin = workspace_root.and_then(|root| {
        if !allow_hub_bundle || !root.is_absolute() || !is_hub_bundled_membrane(exe) {
            return None;
        }
        Some(root.join("tools/bin"))
    });
    let bin = direct_bin
        .map(Path::to_path_buf)
        .or(linked_bin)
        .or(bundled_bin)
        .ok_or_else(|| {
            "membrane resident must be Hub-owned or run from its exact canonical tools/bin path"
                .to_string()
        })?;
    let tools = bin
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "tools"))
        .ok_or_else(|| "membrane resident could not locate the tools directory".to_string())?;
    let config_path = tools.join("lib/memory/runtime.json");
    let config: RuntimeConfig = serde_json::from_slice(
        &std::fs::read(&config_path)
            .map_err(|error| format!("read {}: {error}", config_path.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", config_path.display()))?;
    if config.schema_version != 1
        || config.service_id != "membrane-local-v1"
        || config.host != "127.0.0.1"
        || config.port < 1024
    {
        return Err(format!(
            "invalid runtime identity in {}",
            config_path.display()
        ));
    }
    let ort_name = if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    };
    Ok(Runtime {
        workspace_root: tools
            .parent()
            .ok_or_else(|| "membrane runtime could not locate workspace root".to_string())?
            .to_path_buf(),
        db: tools.join(".cache/memory/cortex-engine.db"),
        token: tools.join(".cache/memory/api-token"),
        ort: bin.join(ort_name),
        hf_home: tools.join(".cache/fastembed"),
        port: config.port,
        origin: "development",
        stable_current: None,
        version_root: None,
    })
}

fn runtime_from_installed_exe(exe: &Path) -> Result<Runtime, String> {
    let current = exe
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "current"))
        .ok_or_else(|| "executable is not under installed current".to_string())?;
    let product_root = current
        .parent()
        .ok_or_else(|| "installed current has no product root".to_string())?;
    let versions = product_root.join("versions");
    let pointer = std::fs::read_link(current)
        .map_err(|error| format!("read installed current pointer: {error}"))?;
    let pointer = if pointer.is_absolute() {
        pointer
    } else {
        product_root.join(pointer)
    };
    let version_root = std::fs::canonicalize(pointer)
        .map_err(|error| format!("resolve installed version: {error}"))?;
    let versions = std::fs::canonicalize(versions)
        .map_err(|error| format!("resolve installed versions: {error}"))?;
    if version_root.parent() != Some(versions.as_path()) || !version_root.is_dir() {
        return Err("installed current does not target one direct version".into());
    }
    let state = product_root.join("state");
    runtime_from_installed_state(&state, current.to_path_buf(), version_root)
}

fn runtime_from_installed_state(
    state: &Path,
    stable_current: PathBuf,
    version_root: PathBuf,
) -> Result<Runtime, String> {
    let tools = state.join("tools");
    let bin = stable_current;
    let ort_name = if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    };
    Ok(Runtime {
        workspace_root: state.to_path_buf(),
        db: tools.join(".cache/memory/cortex-engine.db"),
        token: tools.join(".cache/memory/api-token"),
        ort: version_root.join(ort_name),
        hf_home: tools.join(".cache/fastembed"),
        port: 47_851,
        origin: "installed",
        stable_current: Some(bin),
        version_root: Some(version_root),
    })
}

fn is_hub_bundled_membrane(exe: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let Some(macos) = exe.parent() else {
            return false;
        };
        let Some(contents) = macos.parent() else {
            return false;
        };
        let Some(bundle) = contents.parent() else {
            return false;
        };
        return exe.file_name().is_some_and(|name| name == "membrane")
            && macos.file_name().is_some_and(|name| name == "MacOS")
            && contents.file_name().is_some_and(|name| name == "Contents")
            && bundle
                .file_name()
                .is_some_and(|name| name == "Membrane Hub.app")
            && macos.join("membrane-hub").is_file();
    }
    #[cfg(target_os = "windows")]
    {
        return exe.file_name().is_some_and(|name| name == "membrane.exe")
            && exe
                .parent()
                .is_some_and(|directory| directory.join("membrane-hub.exe").is_file());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

pub(crate) fn runtime_from_exe(exe: &Path) -> Result<Runtime, String> {
    let development = std::env::var_os("MEMBRANE_RUNTIME_ORIGIN")
        .is_some_and(|value| value == "development");
    let workspace = development.then(|| std::env::var_os("WORKSPACE_ROOT")).flatten().map(PathBuf::from);
    runtime_from_exe_at_workspace(exe, workspace.as_deref(), lifecycle_control().hub_bound())
}

fn runtime_from_workspace_root(workspace_root: &Path) -> Result<Runtime, String> {
    let root = std::fs::canonicalize(workspace_root)
        .map_err(|error| format!("canonicalize Hub workspace root: {error}"))?;
    if root.file_name().is_some_and(|name| name == "state")
        && root
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "Membrane")
    {
        let product_root = root
            .parent()
            .ok_or_else(|| "installed state has no product root".to_string())?;
        let current = product_root.join("current");
        let versions = std::fs::canonicalize(product_root.join("versions"))
            .map_err(|error| format!("resolve installed versions: {error}"))?;
        let pointer = std::fs::read_link(&current)
            .map_err(|error| format!("read installed current pointer: {error}"))?;
        let pointer = if pointer.is_absolute() {
            pointer
        } else {
            product_root.join(pointer)
        };
        let version_root = std::fs::canonicalize(pointer)
            .map_err(|error| format!("resolve installed version: {error}"))?;
        if version_root.parent() != Some(versions.as_path()) {
            return Err("installed current does not target one direct version".into());
        }
        return runtime_from_installed_state(&root, current, version_root);
    }
    let tools = root.join("tools");
    let bin = tools.join("bin");
    let config_path = tools.join("lib/memory/runtime.json");
    let config: RuntimeConfig = serde_json::from_slice(
        &std::fs::read(&config_path)
            .map_err(|error| format!("read {}: {error}", config_path.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", config_path.display()))?;
    if config.schema_version != 1
        || config.service_id != "membrane-local-v1"
        || config.host != "127.0.0.1"
        || config.port < 1024
    {
        return Err(format!(
            "invalid runtime identity in {}",
            config_path.display()
        ));
    }
    let ort_name = if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    };
    Ok(Runtime {
        workspace_root: root.clone(),
        db: tools.join(".cache/memory/cortex-engine.db"),
        token: tools.join(".cache/memory/api-token"),
        ort: bin.join(ort_name),
        hf_home: tools.join(".cache/fastembed"),
        port: config.port,
        origin: "development",
        stable_current: None,
        version_root: None,
    })
}

/// Run the sole resident Membrane runtime inside the active Hub process.
/// The process-wide claim rejects a second runtime before it can bind storage
/// or a port, preserving one active Hub/runtime authority.
pub fn run_hub_runtime(workspace_root: &Path, lifecycle: LifecycleControl) -> Result<(), String> {
    let _claim = HubRuntimeClaim::acquire()?;
    install_lifecycle_control(lifecycle)?;
    let runtime = runtime_from_workspace_root(workspace_root)?;
    run_runtime(runtime)
}

fn run_runtime(runtime: Runtime) -> Result<(), String> {
    std::env::set_var("CORTEX_DB", &runtime.db);
    std::env::set_var("MEMBRANE_PORT", runtime.port.to_string());
    std::env::set_var("MEMBRANE_API_TOKEN_FILE", &runtime.token);
    std::env::set_var("ORT_DYLIB_PATH", &runtime.ort);
    std::env::set_var("HF_HOME", &runtime.hf_home);
    std::env::set_var("HF_HUB_OFFLINE", "1");
    std::env::set_var("WORKSPACE_ROOT", &runtime.workspace_root);
    std::env::set_var("MEMBRANE_RUNTIME_ORIGIN", runtime.origin);
    let catalog_path = crate::catalog::default_catalog_path().map_err(|error| error.to_string())?;
    std::env::set_var("MEMBRANE_CATALOG", catalog_path);
    let (identity, claim) = prepare_runtime_identity(&runtime)?;
    let workspace_root = &runtime.workspace_root;
    // Publish the IPC handshake manifest before any peer can connect. This
    // is a hard requirement of the MBR-105 contract: a resident that has
    // not published its manifest must reject every handshake. We deliberately
    // do this AFTER `prepare_runtime_identity` so the manifest always
    // reflects the just-minted startup generation. A failure to publish is
    // fatal: the resident would otherwise serve requests that no peer can
    // verify, which silently breaks the contract.
    let active_manifest =
        crate::installation_manifest::build_active_manifest(&identity, &claim, workspace_root);
    crate::installation_manifest::publish_active_manifest(active_manifest)
        .map_err(|error| format!("publish installation manifest: {error}"))?;
    std::env::set_var("MEMBRANE_INSTALLATION_ID", &identity.installation_id);
    std::env::set_var("MEMBRANE_SERVICE_INSTANCE_ID", &claim.service_instance_id);
    crate::serve::run(
        runtime
            .db
            .to_str()
            .ok_or_else(|| "database path is not valid UTF-8".to_string())?,
        runtime.port,
        &identity,
        &claim,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_capability_is_bounded_memory_only_authority() {
        let capability = "a".repeat(64);
        let control = LifecycleControl::from_lifecycle_capability(&capability).unwrap();
        assert!(control.snapshot_authorized(Some(&capability)));
        assert!(!control.snapshot_authorized(Some("wrong")));
        assert!(!control.snapshot_authorized(None));
        assert!(LifecycleControl::from_lifecycle_capability("").is_err());
        assert!(LifecycleControl::from_lifecycle_capability(&"x".repeat(257)).is_err());
    }

    #[test]
    fn lifecycle_control_closes_admission_and_preserves_first_command() {
        let control = LifecycleControl::default();
        control.mark_ready(47_851);
        assert_eq!(control.wait_until_ready().unwrap(), 47_851);
        control.request_drain(Some("stop"));
        control.request_drain(Some("drain"));
        assert!(!control.admission_open());
        assert!(control.shutdown_requested());
        assert_eq!(control.command().as_deref(), Some("stop"));
    }

    #[test]
    fn hub_runtime_claim_allows_exactly_one_active_owner() {
        let first = HubRuntimeClaim::acquire().unwrap();
        assert!(HubRuntimeClaim::acquire().is_err());
        drop(first);
        assert!(HubRuntimeClaim::acquire().is_ok());
    }

    #[test]
    fn build_info_exposes_source_commit_and_tree_identity_fields() {
        let info = build_info();
        assert_eq!(info["product_version"], env!("CARGO_PKG_VERSION"));
        assert!(info.get("membrane_source_commit").is_some());
        assert!(info.get("source_tree_sha256").is_some());
        assert!(info.get("release_generation").is_some());
        assert!(info.get("target").is_some());
    }

    #[test]
    fn deployed_service_resolves_canonical_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("tools/bin");
        let config_dir = temp.path().join("tools/lib/memory");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("runtime.json"),
            r#"{"schemaVersion":1,"serviceId":"membrane-local-v1","host":"127.0.0.1","port":47851}"#,
        )
        .unwrap();
        let runtime = runtime_from_exe(&bin.join("membrane.exe")).unwrap();
        assert_eq!(runtime.port, 47851);
        assert_eq!(
            runtime.db,
            temp.path().join("tools/.cache/memory/cortex-engine.db")
        );
        assert_eq!(
            runtime.token,
            temp.path().join("tools/.cache/memory/api-token")
        );
    }

    #[test]
    fn hub_runtime_resolves_directly_from_its_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("tools/lib/memory");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("runtime.json"),
            r#"{"schemaVersion":1,"serviceId":"membrane-local-v1","host":"127.0.0.1","port":47851}"#,
        )
        .unwrap();
        let runtime = runtime_from_workspace_root(temp.path()).unwrap();
        assert_eq!(runtime.port, 47851);
        assert_eq!(
            runtime.db,
            temp.path()
                .canonicalize()
                .unwrap()
                .join("tools/.cache/memory/cortex-engine.db")
        );
    }

    #[test]
    fn installed_state_binds_fixed_port_and_stable_paths() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("Membrane/state");
        let current = temp.path().join("Membrane/current");
        let version = temp.path().join("Membrane/versions/v1");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&version).unwrap();
        let runtime = runtime_from_installed_state(&state, current.clone(), version.clone()).unwrap();
        assert_eq!(runtime.port, 47_851);
        assert_eq!(runtime.workspace_root, state);
        assert_eq!(runtime.stable_current, Some(current));
        assert_eq!(runtime.version_root, Some(version));
        assert_eq!(runtime.origin, "installed");
    }

    #[cfg(unix)]
    #[test]
    fn relocated_service_requires_exact_workspace_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let bin = workspace.join("tools/bin");
        let config_dir = workspace.join("tools/lib/memory");
        let relocated = temp.path().join("resident/membrane");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(relocated.parent().unwrap()).unwrap();
        std::fs::write(&relocated, b"fixture").unwrap();
        std::fs::write(
            config_dir.join("runtime.json"),
            r#"{"schemaVersion":1,"serviceId":"membrane-local-v1","host":"127.0.0.1","port":47851}"#,
        )
        .unwrap();
        symlink(&relocated, bin.join("membrane")).unwrap();

        let runtime = runtime_from_exe_at_workspace(&relocated, Some(&workspace), false).unwrap();
        assert_eq!(runtime.port, 47851);
        assert_eq!(runtime.ort, bin.join("libonnxruntime.dylib"));

        let other = temp.path().join("resident/other-service");
        std::fs::write(&other, b"other").unwrap();
        assert!(runtime_from_exe_at_workspace(&other, Some(&workspace), false).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bundled_service_requires_authenticated_hub_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let bin = workspace.join("tools/bin");
        let config_dir = workspace.join("tools/lib/memory");
        let app_bin = temp.path().join("Membrane Hub.app/Contents/MacOS");
        let membrane = app_bin.join("membrane");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&app_bin).unwrap();
        std::fs::write(&membrane, b"fixture").unwrap();
        std::fs::write(app_bin.join("membrane-hub"), b"fixture").unwrap();
        std::fs::write(
            config_dir.join("runtime.json"),
            r#"{"schemaVersion":1,"serviceId":"membrane-local-v1","host":"127.0.0.1","port":47851}"#,
        )
        .unwrap();

        assert!(runtime_from_exe_at_workspace(&membrane, Some(&workspace), false).is_err());
        let runtime = runtime_from_exe_at_workspace(&membrane, Some(&workspace), true).unwrap();
        assert_eq!(runtime.port, 47851);
        assert_eq!(
            runtime.db,
            workspace.join("tools/.cache/memory/cortex-engine.db")
        );
    }

    #[test]
    fn resident_startup_advances_identity_and_publishes_claim_before_serve() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = Runtime {
            workspace_root: temp.path().to_path_buf(),
            db: temp.path().join("tools/.cache/memory/cortex-engine.db"),
            token: temp.path().join("tools/.cache/memory/api-token"),
            ort: temp.path().join("tools/bin/onnxruntime.dll"),
            hf_home: temp.path().join("tools/.cache/fastembed"),
            port: 47851,
            origin: "development",
            stable_current: None,
            version_root: None,
        };

        let (identity, claim) = prepare_runtime_identity(&runtime).unwrap();

        assert_eq!(identity.startup_generation, 1);
        assert_eq!(claim.installation_id, identity.installation_id);
        assert!(temp
            .path()
            .join("memory-mirror/_installation_claims")
            .join(&claim.installation_id)
            .join(format!("{:020}", claim.startup_generation))
            .join(format!("{}.json", claim.service_instance_id))
            .is_file());
    }
}
