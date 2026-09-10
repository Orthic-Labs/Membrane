//! Native Rust port of `blueprint/src/lib/update/rollback.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `rollback`/`valid_digest`/receipt-bound-restore across
//! membrane-blueprint/src and membrane-runtime/src produced no match).
//!
//! SCOPE: the legacy module orchestrates a full app/store swap with journal
//! recovery; that OS-transaction choreography is not replicated here (see
//! `lib_update_apply.rs` module docs for the same rationale). This port
//! covers the deterministic, testable receipt-binding contract: `is_valid_
//! digest` (the sha256-hex shape check gating every rollback decision) and
//! `receipt_is_self_consistent`, which mirrors the exact binding checks
//! `rollback(...)` performs against a receipt before trusting it
//! (`currentAppDigest`/`priorAppDigest` must be valid digests, package
//! versions must be semver-shaped) — the precondition every real rollback
//! must satisfy before any file is touched.

/// Mirrors `validDigest(value)`: the legacy regex is `/^[a-f0-9]{64}$/`
/// (lowercase hex only, matching `sha256`/`hex.encode` output).
pub fn is_valid_digest(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

fn is_semver_prefixed(value: &str) -> bool {
    let mut parts = value.splitn(4, '.');
    let major = parts.next().unwrap_or("");
    let minor = parts.next().unwrap_or("");
    let patch_and_rest = parts.next().unwrap_or("");
    let patch: String = patch_and_rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    !major.is_empty()
        && major.chars().all(|c| c.is_ascii_digit())
        && !minor.is_empty()
        && minor.chars().all(|c| c.is_ascii_digit())
        && !patch.is_empty()
}

#[derive(Debug, Clone)]
pub struct RollbackReceipt {
    pub current_app_digest: String,
    pub prior_app_digest: String,
    pub current_package_version: String,
    pub prior_package_version: String,
}

/// Mirrors the receipt-binding validity checks inside `rollback(...)`
/// before it trusts a receipt (digest shape + semver-prefixed versions).
pub fn receipt_is_self_consistent(receipt: &RollbackReceipt) -> bool {
    is_valid_digest(&receipt.current_app_digest)
        && is_valid_digest(&receipt.prior_app_digest)
        && is_semver_prefixed(&receipt.current_package_version)
        && is_semver_prefixed(&receipt.prior_package_version)
}

/// Bind rollback to the exact current/prior directory trees and repository
/// scope before any copy or rename. Shape-valid receipt fields alone are not
/// evidence that those directories contain the versions named by receipt.
pub fn validate_rollback_binding(
    root: &std::path::Path,
    current_app_dir: &std::path::Path,
    prior_app_dir: &std::path::Path,
    receipt: &RollbackReceipt,
) -> Result<(), &'static str> {
    if !receipt_is_self_consistent(receipt) {
        return Err("rollback_receipt_invalid");
    }
    if current_app_dir == prior_app_dir
        || crate::security::is_confined_path(root, current_app_dir, false).is_err()
        || crate::security::is_confined_path(root, prior_app_dir, false).is_err()
    {
        return Err("unsafe rollback target");
    }
    let current_physical = std::fs::canonicalize(current_app_dir)
        .map_err(|_| "rollback_current_app_unreadable")?;
    let prior_physical = std::fs::canonicalize(prior_app_dir)
        .map_err(|_| "rollback_prior_app_unreadable")?;
    if current_physical == prior_physical {
        return Err("unsafe rollback target");
    }
    let current_digest = crate::lib_update_manifest::tree_digest(current_app_dir)
        .map_err(|_| "rollback_current_app_unreadable")?;
    let prior_digest = crate::lib_update_manifest::tree_digest(prior_app_dir)
        .map_err(|_| "rollback_prior_app_unreadable")?;
    if current_digest != receipt.current_app_digest || prior_digest != receipt.prior_app_digest {
        return Err("receipt app binding mismatch");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_digest_requires_64_hex_chars() {
        assert!(is_valid_digest(&"a".repeat(64)));
        assert!(!is_valid_digest(&"a".repeat(63)));
        assert!(!is_valid_digest(&"g".repeat(64)));
    }

    #[test]
    fn receipt_is_self_consistent_requires_valid_digests_and_semver() {
        let receipt = RollbackReceipt {
            current_app_digest: "a".repeat(64),
            prior_app_digest: "b".repeat(64),
            current_package_version: "1.2.3".to_string(),
            prior_package_version: "1.2.2".to_string(),
        };
        assert!(receipt_is_self_consistent(&receipt));
    }

    #[test]
    fn receipt_with_bad_digest_is_rejected() {
        let receipt = RollbackReceipt {
            current_app_digest: "not-a-digest".to_string(),
            prior_app_digest: "b".repeat(64),
            current_package_version: "1.2.3".to_string(),
            prior_package_version: "1.2.2".to_string(),
        };
        assert!(!receipt_is_self_consistent(&receipt));
    }

    #[test]
    fn receipt_with_non_semver_version_is_rejected() {
        let receipt = RollbackReceipt {
            current_app_digest: "a".repeat(64),
            prior_app_digest: "b".repeat(64),
            current_package_version: "not-a-version".to_string(),
            prior_package_version: "1.2.2".to_string(),
        };
        assert!(!receipt_is_self_consistent(&receipt));
    }

    #[test]
    fn copy_recursive_is_reused_from_update_apply_without_duplication() {
        // rollback()'s file-copy step uses the exact same symlink-refusing
        // recursive copy as apply(); prove the shared implementation works
        // for rollback's use shape (copying a whole prior-app directory).
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("app.txt"), b"prior-version").unwrap();
        let dst = tempfile::tempdir().unwrap().path().join("restored");
        crate::lib_update_apply::copy_recursive(src.path(), &dst).unwrap();
        assert_eq!(std::fs::read(dst.join("app.txt")).unwrap(), b"prior-version");
    }
}
