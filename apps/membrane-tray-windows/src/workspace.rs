//! Strict workspace authority resolution for the resident tray.
//!
//! Login launches do not inherit a shell working directory or workspace
//! environment. Resolve the same durable v3 config previously consumed by
//! the Hub so the tray can launch its daemon against the canonical store.

use std::path::{Path, PathBuf};

use membrane_runtime::residency::Identity;

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

    /// Holder transitions require independently verified installed identity.
    /// This helper refuses development workspace values rather than deriving
    /// authority from a checkout path or tray process environment.
    pub fn validate_controller_identity(&self, identity: &Identity) -> Result<(), &'static str> {
        if self.origin != RuntimeOrigin::Installed {
            return Err("resident_controller_requires_installed_origin");
        }
        if self.stable_current.is_none() || self.version_root.is_none() || self.state_root.is_none() {
            return Err("resident_controller_layout_invalid");
        }
        if identity.installation_id.trim().is_empty()
            || identity.cortex_store_id.trim().is_empty()
            || identity.release_generation.trim().is_empty()
            || identity.startup_generation == 0
        {
            return Err("resident_controller_identity_invalid");
        }
        Ok(())
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
    let executable = std::env::current_exe().ok();
    let installed_root = product_root().ok();
    let development_requested = std::env::var_os("MEMBRANE_RUNTIME_ORIGIN").as_deref()
        == Some(std::ffi::OsStr::new("development"));
    match select_origin(executable.as_deref(), installed_root.as_deref(), development_requested) {
        RuntimeOrigin::Installed => installed_layout(),
        RuntimeOrigin::Development => resolve_development(),
    }
}

fn select_origin(executable: Option<&Path>, root: Option<&Path>, development_requested: bool) -> RuntimeOrigin {
    // Installed executables remain installed even if a calling development
    // shell leaks overrides. Account for Windows resolving `current` to its
    // version directory; a broken pointer must never authorize dev fallback.
    let installed_executable = executable.zip(root).is_some_and(|(exe, root)| {
        fn normalized(path: &Path) -> PathBuf {
            #[cfg(windows)]
            { PathBuf::from(path.to_string_lossy().trim_start_matches(r"\\?\").to_ascii_lowercase()) }
            #[cfg(not(windows))]
            { path.to_path_buf() }
        }
        let exe = normalized(exe);
        let root = normalized(root);
        exe.parent().is_some_and(|parent| {
            parent == root.join("current") || parent.parent() == Some(root.join("versions").as_path())
        })
    });
    if development_requested && !installed_executable {
        RuntimeOrigin::Development
    } else {
        RuntimeOrigin::Installed
    }
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

/// Outcome of resolving the daemon's bearer credential. `workspace.rs` is a
/// resolver only: it reads the credential `serve.rs` publishes and never
/// writes, generates, or replaces it. `serve.rs` (owned by a sibling lane)
/// is the sole protected 64-hex credential authority and migration lock —
/// this module reports what it observed so a caller can wait on or trigger
/// that authority, never so it can act as a second publisher.
///
/// Resolve the workspace daemon bearer credential.
///
/// This is a read-only resolver: it never generates, writes, or replaces the
/// credential file. `serve.rs` is the sole protected credential authority and
/// migration lock. A missing file, a noncanonical shape, or an
/// environment-installed override (`MEMBRANE_API_TOKEN`/`MEMBRANE_API_TOKEN_FILE`)
/// each return a distinct typed outcome so the caller can wait for or trigger
/// that authority's migration rather than the resolver silently accepting or
/// fabricating a value.
pub fn api_token(root: &Path) -> Result<String, &'static str> {
    // The resolver only ever trusts the canonical published file for this
    // workspace. An environment-installed credential is `serve.rs`'s
    // migration concern, not something this resolver adopts directly —
    // surfacing a typed outcome here keeps the two paths from silently
    // diverging on which credential is authoritative.
    if std::env::var_os("MEMBRANE_API_TOKEN").is_some()
        || std::env::var_os("MEMBRANE_API_TOKEN_FILE").is_some()
    {
        return Err("workspace_api_token_env_migration_required");
    }
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
            // Present but not the canonical 64-hex shape: this resolver never
            // rewrites or replaces the file (that is `serve.rs`'s exclusive
            // authority/migration lock). Report a typed migration outcome so
            // the caller waits for or triggers that authority's
            // fingerprint/recheck/compare-replace cycle instead.
            Err("workspace_api_token_migration_required")
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Bounded startup cleanup territory: nothing has been published
            // yet. The resolver never fabricates a credential to fill the
            // gap; the caller retries after the authority publishes one.
            Err("workspace_api_token_pending_publication")
        }
        Err(_) => Err("workspace_api_token_unreadable"),
    }
}

fn is_canonical_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_projection_and_version_reject_development_override() {
        let root = Path::new("/installed/Membrane");
        for relative in ["current/membrane-tray.exe", "versions/0.1.24/membrane-tray.exe"] {
            assert_eq!(select_origin(Some(&root.join(relative)), Some(root), true), RuntimeOrigin::Installed);
        }
        let checkout = Path::new("/development/membrane-tray.exe");
        assert_eq!(select_origin(Some(checkout), Some(root), false), RuntimeOrigin::Installed);
        assert_eq!(select_origin(Some(checkout), Some(root), true), RuntimeOrigin::Development);
        assert_eq!(select_origin(None, None, false), RuntimeOrigin::Installed);
    }

    #[cfg(windows)]
    #[test]
    fn installed_origin_accepts_windows_canonical_path_spelling() {
        assert_eq!(select_origin(
            Some(Path::new(r"\\?\C:\Users\Test\Membrane\versions\0.1.24\membrane-tray.exe")),
            Some(Path::new(r"C:\users\test\membrane")), true,
        ), RuntimeOrigin::Installed);
    }

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
