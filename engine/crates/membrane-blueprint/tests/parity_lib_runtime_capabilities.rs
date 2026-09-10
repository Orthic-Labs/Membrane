//! Parity tests for `lib_runtime_capabilities` (native port of the portable
//! semver-floor logic in `blueprint/src/lib/runtime-capabilities.mjs`),
//! lane LIB4 (r5 closure).

use membrane_blueprint::lib_runtime_capabilities::{
    parse_major_minor, supports_current, RuntimeVersion,
};

#[test]
fn parses_full_triple_with_v_prefix() {
    assert_eq!(
        parse_major_minor("v22.22.3"),
        Some(RuntimeVersion { major: 22, minor: 22, patch: 3 })
    );
}

#[test]
fn parses_without_v_prefix_and_without_patch() {
    assert_eq!(
        parse_major_minor("22.22"),
        Some(RuntimeVersion { major: 22, minor: 22, patch: 0 })
    );
}

#[test]
fn rejects_unparseable_version() {
    assert_eq!(parse_major_minor("not-a-version"), None);
    assert_eq!(parse_major_minor(""), None);
}

#[test]
fn supports_current_true_when_above_floor() {
    assert!(supports_current("v22.22.3", ">=22.22.3"));
    assert!(supports_current("v22.23.0", ">=22.22.3"));
    assert!(supports_current("v23.0.0", ">=22.22.3"));
}

#[test]
fn supports_current_false_when_below_floor() {
    assert!(!supports_current("v20.10.0", ">=22.22.3"));
    assert!(!supports_current("v22.22.2", ">=22.22.3"));
    assert!(!supports_current("v22.21.9", ">=22.22.3"));
}

#[test]
fn supports_current_handles_short_floor_forms() {
    assert!(supports_current("v22.0.0", ">=22"));
    assert!(supports_current("v22.5.0", ">=22.0"));
    assert!(!supports_current("v21.9.9", ">=22"));
}

#[test]
fn supports_current_false_for_unparseable_inputs() {
    assert!(!supports_current("garbage", ">=22.22.3"));
    assert!(!supports_current("v22.22.3", "no floor here"));
}
