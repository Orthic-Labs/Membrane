use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const WORKSPACE_SCHEMA_VERSION: u32 = 3;

/// Shipping workspace configuration. Python is intentionally not represented
/// here: a v2 document must be migrated by installation, never accepted by a
/// native-only Hub process.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    schema_version: u32,
    workspace_root: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigV2 {
    schema_version: u32,
    workspace_root: PathBuf,
    python_executable: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigV3<'a> {
    schema_version: u32,
    workspace_root: &'a PathBuf,
}

/// Durable, content-free installation receipt. Re-running migration against
/// v3 returns this same semantic result without rewriting configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfigMigrationReceipt {
    pub schema_version: u32,
    pub migration: &'static str,
    pub workspace_root: PathBuf,
    pub migrated: bool,
}
pub struct Workspace {
    pub root: PathBuf,
}

fn directory(path: PathBuf) -> Option<PathBuf> {
    path.is_absolute()
        .then(|| std::fs::canonicalize(path).ok())
        .flatten()
        .filter(|path| path.is_dir())
}
fn config_path(path: Option<PathBuf>, home: Option<PathBuf>) -> Result<PathBuf, String> {
    let explicit = path.filter(|path| !path.as_os_str().is_empty());
    let path = match explicit {
        Some(path) => path,
        None => home
            .map(|home| home.join(".config/membrane/workspace.json"))
            .ok_or_else(|| "workspace_config_missing".to_string())?,
    };
    (path.is_absolute() && path.is_file())
        .then_some(path)
        .ok_or_else(|| "workspace_config_invalid".into())
}
fn from_config(config: Config) -> Result<Workspace, String> {
    if config.schema_version != WORKSPACE_SCHEMA_VERSION {
        return Err("workspace_config_schema_unsupported".into());
    }
    Ok(Workspace {
        root: directory(config.workspace_root).ok_or("workspace_root_invalid")?,
    })
}

/// Atomically rewrite a strict v2 file as strict v3. Existing v3 input is an
/// idempotent no-op. This is installer/updater-only; runtime resolution below
/// deliberately returns `workspace_config_migration_required` for v2.
pub fn migrate_v2_to_v3(path: &std::path::Path) -> Result<WorkspaceConfigMigrationReceipt, String> {
    let bytes = std::fs::read(path).map_err(|_| "workspace_config_unreadable")?;
    if let Ok(config) = serde_json::from_slice::<Config>(&bytes) {
        let root = directory(config.workspace_root).ok_or("workspace_root_invalid")?;
        if config.schema_version != WORKSPACE_SCHEMA_VERSION {
            return Err("workspace_config_schema_unsupported".into());
        }
        return Ok(WorkspaceConfigMigrationReceipt {
            schema_version: 1,
            migration: "workspace_config_v2_to_v3",
            workspace_root: root,
            migrated: false,
        });
    }
    let legacy: ConfigV2 =
        serde_json::from_slice(&bytes).map_err(|_| "workspace_config_invalid")?;
    if legacy.schema_version != 2 {
        return Err("workspace_config_schema_unsupported".into());
    }
    let root = directory(legacy.workspace_root).ok_or("workspace_root_invalid")?;
    // Strictly parse the removed field so malformed v2 never becomes trusted
    // v3, but never require an executable that native Hub no longer uses.
    if !legacy.python_executable.is_absolute() {
        return Err("workspace_config_invalid".into());
    }
    let encoded = serde_json::to_vec(&ConfigV3 {
        schema_version: WORKSPACE_SCHEMA_VERSION,
        workspace_root: &root,
    })
    .map_err(|_| "workspace_config_invalid")?;
    let parent = path.parent().ok_or("workspace_config_invalid")?;
    let temp = parent.join(format!(".workspace-{}.tmp", std::process::id()));
    std::fs::write(&temp, encoded).map_err(|_| "workspace_config_unreadable")?;
    std::fs::rename(&temp, path).map_err(|_| {
        let _ = std::fs::remove_file(&temp);
        "workspace_config_unreadable"
    })?;
    Ok(WorkspaceConfigMigrationReceipt {
        schema_version: 1,
        migration: "workspace_config_v2_to_v3",
        workspace_root: root,
        migrated: true,
    })
}

