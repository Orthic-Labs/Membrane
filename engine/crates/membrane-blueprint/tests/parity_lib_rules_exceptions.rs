//! Parity test for `blueprint/src/lib/rules/exceptions.mjs`.

use membrane_blueprint::lib_rules_exceptions::{
    is_exception_valid, suppressed_by_exception, FindingRef, RuleException,
};

fn exception(scope: &str, expires_ms: Option<i64>, path: Option<&str>) -> RuleException {
    RuleException {
        owner: Some("team-x".to_string()),
        rationale: Some("legacy migration in progress".to_string()),
        expires_ms,
        rule_id: Some("no-legacy-import".to_string()),
        scope: Some(scope.to_string()),
        path: path.map(String::from),
    }
}

#[test]
fn every_required_field_must_be_present() {
    let mut e = exception("global", Some(1_000_000), None);
    e.rationale = None;
    assert!(!is_exception_valid(&e, 0));
}

#[test]
fn expiry_strictly_in_the_future_is_required() {
    assert!(!is_exception_valid(&exception("global", Some(500), None), 500));
    assert!(is_exception_valid(&exception("global", Some(501), None), 500));
}

#[test]
fn missing_expiry_is_invalid() {
    assert!(!is_exception_valid(&exception("global", None, None), 0));
}

#[test]
fn global_scope_suppresses_any_matching_rule_id() {
    let exceptions = vec![exception("global", Some(1_000_000), None)];
    let finding = FindingRef {
        rule_id: "no-legacy-import",
        source: "src/anywhere/thing.rs",
    };
    assert!(suppressed_by_exception(&finding, &exceptions, 0));
}

#[test]
fn path_scope_requires_source_prefix_match() {
    let exceptions = vec![exception("path", Some(1_000_000), Some("src/legacy/"))];
    let inside = FindingRef {
        rule_id: "no-legacy-import",
        source: "src/legacy/old.rs",
    };
    let outside = FindingRef {
        rule_id: "no-legacy-import",
        source: "src/fresh/new.rs",
    };
    assert!(suppressed_by_exception(&inside, &exceptions, 0));
    assert!(!suppressed_by_exception(&outside, &exceptions, 0));
}

#[test]
fn different_rule_id_is_never_suppressed() {
    let exceptions = vec![exception("global", Some(1_000_000), None)];
    let finding = FindingRef {
        rule_id: "some-other-rule",
        source: "src/anywhere/thing.rs",
    };
    assert!(!suppressed_by_exception(&finding, &exceptions, 0));
}

#[test]
fn an_expired_exception_never_suppresses_even_if_otherwise_matching() {
    let exceptions = vec![exception("global", Some(100), None)];
    let finding = FindingRef {
        rule_id: "no-legacy-import",
        source: "src/anywhere/thing.rs",
    };
    assert!(!suppressed_by_exception(&finding, &exceptions, 200));
}
