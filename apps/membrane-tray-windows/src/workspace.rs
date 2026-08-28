//! Strict workspace authority resolution for the resident tray.
//!
//! Login launches do not inherit a shell working directory or workspace
//! environment. Resolve the same durable v3 config previously consumed by
//! the Hub so the tray can launch its daemon against the canonical store.

use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

const WORKSPACE_SCHEMA_VERSION: u64 = 3;
pub const INSTALLED_PORT: u16 = 47_851;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeOrigin {
    Installed,
    Development,
}

impl RuntimeOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Development => "development",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub root: PathBuf,
    pub http_port: u16,
    pub origin: RuntimeOrigin,
    pub product_root: Option<PathBuf>,
    pub stable_current: Option<PathBuf>,
    pub version_root: Option<PathBuf>,
    pub state_root: Option<PathBuf>,
}

impl Workspace {
    pub fn daemon_path(&self) -> Option<PathBuf> {
        self.stable_current.as_ref().map(|root| {
            root.join(if cfg!(windows) {
                "membrane-daemon.exe"
            } else {
                "membrane-daemon"
            })
        })
    }

    pub fn tray_path(&self) -> Option<PathBuf> {
        self.stable_current.as_ref().map(|root| {
            root.join(if cfg!(windows) {
                "membrane-tray.exe"
            } else {
                "membrane-tray"
            })
        })
    }

    pub fn dashboard_path(&self) -> Option<PathBuf> {
        self.stable_current.as_ref().map(|root| {
            root.join(if cfg!(windows) {
                "membrane-hub.exe"
            } else {
                "membrane-hub"
            })
        })
    }
}

fn canonical_directory(path: &Path) -> Result<PathBuf, &'static str> {
    if !path.is_absolute() {
        return Err("workspace_root_invalid");
    }
    std::fs::canonicalize(path)
        .ok()
        .filter(|resolved| resolved.is_dir())
        .ok_or("workspace_root_invalid")
}

fn config_path() -> Result<PathBuf, &'static str> {
    if let Some(explicit) = std::env::var_os("MEMBRANE_WORKSPACE_CONFIG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return (explicit.is_absolute() && explicit.is_file())
            .then_some(explicit)
            .ok_or("workspace_config_invalid");
    }
    let profile = std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or("workspace_config_missing")?;
    let path = profile.join(".config/membrane/workspace.json");
    path.is_file()
        .then_some(path)
        .ok_or("workspace_config_missing")
}

fn runtime_port(root: &Path) -> Result<u16, &'static str> {
    let path = root.join("tools/lib/memory/runtime.json");
    let bytes = std::fs::read(path).map_err(|_| "workspace_runtime_config_missing")?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "workspace_runtime_config_invalid")?;
    let object = value
        .as_object()
        .ok_or("workspace_runtime_config_invalid")?;
    let valid_identity = object
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        == Some(1)
        && object.get("serviceId").and_then(serde_json::Value::as_str) == Some("membrane-local-v1")
        && object.get("host").and_then(serde_json::Value::as_str) == Some("127.0.0.1");
    let port = object
        .get("port")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| *value >= 1024)
        .ok_or("workspace_runtime_config_invalid")?;
    if !valid_identity {
        return Err("workspace_runtime_config_invalid");
    }
    Ok(port)
}

fn from_root(root: PathBuf) -> Result<Workspace, &'static str> {
    let root = canonical_directory(&root)?;
    let http_port = runtime_port(&root)?;
    Ok(Workspace {
        root,
        http_port,
        origin: RuntimeOrigin::Development,
        product_root: None,
        stable_current: None,
        version_root: None,
        state_root: None,
    })
}

fn product_root() -> Result<PathBuf, &'static str> {
    let base = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or("installed_product_root_missing")?;
    Ok(base.join("Orthic Labs").join("Membrane"))
}

fn installed_layout() -> Result<Workspace, &'static str> {
    let product_root = product_root()?;
    let stable_current = product_root.join("current");
    let versions = product_root.join("versions");
    let pointer = std::fs::read_link(&stable_current).map_err(|_| "installed_current_missing")?;
    let pointer = if pointer.is_absolute() {
        pointer
    } else {
        product_root.join(pointer)
    };
    let version_root = std::fs::canonicalize(pointer).map_err(|_| "installed_version_missing")?;
    let versions = std::fs::canonicalize(versions).map_err(|_| "installed_versions_missing")?;
    if version_root.parent() != Some(versions.as_path()) || !version_root.is_dir() {
        return Err("installed_current_invalid");
    }
    let state_root = product_root.join("state");
    Ok(Workspace {
        root: state_root.clone(),
        http_port: INSTALLED_PORT,
        origin: RuntimeOrigin::Installed,
        product_root: Some(product_root),
        stable_current: Some(stable_current),
        version_root: Some(version_root),
        state_root: Some(state_root),
    })
}

pub fn installed_tray_path() -> Option<PathBuf> {
    installed_layout().ok().and_then(|workspace| workspace.tray_path())
}

