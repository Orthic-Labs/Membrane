//! Parity tests for `blueprint doctor --repair-plan[--apply-repair[--yes]]
//! --json` and `blueprint service support-bundle`
//! (`blueprint/tests/doctor-repair.test.mjs`,
//! `blueprint/tests/support-bundle-redaction.test.mjs`), ported to the
//! native `membrane_blueprint::cli::{repair_plan, apply_repair,
//! support_bundle}` dispatch.

use membrane_blueprint::cli;
use std::fs;

#[test]
fn repair_plan_is_ordered_and_non_destructive_for_a_fresh_missing_store() {
    let dir = tempfile::tempdir().unwrap();
    let plan = cli::repair_plan(dir.path().to_string_lossy().to_string());
    assert_eq!(plan["schemaVersion"], 1);
    let actions = plan["actions"].as_array().unwrap();
    assert!(!actions.is_empty());
    assert_eq!(actions[0]["id"], "rebuild-graph");
    assert_eq!(actions[0]["reversible"], false);
}

#[test]
fn repair_plan_no_ops_once_the_store_has_been_built() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let root = dir.path().to_string_lossy().to_string();
    cli::manual_refresh(root.clone(), None).expect("manual_refresh should succeed on a plain repo");
    let plan = cli::repair_plan(root);
    let actions = plan["actions"].as_array().unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["id"], "no-op");
}

#[test]
fn apply_repair_without_yes_requires_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let result = cli::apply_repair(dir.path().to_string_lossy().to_string(), false);
    let error = result.expect_err("apply_repair without yes must refuse");
    assert_eq!(error.code, "confirmation_required");
}

/// A corrupted-store fixture: `.agent/graph/graph.db` exists but is not a
/// valid SQLite file, matching how a truncated/torn write would leave the
/// on-disk store. `apply_repair(..., yes: true)` must recognize the
/// corrupt-store blocker, plan a rebuild, and actually rebuild the store
/// through the native engine's own build path -- proving the repair fixes
/// the fixture rather than merely reporting on it.
#[test]
fn apply_repair_with_yes_rebuilds_a_corrupted_store() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let graph_dir = dir.path().join(".agent").join("graph");
    fs::create_dir_all(&graph_dir).unwrap();
    fs::write(graph_dir.join("graph.db"), b"corrupted, not a real sqlite file").unwrap();
    let root = dir.path().to_string_lossy().to_string();

    let before = cli::doctor(root.clone(), false);
    assert_eq!(before["state"], "corrupt");

    let applied = cli::apply_repair(root.clone(), true).expect("apply_repair with yes should succeed");
    let actions = applied["applied"].as_array().unwrap();
    assert!(actions.iter().any(|a| a["id"] == "rebuild-graph" && a["status"] == "applied"), "rebuild-graph action missing or not applied: {actions:?}");

    let after = cli::doctor(root, false);
    assert_ne!(after["state"], "corrupt", "store should no longer be corrupt after apply_repair rebuilt it");
    assert_ne!(after["state"], "missing");
}

#[test]
fn apply_repair_no_ops_cleanly_when_nothing_needs_repair() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let root = dir.path().to_string_lossy().to_string();
    cli::manual_refresh(root.clone(), None).expect("manual_refresh should succeed");
    let applied = cli::apply_repair(root, true).expect("apply_repair should succeed");
    let actions = applied["applied"].as_array().unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["id"], "no-op");
    assert_eq!(actions[0]["status"], "applied");
}

#[test]
fn support_bundle_writes_the_legacy_allowlisted_redacted_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let root = dir.path().to_string_lossy().to_string();
    cli::manual_refresh(root.clone(), None).expect("manual_refresh should succeed");

    let bundle = cli::support_bundle(root, None).expect("support_bundle should succeed");
    let path = std::path::PathBuf::from(bundle["path"].as_str().unwrap());
    assert!(path.join("summary.json").exists());
    assert!(path.join("doctor.json").exists());
    assert!(path.join("repair-plan.json").exists());
    assert!(path.join("checksums.txt").exists());

    let summary: serde_json::Value = serde_json::from_str(&fs::read_to_string(path.join("summary.json")).unwrap()).unwrap();
    assert_eq!(summary["product"], "blueprint");
    // The repo root itself must not appear verbatim in the redacted summary.
    let repo_root_str = dir.path().to_string_lossy().to_string();
    assert!(!summary["repoRoot"].as_str().unwrap().contains(&repo_root_str));

    let checksums = bundle["checksums"].as_array().unwrap();
    assert!(!checksums.is_empty());
}
