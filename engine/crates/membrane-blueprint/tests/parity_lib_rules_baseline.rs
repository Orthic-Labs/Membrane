//! Parity test for `blueprint/src/lib/rules/baseline.mjs`.
//!
//! The baseline store is process-global (mirroring the legacy module-level
//! `Map`), so these tests use a unique name per test to stay independent
//! without needing external synchronization.

use membrane_blueprint::lib_rules_baseline::{
    capture_named_baseline, changed_slice, clear_named_baselines, get_named_baseline,
    list_named_baselines, BaselineError, FindingRef,
};

fn finding(fp: &str) -> FindingRef {
    FindingRef {
        fingerprint: fp.to_string(),
        rule_id: Some("r".to_string()),
        path: None,
        name: None,
        specifier: None,
    }
}

#[test]
fn changed_slice_only_reports_findings_absent_from_the_baseline() {
    let baseline = vec![finding("a"), finding("b")];
    let findings = vec![finding("a"), finding("b"), finding("c"), finding("d")];
    let slice = changed_slice(&findings, Some(&baseline));
    assert_eq!(slice.total, 4);
    assert_eq!(slice.retained_total, 4);
    assert_eq!(slice.new_count, 2);
    let fps: Vec<_> = slice.findings.iter().map(|f| f.fingerprint.as_str()).collect();
    assert_eq!(fps, vec!["c", "d"]);
}

#[test]
fn changed_slice_against_no_baseline_reports_everything_as_new() {
    let findings = vec![finding("x"), finding("y")];
    let slice = changed_slice(&findings, None);
    assert_eq!(slice.new_count, 2);
}

#[test]
fn capture_sorts_fingerprints_and_records_generation_id() {
    let summary = capture_named_baseline(
        "parity-test-capture",
        "gen-42",
        vec![finding("z"), finding("a"), finding("m")],
        Some("2026-02-01T00:00:00.000Z".to_string()),
    )
    .unwrap();
    assert_eq!(
        summary.findings_fingerprints,
        vec!["a".to_string(), "m".to_string(), "z".to_string()]
    );
    assert_eq!(summary.generation_id, "gen-42");
    assert_eq!(summary.finding_count, 3);
}

#[test]
fn get_and_list_reflect_captured_baselines_sorted_by_name() {
    // NOTE: the baseline store is process-global (mirrors the legacy
    // module-level Map) and cargo test runs tests within one binary
    // concurrently by default, so this test avoids clear_named_baselines()
    // (which would race with other tests' captures in this same binary)
    // and instead uses names unique to this test plus a filtered,
    // still-sorted view of the shared store.
    capture_named_baseline("parity-test-list-b", "g1", vec![finding("1")], None).unwrap();
    capture_named_baseline("parity-test-list-a", "g2", vec![finding("2")], None).unwrap();
    let listed = list_named_baselines();
    let names: Vec<_> = listed
        .iter()
        .map(|b| b.name.as_str())
        .filter(|n| n.starts_with("parity-test-list-"))
        .collect();
    assert_eq!(names, vec!["parity-test-list-a", "parity-test-list-b"]);
    assert!(get_named_baseline("parity-test-list-a").is_some());
    assert!(get_named_baseline("definitely-does-not-exist-xyz").is_none());
}

#[test]
fn empty_or_whitespace_name_is_rejected() {
    assert!(matches!(
        capture_named_baseline("   ", "g", vec![], None).unwrap_err(),
        BaselineError::NameInvalid
    ));
}

#[test]
fn missing_generation_id_is_rejected() {
    assert!(matches!(
        capture_named_baseline("parity-test-missing-gen", "", vec![], None).unwrap_err(),
        BaselineError::GenerationMissing
    ));
}