/// Run installed-config migration during Hub startup, before any runtime
/// consumer attempts strict v3 resolution. Existing config is migrated even
/// when an explicit workspace-root override currently selects runtime root.
pub fn migrate_startup_config() -> Result<Option<WorkspaceConfigMigrationReceipt>, String> {
    migrate_startup_config_from(
        std::env::var_os("MEMBRANE_WORKSPACE_ROOT").map(PathBuf::from),
        std::env::var_os("WORKSPACE_ROOT").map(PathBuf::from),
        std::env::var_os("MEMBRANE_WORKSPACE_CONFIG").map(PathBuf::from),
        home_for_host_os(),
    )
}

fn migrate_startup_config_from(
    primary: Option<PathBuf>,
    compatibility: Option<PathBuf>,
    config: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<Option<WorkspaceConfigMigrationReceipt>, String> {
    let root_override = primary
        .as_deref()
        .is_some_and(|value| !value.as_os_str().is_empty())
        || compatibility
            .as_deref()
            .is_some_and(|value| !value.as_os_str().is_empty());

    // A root override changes runtime selection, but must not strand an
    // existing installed config at v2. Migrate an existing absolute config;
    // skip only when override is active and no usable config file exists.
    if root_override {
        let candidate = config
            .clone()
            .filter(|path| !path.as_os_str().is_empty())
            .or_else(|| {
                home.clone()
                    .map(|home| home.join(".config/membrane/workspace.json"))
            });
        if let Some(path) = candidate.filter(|path| path.is_absolute() && path.is_file()) {
            return migrate_v2_to_v3(&path).map(Some);
        }
        return Ok(None);
    }

    let path = config_path(config, home)?;
    migrate_v2_to_v3(&path).map(Some)
}

/// Environment roots take precedence. On Mac the user-home var is `HOME`;
/// on Windows it is `USERPROFILE`. We do not gather both -- mixing them
/// lets a stale shell var override the real user profile on one OS.
pub fn resolve() -> Result<Workspace, String> {
    resolve_from(
        std::env::var_os("MEMBRANE_WORKSPACE_ROOT").map(PathBuf::from),
        std::env::var_os("WORKSPACE_ROOT").map(PathBuf::from),
        std::env::var_os("MEMBRANE_WORKSPACE_CONFIG").map(PathBuf::from),
        home_for_host_os(),
    )
}

#[cfg(target_os = "windows")]
fn home_for_host_os() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

#[cfg(not(target_os = "windows"))]
fn home_for_host_os() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn resolve_from(
    primary: Option<PathBuf>,
    compatibility: Option<PathBuf>,
    config: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<Workspace, String> {
    for value in [primary, compatibility] {
        if let Some(value) = value.filter(|value| !value.as_os_str().is_empty()) {
            return Ok(Workspace {
                root: directory(value).ok_or("workspace_root_invalid")?,
            });
        }
    }
    let path = config_path(config, home)?;
    let bytes = std::fs::read(path).map_err(|_| "workspace_config_unreadable")?;
    if serde_json::from_slice::<ConfigV2>(&bytes)
        .ok()
        .is_some_and(|config| config.schema_version == 2)
    {
        return Err("workspace_config_migration_required".into());
    }
    let config = serde_json::from_slice(&bytes).map_err(|_| "workspace_config_invalid")?;
    from_config(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v3_config_requires_canonical_root_only() {
        let root = tempfile::tempdir().unwrap();
        let parsed = serde_json::from_value(serde_json::json!({
            "schemaVersion": 3,
            "workspaceRoot": root.path(),
        }))
        .unwrap();
        let workspace = from_config(parsed).unwrap();
        assert_eq!(workspace.root, std::fs::canonicalize(root.path()).unwrap());
        assert!(serde_json::from_str::<Config>(
            r#"{"schemaVersion":3,"workspaceRoot":"/tmp","legacyRoot":"/tmp"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<Config>(r#"{"schemaVersion":3}"#).is_err());
        assert_eq!(
            config_path(Some(PathBuf::from("relative")), None),
            Err("workspace_config_invalid".into())
        );
        // Empty primary must fall through to compatibility, not fail.
        assert_eq!(
            resolve_from(Some(PathBuf::new()), Some(root.path().into()), None, None)
                .unwrap()
                .root,
            std::fs::canonicalize(root.path()).unwrap()
        );
        // Invalid non-empty primary fails closed (does not fall through).
        assert!(resolve_from(
            Some(PathBuf::from("/nonexistent-membrane-workspace")),
            Some(root.path().into()),
            None,
            None
        )
        .is_err());
    }
}
#[cfg(test)]
#[path = "workspace_contract_tests.rs"]
mod workspace_contract_tests;
