//! Parity test for `blueprint/src/lib/operations/support-bundle.mjs`.

use membrane_blueprint::lib_operations_support_bundle::{
    build_support_bundle, redact_path, SupportBundleRecords, SUPPORT_BUNDLE_ALLOWLIST,
};
use serde_json::json;
use std::fs;

#[test]
fn allowlist_matches_legacy_fixed_set() {
    assert_eq!(
        SUPPORT_BUNDLE_ALLOWLIST,
        [
            "summary.json",
            "versions.json",
            "installation.json",
            "service-status.json",
            "repository-status.json",
            "doctor.json",
            "repair-plan.json",
            "logs/watchman-tail.log",
            "logs/service-tail.log",
            "checksums.txt",
        ]
    );
}

#[test]
fn redact_path_normalizes_backslashes_to_forward_slashes() {
    let out = redact_path(r"src\module\file.rs", "/home/nobody", "/var/repo");
    assert_eq!(out, "src/module/file.rs");
}

#[test]
fn bundle_contains_no_raw_home_path_when_home_does_not_prefix_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    fs::create_dir_all(&root).unwrap();
    let bundle_root = root.join(".agent/support-bundle");
    let records = SupportBundleRecords {
        package_channel: None,
        installation: Some(json!({ "root": root.to_string_lossy() })),
        service_status: None,
        repository_status: None,
        doctor: None,
        repair_plan: None,
        watchman_log: None,
        service_log: None,
    };
    let home = "/totally/unrelated/home";
    build_support_bundle(&root, &bundle_root, home, "2026-01-01T00:00:00.000Z", "win32-x64", &records).unwrap();
    let installation = fs::read_to_string(bundle_root.join("installation.json")).unwrap();
    assert!(!installation.contains(&root.to_string_lossy().to_string()));
    assert!(installation.contains("$REPO"));
}

#[test]
fn checksums_file_lists_every_written_allowlisted_record() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj2");
    fs::create_dir_all(&root).unwrap();
    let bundle_root = root.join(".agent/support-bundle");
    let records = SupportBundleRecords {
        package_channel: Some("stable".to_string()),
        installation: None,
        service_status: None,
        repository_status: None,
        doctor: None,
        repair_plan: None,
        watchman_log: Some("ok".to_string()),
        service_log: Some("ok".to_string()),
    };
    let result = build_support_bundle(
        &root,
        &bundle_root,
        "/home/nobody",
        "2026-01-01T00:00:00.000Z",
        "linux-x64",
        &records,
    )
    .unwrap();
    // Every allowlisted entry except checksums.txt itself gets a checksum.
    assert_eq!(result.checksums.len(), SUPPORT_BUNDLE_ALLOWLIST.len() - 1);
    let checksums_text = fs::read_to_string(bundle_root.join("checksums.txt")).unwrap();
    for (name, hash) in &result.checksums {
        assert!(checksums_text.contains(&format!("{hash}  {name}")));
    }
}
