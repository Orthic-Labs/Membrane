//! Parity tests for `blueprint doctor [--full] [--json]`
//! (`blueprint/scripts/cli/commands.mjs` doctor path +
//! `blueprint/src/lib/operations/doctor.mjs`), ported to the native
//! `membrane_blueprint::cli::doctor` dispatch. See
//! `crate::cli`'s module docs for why native readiness is checked against
//! the SQLite store (`.agent/graph/graph.db`) instead of legacy's
//! `.agent/map.json`.

use membrane_blueprint::cli;
use std::fs;

#[test]
fn doctor_reports_missing_state_for_a_fresh_repo_with_no_store() {
    let dir = tempfile::tempdir().unwrap();
    let result = cli::doctor(dir.path().to_string_lossy().to_string(), false);
    assert_eq!(result["schemaVersion"], 1);
    assert_eq!(result["state"], "missing");
    assert!(result["errors"].as_array().unwrap().iter().any(|e| e.as_str().unwrap().contains("run build/refresh first")));
    assert!(result["reasons"].as_array().unwrap().iter().any(|r| r["code"] == "missing_map"));
}

#[test]
fn doctor_reports_corrupt_state_for_an_unreadable_store_file() {
    let dir = tempfile::tempdir().unwrap();
    let graph_dir = dir.path().join(".agent").join("graph");
    fs::create_dir_all(&graph_dir).unwrap();
    // Not a valid SQLite file -- the store open/read path must fail closed,
    // not panic or silently treat this as an empty/ready store.
    fs::write(graph_dir.join("graph.db"), b"this is not a sqlite database").unwrap();
    let result = cli::doctor(dir.path().to_string_lossy().to_string(), false);
    assert_eq!(result["state"], "corrupt");
    assert!(result["reasons"].as_array().unwrap().iter().any(|r| r["code"] == "corrupt_map"));
}

#[test]
fn doctor_full_flag_adds_a_completion_check_marker() {
    let dir = tempfile::tempdir().unwrap();
    let shallow = cli::doctor(dir.path().to_string_lossy().to_string(), false);
    let full = cli::doctor(dir.path().to_string_lossy().to_string(), true);
    assert!(shallow["completion"].is_null());
    assert_eq!(full["completion"]["checked"], true);
}

#[test]
fn doctor_builds_a_real_store_via_manual_refresh_then_reports_degraded_not_missing() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let root = dir.path().to_string_lossy().to_string();
    // Real repository build through the same native engine path `blueprint
    // build`/`refresh` uses -- not a hand-authored map.json fixture.
    let refreshed = cli::manual_refresh(root.clone(), None);
    assert!(refreshed.is_ok(), "manual_refresh failed: {:?}", refreshed.err());
    assert!(dir.path().join(".agent").join("graph").join("graph.db").exists());
    let result = cli::doctor(root, false);
    assert_ne!(result["state"], "missing");
    assert_ne!(result["state"], "corrupt");
}

#[test]
fn doctor_json_shape_matches_legacy_top_level_keys() {
    let dir = tempfile::tempdir().unwrap();
    let result = cli::doctor(dir.path().to_string_lossy().to_string(), false);
    for key in ["schemaVersion", "state", "generatedAt", "artifacts", "errors", "warnings", "reasons"] {
        assert!(result.get(key).is_some(), "missing legacy-shaped key: {key}");
    }
    assert!(result["generatedAt"].as_str().unwrap().ends_with('Z'));
}
