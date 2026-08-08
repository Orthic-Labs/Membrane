//! MBR-210: conservative import of source-coupled Crypt layouts.

use serde::Serialize;
use std::path::{Path, PathBuf};

pub const MIGRATION_RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const MIGRATION_RECEIPT_FILE_NAME: &str = "migration-receipt.json";
const LEGACY_LAYOUTS: [(&str, &str); 2] = [
    ("workspace-cache", "tools/.cache/memory"),
    ("home-crypt", ".claude/crypt"),
];
const PID_FILES: [&str; 3] = [
    "supervisor.pid",
    "crypt-service.pid",
    "membrane-supervisor.pid",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReceiptV1 {
    pub schema_version: u32,
    pub source: PathBuf,
    pub target: PathBuf,
    pub layouts: Vec<String>,
    pub outcome: &'static str,
    pub rollback: &'static str,
}

/// Moves recognized legacy state as whole directories. It never copies state, starts a daemon,
/// or acts while a legacy pid marker exists; rename makes rollback `rename(target, source)`.
pub fn migrate(legacy_root: &Path, target_root: &Path) -> Result<MigrationReceiptV1, String> {
    migrate_with_receipt_writer(legacy_root, target_root, |target, receipt| {
        let bytes = serde_json::to_vec_pretty(receipt)
            .map_err(|error| format!("serialize migration receipt: {error}"))?;
        std::fs::write(target.join(MIGRATION_RECEIPT_FILE_NAME), bytes)
            .map_err(|error| format!("write migration receipt: {error}"))
    })
}

fn migrate_with_receipt_writer(
    legacy_root: &Path,
    target_root: &Path,
    write_receipt: impl FnOnce(&Path, &MigrationReceiptV1) -> Result<(), String>,
) -> Result<MigrationReceiptV1, String> {
    if std::fs::symlink_metadata(target_root).is_ok() {
        return Err(format!("target {} already exists", target_root.display()));
    }
    let source_root = legacy_root
        .canonicalize()
        .map_err(|error| format!("canonical legacy root: {error}"))?;
    let parent = target_root
        .parent()
        .ok_or_else(|| "target has no parent".to_string())?
        .canonicalize()
        .map_err(|error| format!("canonical target parent: {error}"))?;
    let target_name = target_root
        .file_name()
        .ok_or_else(|| "target has no final path component".to_string())?;
    let target = parent.join(target_name);
    if target.starts_with(&source_root) || source_root.starts_with(&target) {
        return Err("legacy root and target must be disjoint".into());
    }
    let mut found: Vec<(&str, PathBuf)> = Vec::new();
    for (name, relative) in LEGACY_LAYOUTS {
        if let Some(path) = safe_legacy_layout(&source_root, relative)? {
            found.push((name, path));
        }
    }
    if found.is_empty() {
        return Err("no recognized legacy installation layout".into());
    }
    for (_, path) in &found {
        if PID_FILES.iter().any(|name| path.join(name).exists()) {
            return Err(format!(
                "legacy supervisor marker present under {}; stop legacy daemon first",
                path.display()
            ));
        }
    }
    std::fs::create_dir(&target).map_err(|error| format!("create target: {error}"))?;
    let mut layouts: Vec<String> = Vec::new();
    for (name, source) in &found {
        let destination = target.join(name);
        if let Err(error) = std::fs::rename(source, &destination) {
            return Err(transaction_error(
                format!("move {}: {error}", source.display()),
                &layouts,
                &target,
                &source_root,
            ));
        }
        layouts.push((*name).to_string());
    }
    let receipt = MigrationReceiptV1 {
        schema_version: MIGRATION_RECEIPT_SCHEMA_VERSION,
        source: source_root.clone(),
        target: target.clone(),
        layouts,
        outcome: "migrated",
        rollback: "rename each target layout back to its recorded legacy path",
    };
    if let Err(error) = write_receipt(&target, &receipt) {
        return Err(transaction_error(
            error,
            &receipt.layouts,
            &target,
            &source_root,
        ));
    }
    Ok(receipt)
}

/// Returns a recognized layout only when every component below the canonical
/// legacy root is a real directory. This refuses both a layout symlink and an
/// intermediate symlink before a target is created or any state is moved.
fn safe_legacy_layout(source_root: &Path, relative: &str) -> Result<Option<PathBuf>, String> {
    let mut path = source_root.to_path_buf();
    for component in Path::new(relative).components() {
        let std::path::Component::Normal(component) = component else {
            return Err(format!("invalid legacy layout {relative}"));
        };
        path.push(component);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("inspect legacy layout {}: {error}", path.display())),
        };
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "legacy layout component is a symlink: {}",
                path.display()
            ));
        }
    }
    if !path.is_dir() {
        return Ok(None);
    }
    Ok(Some(path))
}

