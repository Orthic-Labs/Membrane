//! Parity test for `blueprint/src/lib/update/rollback.mjs` (scoped -- see
//! lib_update_rollback.rs module docs).

use membrane_blueprint::lib_update_apply::copy_recursive;
use membrane_blueprint::lib_update_rollback::{is_valid_digest, receipt_is_self_consistent, RollbackReceipt};
use std::fs;

#[test]
fn valid_digest_requires_exactly_64_lowercase_or_digit_hex_chars() {
    assert!(is_valid_digest(&"0".repeat(64)));
    assert!(is_valid_digest(&"f".repeat(64)));
    assert!(!is_valid_digest(&"f".repeat(63)));
    assert!(!is_valid_digest("not-hex-at-all-and-wrong-length"));
}

#[test]
fn digest_check_is_lowercase_hex_only_matching_the_legacy_regex() {
    // The legacy validDigest regex is /^[a-f0-9]{64}$/ -- lowercase only.
    assert!(is_valid_digest(&"a".repeat(64)));
    assert!(!is_valid_digest(&"A".repeat(64)));
}

#[test]
fn a_fully_valid_receipt_is_self_consistent() {
    let receipt = RollbackReceipt {
        current_app_digest: "a".repeat(64),
        prior_app_digest: "b".repeat(64),
        current_package_version: "3.1.0".to_string(),
        prior_package_version: "3.0.5".to_string(),
    };
    assert!(receipt_is_self_consistent(&receipt));
}

#[test]
fn a_receipt_with_a_malformed_digest_is_rejected() {
    let receipt = RollbackReceipt {
        current_app_digest: "short".to_string(),
        prior_app_digest: "b".repeat(64),
        current_package_version: "3.1.0".to_string(),
        prior_package_version: "3.0.5".to_string(),
    };
    assert!(!receipt_is_self_consistent(&receipt));
}

#[test]
fn a_receipt_with_a_non_semver_version_is_rejected() {
    let receipt = RollbackReceipt {
        current_app_digest: "a".repeat(64),
        prior_app_digest: "b".repeat(64),
        current_package_version: "latest".to_string(),
        prior_package_version: "3.0.5".to_string(),
    };
    assert!(!receipt_is_self_consistent(&receipt));
}

#[test]
fn rollback_restores_a_prior_app_directory_via_the_shared_copy_recursive() {
    let prior = tempfile::tempdir().unwrap();
    fs::write(prior.path().join("app.js"), b"prior build").unwrap();
    let restored = tempfile::tempdir().unwrap().path().join("app");
    copy_recursive(prior.path(), &restored).unwrap();
    assert_eq!(fs::read(restored.join("app.js")).unwrap(), b"prior build");
}
