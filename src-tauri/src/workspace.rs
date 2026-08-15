use serde::Deserialize;
use std::path::PathBuf;

/// Schema v2 with `deny_unknown_fields`; missing required keys are rejected.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    schema_version: u32,
    workspace_root: PathBuf,
    python_executable: PathBuf,
}
pub struct Workspace {
    pub root: PathBuf,
    pub python_executable: PathBuf,
}

fn directory(path: PathBuf) -> Option<PathBuf> {
    path.is_absolute()
        .then(|| std::fs::canonicalize(path).ok())
        .flatten()
        .filter(|path| path.is_dir())
}
fn file(path: PathBuf) -> Option<PathBuf> {
    path.is_absolute()
        .then(|| std::fs::canonicalize(path).ok())
        .flatten()
        .filter(|path| path.is_file())
}
fn config_path(path: Option<PathBuf>, home: Option<PathBuf>) -> Result<PathBuf, String> {
    let explicit = path.filter(|path| !path.as_os_str().is_empty());
    let path = match explicit {
        Some(path) => path,
        None => home
            .map(|home| home.join(".config/orthic/workspace.json"))
            .ok_or_else(|| "workspace_config_missing".to_string())?,
    };
    (path.is_absolute() && path.is_file())
        .then_some(path)
        .ok_or_else(|| "workspace_config_invalid".into())
}
fn from_config(config: Config) -> Result<Workspace, String> {
    if config.schema_version != 2 {
        return Err("workspace_config_schema_unsupported".into());
    }
    Ok(Workspace {
        root: directory(config.workspace_root).ok_or("workspace_root_invalid")?,
        python_executable: file(config.python_executable).ok_or("python_executable_invalid")?,
    })
}
/// Environment roots take precedence. On Mac the user-home var is `HOME`;
/// on Windows it is `USERPROFILE`. We do not gather both -- mixing them
/// lets a stale shell var override the real user profile on one OS.
pub fn resolve() -> Result<Workspace, String> {
    // Product-neutral (O2): only Orthic-owned variables feed resolution. No
    // product-discriminating env fallback survives — a Membrane-specific env
    // branch is exactly the seam defect this module must not reintroduce.
    let primary = std::env::var_os("ORTHIC_WORKSPACE_ROOT").map(PathBuf::from);
    let config = std::env::var_os("ORTHIC_WORKSPACE_CONFIG").map(PathBuf::from);
    resolve_from(
        primary,
        std::env::var_os("WORKSPACE_ROOT").map(PathBuf::from),
        config,
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
                python_executable: PathBuf::new(),
            });
        }
    }
    let config = serde_json::from_slice(
        &std::fs::read(config_path(config, home)?).map_err(|_| "workspace_config_unreadable")?,
    )
    .map_err(|_| "workspace_config_invalid")?;
    from_config(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v2_config_requires_canonical_root_and_python() {
        let root = tempfile::tempdir().unwrap();
        let python = root.path().join("python");
        std::fs::write(&python, b"x").unwrap();
        let parsed: Config = serde_json::from_value(serde_json::json!({
            "schemaVersion": 2,
            "workspaceRoot": root.path(),
            "pythonExecutable": python,
        }))
        .unwrap();
        let workspace = from_config(parsed).unwrap();
        assert_eq!(workspace.root, std::fs::canonicalize(root.path()).unwrap());
        assert_eq!(
            workspace.python_executable,
            std::fs::canonicalize(python).unwrap()
        );
        assert!(serde_json::from_str::<Config>(r#"{"schemaVersion":2,"workspaceRoot":"/tmp","pythonExecutable":"/tmp/python","legacyRoot":"/tmp"}"#).is_err());
        assert!(
            serde_json::from_str::<Config>(r#"{"schemaVersion":2,"workspaceRoot":"/tmp"}"#)
                .is_err()
        );
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
