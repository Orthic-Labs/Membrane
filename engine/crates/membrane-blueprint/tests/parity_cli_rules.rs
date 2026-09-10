//! Parity tests for `blueprint rules [check|baseline|explain]`
//! (`blueprint/scripts/cli/commands.mjs:458` case `"rules"`), ported to the
//! native `membrane_blueprint::cli::rules` dispatch.

use membrane_blueprint::cli;
use std::fs;

fn write_rules(root: &std::path::Path) {
    fs::write(
        root.join("blueprint.rules.yml"),
        "- id: no-ui-to-db\n  from:\n    path: \"src/ui/**\"\n  disallow:\n    to:\n      path: \"src/db/**\"\n  severity: error\n- id: no-legacy-import\n  from:\n    path: \"src/new/**\"\n  disallow:\n    to:\n      path: \"src/legacy/**\"\n  severity: warn\n",
    )
    .unwrap();
}

#[test]
fn missing_rules_file_is_a_typed_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = cli::rules(dir.path().to_string_lossy().to_string(), None).unwrap_err();
    assert_eq!(err.code, "rules_missing");
}

#[test]
fn check_defaults_and_reports_id_and_severity_per_rule() {
    let dir = tempfile::tempdir().unwrap();
    write_rules(dir.path());
    let payload = cli::rules(dir.path().to_string_lossy().to_string(), None).unwrap();
    assert_eq!(payload["schemaVersion"], 1);
    assert_eq!(payload["ruleCount"], 2);
    let rules = payload["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0]["id"], "no-ui-to-db");
    assert_eq!(rules[0]["severity"], "error");
}

#[test]
fn baseline_and_explain_report_command_and_rule_count() {
    let dir = tempfile::tempdir().unwrap();
    write_rules(dir.path());
    let baseline = cli::rules(dir.path().to_string_lossy().to_string(), Some("baseline")).unwrap();
    assert_eq!(baseline["command"], "baseline");
    assert_eq!(baseline["ruleCount"], 2);
    let explain = cli::rules(dir.path().to_string_lossy().to_string(), Some("explain")).unwrap();
    assert_eq!(explain["command"], "explain");
    assert_eq!(explain["ruleCount"], 2);
}

#[test]
fn unknown_subcommand_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    write_rules(dir.path());
    let err = cli::rules(dir.path().to_string_lossy().to_string(), Some("bogus")).unwrap_err();
    assert_eq!(err.code, "usage");
}
