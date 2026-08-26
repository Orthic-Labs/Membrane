use super::*;

fn config(root: &std::path::Path) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 3,
        "workspaceRoot": root,
    }))
    .unwrap()
}
/// Full env-free matrix. Every case constructs the inputs by hand and never
/// touches the process environment.
#[test]
fn resolve_from_is_env_free_first_nonempty_and_strict() {
    let dir = tempfile::tempdir().unwrap();
    let explicit_config_path = dir.path().join("workspace.json");
    std::fs::write(&explicit_config_path, config(dir.path())).unwrap();
    let home_config_path = dir.path().join(".config/membrane/workspace.json");
    std::fs::create_dir_all(home_config_path.parent().unwrap()).unwrap();
    std::fs::write(&home_config_path, config(dir.path())).unwrap();
    let first = resolve_from(Some(dir.path().into()), Some(PathBuf::new()), None, None).unwrap();
    assert_eq!(first.root, std::fs::canonicalize(dir.path()).unwrap());
    let fallback = resolve_from(Some(PathBuf::new()), Some(dir.path().into()), None, None).unwrap();
    assert_eq!(fallback.root, std::fs::canonicalize(dir.path()).unwrap());
    // Config path used when no env override is present.
    let configured = resolve_from(None, None, Some(explicit_config_path.clone()), None).unwrap();
    assert_eq!(configured.root, std::fs::canonicalize(dir.path()).unwrap());

    // Empty config override falls through to <home>/.config/membrane/...
    let from_home =
        resolve_from(None, None, Some(PathBuf::new()), Some(dir.path().into())).unwrap();
    assert_eq!(from_home.root, std::fs::canonicalize(dir.path()).unwrap());

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
        dir.path().join(".config/membrane/workspace.json")
    );
}
#[test]
fn strict_config_rejects_missing_extra_legacy_and_invalid_paths() {
    assert!(serde_json::from_str::<Config>(r#"{"schemaVersion":3}"#).is_err());
    assert!(serde_json::from_str::<Config>(
        r#"{"schemaVersion":3,"workspaceRoot":"/tmp","legacy":"x"}"#
    )
    .is_err());
    let unsupported: Config =
        serde_json::from_str(r#"{"schemaVersion":1,"workspaceRoot":"/missing"}"#).unwrap();
    assert!(from_config(unsupported).is_err());
}

#[test]
fn migration_is_atomic_idempotent_and_runtime_requires_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workspace.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "workspaceRoot": dir.path(),
            "pythonExecutable": dir.path().join("removed-python"),
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        resolve_from(None, None, Some(path.clone()), None),
        Err(error) if error == "workspace_config_migration_required"
    ));
    let receipt = migrate_v2_to_v3(&path).unwrap();
    assert!(receipt.migrated);
    assert_eq!(
        receipt.workspace_root,
        std::fs::canonicalize(dir.path()).unwrap()
    );
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("\"schemaVersion\":3"));
    assert!(!written.contains("pythonExecutable"));
    assert!(!migrate_v2_to_v3(&path).unwrap().migrated);
}

#[test]
fn startup_migration_runs_before_root_override_and_skips_only_missing_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workspace.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "workspaceRoot": dir.path(),
            "pythonExecutable": dir.path().join("removed-python"),
        }))
        .unwrap(),
    )
    .unwrap();

    let receipt =
        migrate_startup_config_from(Some(dir.path().into()), None, Some(path.clone()), None)
            .unwrap()
            .unwrap();
    assert!(receipt.migrated);
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("pythonExecutable"));

    assert_eq!(
        migrate_startup_config_from(
            Some(dir.path().into()),
            None,
            Some(dir.path().join("missing.json")),
            None,
        )
        .unwrap(),
        None
    );
}
