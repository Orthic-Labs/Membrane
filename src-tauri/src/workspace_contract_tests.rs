use super::*;

fn config(root: &std::path::Path, python: &std::path::Path) -> Vec<u8> {
    format!(
        r#"{{"schemaVersion":2,"workspaceRoot":"{}","pythonExecutable":"{}"}}"#,
        root.display(),
        python.display()
    )
    .into_bytes()
}
/// Full env-free matrix. Every case constructs the inputs by hand and never
/// touches the process environment.
#[test]
fn resolve_from_is_env_free_first_nonempty_and_strict() {
    let dir = tempfile::tempdir().unwrap();
    let python = dir.path().join("python");
    std::fs::write(&python, b"x").unwrap();
    let explicit_config_path = dir.path().join("workspace.json");
    std::fs::write(&explicit_config_path, config(dir.path(), &python)).unwrap();
    let home_config_path = dir.path().join(".config/orthic/workspace.json");
    std::fs::create_dir_all(home_config_path.parent().unwrap()).unwrap();
    std::fs::write(&home_config_path, config(dir.path(), &python)).unwrap();
    let first = resolve_from(Some(dir.path().into()), Some(PathBuf::new()), None, None).unwrap();
    assert_eq!(first.root, std::fs::canonicalize(dir.path()).unwrap());
    let fallback = resolve_from(Some(PathBuf::new()), Some(dir.path().into()), None, None).unwrap();
    assert_eq!(fallback.root, std::fs::canonicalize(dir.path()).unwrap());
    // Config path used when no env override is present.
    let configured = resolve_from(None, None, Some(explicit_config_path.clone()), None).unwrap();
    assert_eq!(
        configured.python_executable,
        std::fs::canonicalize(&python).unwrap()
    );

    // Empty config override falls through to <home>/.config/orthic/...
    let from_home =
        resolve_from(None, None, Some(PathBuf::new()), Some(dir.path().into())).unwrap();
    assert_eq!(
        from_home.python_executable,
        std::fs::canonicalize(&python).unwrap()
    );

    // No roots, no home -> config missing.
    assert_eq!(
        resolve_from(None, None, None, None).err().unwrap(),
        "workspace_config_missing"
    );
    // Relative config override is invalid.
    assert!(resolve_from(None, None, Some(PathBuf::from("relative")), None).is_err());
    // Invalid non-empty primary fails closed (does not fall through).
    assert!(resolve_from(
        Some(PathBuf::from("/nonexistent-membrane-workspace")),
        Some(dir.path().into()),
        None,
        None
    )
    .is_err());
    // Config file missing when only home is present.
    assert_eq!(
        config_path(None, Some(dir.path().into())).unwrap(),
        dir.path().join(".config/orthic/workspace.json")
    );
}
#[test]
fn strict_config_rejects_missing_extra_legacy_and_invalid_paths() {
    assert!(
        serde_json::from_str::<Config>(r#"{"schemaVersion":2,"workspaceRoot":"/tmp"}"#).is_err()
    );
    assert!(serde_json::from_str::<Config>(
        r#"{"schemaVersion":2,"workspaceRoot":"/tmp","pythonExecutable":"/tmp/p","legacy":"x"}"#
    )
    .is_err());
    let legacy: Config = serde_json::from_str(
        r#"{"schemaVersion":1,"workspaceRoot":"/missing","pythonExecutable":"/missing"}"#,
    )
    .unwrap();
    assert!(from_config(legacy).is_err());
}
