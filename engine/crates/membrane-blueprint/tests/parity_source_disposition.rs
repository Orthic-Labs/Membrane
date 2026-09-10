//! Port of `blueprint/tests/*source-disposition*.mjs`: every tracked path
//! gets a terminal outcome, non-git roots report admitted-only scope, and
//! policy-excluded directories are classified `ignored_policy` rather than
//! silently dropped.

use membrane_blueprint::providers::source_disposition::{audit_source_dispositions, Disposition, Scope};
use std::fs;
use std::process::Command;
use tempfile::tempdir;

fn init_git_repo(dir: &std::path::Path) {
    let run = |args: &[&str]| {
        let status = Command::new("git").arg("-C").arg(dir).args(args).status().expect("git available on PATH");
        assert!(status.success(), "git {args:?} failed");
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
}

#[test]
fn non_git_root_is_admitted_only_and_complete() {
    let dir = tempdir().unwrap();
    let report = audit_source_dispositions(dir.path(), &["src/main.rs".to_string()]);
    assert!(matches!(report.scope, Scope::AdmittedOnly));
    assert!(report.complete);
    assert_eq!(report.considered, 1);
    assert_eq!(report.indexed, 1);
    assert!(report.exceptions.is_empty());
}

#[test]
fn every_tracked_path_gets_a_terminal_outcome() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("node_modules")).unwrap();
    init_git_repo(dir.path());
    fs::write(dir.path().join("admitted.rs"), b"fn main() {}").unwrap();
    fs::write(dir.path().join("unindexed.md"), b"# docs").unwrap();
    fs::write(dir.path().join("node_modules/dep.js"), b"module.exports = {}").unwrap();
    Command::new("git").arg("-C").arg(dir.path()).args(["add", "-A"]).status().unwrap();

    let report = audit_source_dispositions(dir.path(), &["admitted.rs".to_string()]);
    assert!(matches!(report.scope, Scope::GitTracked));
    assert_eq!(report.considered, 3);
    assert_eq!(report.indexed, 1);
    assert_eq!(report.terminal, Some(3));
    assert!(report.complete, "indexed + exceptions must account for every tracked path");

    let node_modules_entry = report.exceptions.iter().find(|e| e.path == "node_modules/dep.js").expect("node_modules excluded by policy, not silently dropped");
    assert_eq!(node_modules_entry.disposition, Disposition::IgnoredPolicy);
    assert_eq!(node_modules_entry.reason, "primary_scan_exclusion");

    let unindexed_entry = report.exceptions.iter().find(|e| e.path == "unindexed.md").expect("tracked-but-not-admitted path must appear as an exception");
    assert_eq!(unindexed_entry.disposition, Disposition::Unsupported);
}

#[test]
fn missing_tracked_file_is_reported_failed_not_silently_skipped() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());
    fs::write(dir.path().join("gone.rs"), b"fn main() {}").unwrap();
    Command::new("git").arg("-C").arg(dir.path()).args(["add", "-A"]).status().unwrap();
    fs::remove_file(dir.path().join("gone.rs")).unwrap();

    let report = audit_source_dispositions(dir.path(), &[]);
    let entry = report.exceptions.iter().find(|e| e.path == "gone.rs").expect("git-tracked-but-deleted path must be classified, not dropped");
    assert_eq!(entry.disposition, Disposition::Failed);
    assert_eq!(entry.reason, "tracked_path_missing");
}
