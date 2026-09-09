use membrane_blueprint::contracts::ScopeGrantV1;
use membrane_blueprint::security;
use serde_json::json;
use std::fs;
use std::path::Path;

fn grant(root: &Path) -> ScopeGrantV1 {
    ScopeGrantV1 { task_id: "task".into(), repo_root: root.to_string_lossy().into_owned(), generation_id: Some("gen".into()), receipt_id: "receipt".into(), paths: vec!["src/**".into()], issued_ms: 10, ttl_ms: 100, signature: "a".repeat(64) }
}

fn accept_grant(_: &ScopeGrantV1) -> bool { true }

#[test]
fn confinement_rejects_traversal_prefix_and_symlink_escape() {
    let root = tempfile::tempdir().unwrap(); let outside = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    assert!(security::is_confined_path(root.path(), &root.path().join("src/file"), false).is_ok());
    assert!(security::is_confined_path(root.path(), &root.path().join("src/../outside"), false).is_err());
    assert!(security::is_confined_path(root.path(), Path::new(r"src\..\outside"), false).is_err());
    #[cfg(unix)] std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
    #[cfg(windows)] std::os::windows::fs::symlink_dir(outside.path(), root.path().join("escape")).unwrap();
    assert!(security::is_confined_path(root.path(), &root.path().join("escape/secret"), false).is_err());
    let prefix_sibling = root.path().with_file_name(format!("{}-sibling", root.path().file_name().unwrap().to_string_lossy()));
    fs::create_dir_all(&prefix_sibling).unwrap();
    assert!(security::is_confined_path(root.path(), &prefix_sibling.join("secret"), false).is_err());
    let sibling = tempfile::tempdir_in(root.path().parent().unwrap()).unwrap();
    assert!(security::is_confined_path(root.path(), sibling.path().join("secret").as_path(), false).is_err());
    fs::remove_dir_all(prefix_sibling).unwrap();
    #[cfg(unix)] {
        let dangling = root.path().join("dangling");
        std::os::unix::fs::symlink(root.path().join("missing-target"), &dangling).unwrap();
        assert!(security::is_confined_path(root.path(), &dangling, false).is_err());
    }
    #[cfg(windows)] {
        let variant = root.path().to_string_lossy().to_uppercase();
        assert!(security::is_confined_path(root.path(), Path::new(&variant), true).is_ok());
    }
}

#[test]
fn grant_paths_are_segment_bounded_and_platform_normalized() {
    let root = tempfile::tempdir().unwrap(); fs::create_dir_all(root.path().join("src")).unwrap();
    let mut g = grant(root.path());
    g.paths = vec!["src/*".into()];
    assert!(security::validate_scope_grant(&g, "task", root.path(), Some("gen"), "src/file.rs", 20, &accept_grant).is_ok());
    assert!(security::validate_scope_grant(&g, "task", root.path(), Some("gen"), "src/nested/file.rs", 20, &accept_grant).is_err());
    g.paths = vec!["src/?".into()];
    assert!(security::validate_scope_grant(&g, "task", root.path(), Some("gen"), "src/x", 20, &accept_grant).is_ok());
    assert!(security::validate_scope_grant(&g, "task", root.path(), Some("gen"), "src/x/y", 20, &accept_grant).is_err());
    g.paths = vec![r"src\**".into()];
    assert!(security::validate_scope_grant(&g, "task", root.path(), Some("gen"), r"src\nested\file.rs", 20, &accept_grant).is_ok());
}

#[test]
fn grant_validation_is_bound_to_identity_expiry_path_and_trusted_verifier() {
    let root = tempfile::tempdir().unwrap(); fs::create_dir_all(root.path().join("src")).unwrap();
    let g = grant(root.path());
    let verifier = |candidate: &ScopeGrantV1| candidate.signature == g.signature;
    assert!(security::validate_scope_grant(&g, "task", root.path(), Some("gen"), "src/lib.rs", 20, &verifier).is_ok());
    assert_eq!(security::validate_scope_grant(&g, "other", root.path(), Some("gen"), "src/lib.rs", 20, &verifier).unwrap_err().0, security::DenialReason::GrantMismatch);
    assert_eq!(security::validate_scope_grant(&g, "task", root.path(), Some("gen"), "src/lib.rs", 110, &verifier).unwrap_err().0, security::DenialReason::GrantExpired);
    assert_eq!(security::validate_scope_grant(&g, "task", root.path(), Some("gen"), "private", 20, &verifier).unwrap_err().0, security::DenialReason::GrantMismatch);
    let mut tampered = g.clone(); tampered.signature.replace_range(..1, "b");
    assert_eq!(security::validate_scope_grant(&tampered, "task", root.path(), Some("gen"), "src/lib.rs", 20, &verifier).unwrap_err().0, security::DenialReason::SignatureInvalid);
}

#[test]
fn token_generation_and_constant_time_check_require_equal_lengths() {
    let token = security::generate_session_token().unwrap(); assert_eq!(token.len(), 32);
    assert!(security::verify_session_token(&token, &token));
    assert!(!security::verify_session_token(&token, &token[..31]));
    let mut changed = token; changed[0] ^= 1; assert!(!security::verify_session_token(&token, &changed));
}

#[test]
fn redaction_is_recursive_and_preserves_only_named_digests() {
    let sha = "a987ea64c5bad3fd208aab1f5a06ef35318ad99d";
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.signature";
    let slack = "xoxa-secret xoxb-secret xoxp-secret xoxr-secret";
    let pem = "-----BEGIN PRIVATE KEY-----\nsecret-body\n-----END PRIVATE KEY-----";
    let input = json!({"nested": [{"api_key": "top-secret", "url": "postgres://u:password@host/db", "pem": pem, "jwt": jwt, "slack": slack}], "generation": {"indexed_revision": sha}, "raw": sha});
    let output = security::redact_for_egress(&input).unwrap();
    assert_eq!(output["nested"][0]["api_key"], "[REDACTED]");
    assert_eq!(output["generation"]["indexed_revision"], sha);
    assert_eq!(output["raw"], "[REDACTED]");
    assert!(!serde_json::to_string(&output).unwrap().contains("password@"));
    let rendered = serde_json::to_string(&output).unwrap();
    assert!(!rendered.contains("secret-body") && !rendered.contains("eyJhbGci") && !rendered.contains("xoxa-secret") && !rendered.contains("xoxb-secret") && !rendered.contains("xoxp-secret") && !rendered.contains("xoxr-secret"));
}

#[test]
fn denials_do_not_include_paths_or_counts() {
    let root = tempfile::tempdir().unwrap(); let g = grant(root.path());
    let error = security::validate_scope_grant(&g, "wrong", root.path(), Some("gen"), "secret/needle", 20, &accept_grant).unwrap_err();
    assert!(!format!("{error:?}").contains("secret"));
    assert!(!security::DenialReason::GrantMismatch.code().contains("needle"));
}