fn transaction_error(error: String, moved: &[String], target: &Path, source_root: &Path) -> String {
    let rollback = (|| -> Result<(), String> {
        let receipt = target.join(MIGRATION_RECEIPT_FILE_NAME);
        if std::fs::symlink_metadata(&receipt).is_ok() {
            std::fs::remove_file(&receipt).map_err(|error| format!("remove receipt: {error}"))?;
        }
        for name in moved.iter().rev() {
            let relative = LEGACY_LAYOUTS
                .iter()
                .find(|(layout, _)| *layout == name)
                .expect("known layout")
                .1;
            std::fs::rename(target.join(name), source_root.join(relative))
                .map_err(|error| format!("restore {name}: {error}"))?;
        }
        std::fs::remove_dir(target).map_err(|error| format!("remove target: {error}"))
    })();
    match rollback {
        Ok(()) => error,
        Err(rollback) => format!("{error}; rollback failed: {rollback}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_studio_layouts_move_data_grants_events_and_aliases_without_copying() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        for relative in ["tools/.cache/memory", ".claude/crypt"] {
            let dir = source.join(relative);
            std::fs::create_dir_all(&dir).unwrap();
            for file in [
                "crypt-engine.db",
                "grants.json",
                "events.jsonl",
                "aliases.json",
            ] {
                std::fs::write(dir.join(file), format!("{relative}:{file}")).unwrap();
            }
        }
        let receipt = migrate(&source, &target).unwrap();
        assert_eq!(receipt.layouts, ["workspace-cache", "home-crypt"]);
        assert!(target.join(MIGRATION_RECEIPT_FILE_NAME).is_file());
        for (name, _) in LEGACY_LAYOUTS {
            for file in [
                "crypt-engine.db",
                "grants.json",
                "events.jsonl",
                "aliases.json",
            ] {
                assert!(target.join(name).join(file).is_file());
            }
        }
        assert!(!source.join("tools/.cache/memory").exists());
        assert!(!source.join(".claude/crypt").exists());
    }
    #[test]
    fn active_legacy_marker_refuses_before_any_data_moves() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let dir = source.join("tools/.cache/memory");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("supervisor.pid"), "42").unwrap();
        assert!(migrate(&source, &target)
            .unwrap_err()
            .contains("stop legacy daemon"));
        assert!(dir.exists());
        assert!(!target.exists());
    }
    #[test]
    fn nested_target_refuses_before_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = source.join("target");
        std::fs::create_dir_all(source.join("tools/.cache/memory")).unwrap();
        assert!(migrate(&source, &target).unwrap_err().contains("disjoint"));
        assert!(!target.exists());
    }
    #[test]
    fn receipt_failure_restores_every_renamed_layout() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        for (_, relative) in LEGACY_LAYOUTS {
            std::fs::create_dir_all(source.join(relative)).unwrap();
        }
        assert!(migrate_with_receipt_writer(&source, &target, |_, _| Err(
            "fixture receipt failure".into()
        ))
        .unwrap_err()
        .contains("fixture receipt failure"));
        for (_, relative) in LEGACY_LAYOUTS {
            assert!(source.join(relative).is_dir());
        }
        assert!(!target.exists());
    }
    #[cfg(unix)]
    #[test]
    fn leaf_symlink_refuses_before_any_data_moves() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(source.join("tools/.cache")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("crypt-engine.db"), "outside").unwrap();
        std::os::unix::fs::symlink(&outside, source.join("tools/.cache/memory")).unwrap();
        assert!(migrate(&source, &target).unwrap_err().contains("symlink"));
        assert!(source
            .join("tools/.cache/memory")
            .symlink_metadata()
            .is_ok());
        assert!(outside.join("crypt-engine.db").is_file());
        assert!(!target.exists());
    }
    #[cfg(unix)]
    #[test]
    fn intermediate_symlink_refuses_before_any_data_moves() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(outside.join(".cache/memory")).unwrap();
        std::fs::write(outside.join(".cache/memory/crypt-engine.db"), "outside").unwrap();
        std::os::unix::fs::symlink(&outside, source.join("tools")).unwrap();
        assert!(migrate(&source, &target).unwrap_err().contains("symlink"));
        assert!(source.join("tools").symlink_metadata().is_ok());
        assert!(outside.join(".cache/memory/crypt-engine.db").is_file());
        assert!(!target.exists());
    }
}