pub fn resolve() -> Result<Workspace, &'static str> {
    // Development is explicit. This branch is intentionally checked before
    // installed discovery so a checkout can never accidentally take over the
    // user's production state.
    if std::env::var_os("MEMBRANE_RUNTIME_ORIGIN").as_deref()
        == Some(std::ffi::OsStr::new("development"))
    {
        return resolve_development();
    }
    // A process launched from the production projection must never fall back
    // to a checkout when the pointer or installed state is damaged.
    if std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .is_some_and(|parent| {
            product_root()
                .ok()
                .is_some_and(|root| parent == root.join("current"))
        })
    {
        return installed_layout();
    }
    if ["MEMBRANE_WORKSPACE_ROOT", "WORKSPACE_ROOT"]
        .into_iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
    {
        return resolve_development();
    }
    if let Ok(installed) = installed_layout() {
        return Ok(installed);
    }
    resolve_development()
}

fn resolve_development() -> Result<Workspace, &'static str> {
    for name in ["MEMBRANE_WORKSPACE_ROOT", "WORKSPACE_ROOT"] {
        if let Some(root) = std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
        {
            return from_root(root);
        }
    }

    let bytes = std::fs::read(config_path()?).map_err(|_| "workspace_config_unreadable")?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "workspace_config_invalid")?;
    let object = value.as_object().ok_or("workspace_config_invalid")?;
    if object.len() != 2
        || object
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            != Some(WORKSPACE_SCHEMA_VERSION)
    {
        return Err("workspace_config_schema_unsupported");
    }
    let root = object
        .get("workspaceRoot")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .ok_or("workspace_config_invalid")?;
    from_root(root)
}

pub fn api_token(root: &Path) -> Result<String, &'static str> {
    let path = root.join("tools/.cache/memory/api-token");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("workspace_api_token_invalid");
            }
            let raw =
                std::fs::read_to_string(&path).map_err(|_| "workspace_api_token_unreadable")?;
            let token = raw.trim_end_matches(['\r', '\n']);
            if is_canonical_token(token) {
                return Ok(token.to_owned());
            }
            if token.is_empty() || token.contains('\r') || token.contains('\n') {
                return Err("workspace_api_token_invalid");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("workspace_api_token_unreadable"),
    }
    let token = generate_canonical_token()?;
    publish_token(&path, &token)?;
    Ok(token)
}

fn is_canonical_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn generate_canonical_token() -> Result<String, &'static str> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "workspace_api_token_generation_failed")?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn publish_token(path: &Path, token: &str) -> Result<(), &'static str> {
    let parent = path.parent().ok_or("workspace_api_token_write_failed")?;
    std::fs::create_dir_all(parent).map_err(|_| "workspace_api_token_write_failed")?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("api-token");
    let temp = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options
        .open(&temp)
        .map_err(|_| "workspace_api_token_write_failed")?;
    let write_result = file
        .write_all(token.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all());
    drop(file);
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp);
        return Err("workspace_api_token_write_failed");
    }
    let replace_result = replace_file(&temp, path);
    if replace_result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    replace_result
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), &'static str> {
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
    (result != 0)
        .then_some(())
        .ok_or("workspace_api_token_write_failed")
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<(), &'static str> {
    std::fs::rename(source, destination).map_err(|_| "workspace_api_token_write_failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_directory_rejects_relative_and_missing_roots() {
        assert_eq!(
            canonical_directory(Path::new("relative")),
            Err("workspace_root_invalid")
        );
        assert_eq!(
            canonical_directory(Path::new(r"C:\definitely-missing-membrane-root")),
            Err("workspace_root_invalid")
        );
    }

    #[test]
    fn canonical_token_shape_matches_daemon_protocol() {
        assert!(is_canonical_token(&"a".repeat(64)));
        assert!(!is_canonical_token(&"a".repeat(43)));
        assert!(!is_canonical_token(&"A".repeat(64)));
    }

    #[test]
    fn installed_contract_uses_state_and_fixed_port() {
        assert_eq!(INSTALLED_PORT, 47_851);
        assert_eq!(RuntimeOrigin::Installed.as_str(), "installed");
        assert_eq!(RuntimeOrigin::Development.as_str(), "development");
        let workspace = Workspace {
            root: PathBuf::from(r"C:\Users\test\AppData\Local\Orthic Labs\Membrane\state"),
            http_port: INSTALLED_PORT,
            origin: RuntimeOrigin::Installed,
            product_root: Some(PathBuf::from(r"C:\Users\test\AppData\Local\Orthic Labs\Membrane")),
            stable_current: Some(PathBuf::from(r"C:\Users\test\AppData\Local\Orthic Labs\Membrane\current")),
            version_root: Some(PathBuf::from(r"C:\Users\test\AppData\Local\Orthic Labs\Membrane\versions\0.1.0")),
            state_root: Some(PathBuf::from(r"C:\Users\test\AppData\Local\Orthic Labs\Membrane\state")),
        };
        assert_eq!(
            workspace.daemon_path().unwrap().file_stem().and_then(|name| name.to_str()),
            Some("membrane-daemon")
        );
    }
}
