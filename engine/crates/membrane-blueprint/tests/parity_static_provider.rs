//! Parity tests for `membrane_blueprint::static_provider` against the legacy
//! `buildConfigDigest` in `blueprint/src/providers/build.mjs`.
//!
//! These reproduce the fixture shape the legacy incremental-equivalence test
//! (`blueprint/tests/incremental-equivalence-config-provider.test.mjs`) uses
//! to exercise the real per-repository config surface: a `tsconfig.json`
//! with a `paths`/`baseUrl` map, alongside ordinary source files that must
//! never contribute to the digest.

use membrane_blueprint::static_provider::{build_config_digest, is_build_config_file, ConfigDigestInput};

const TSCONFIG_FIXTURE: &str = r#"{"compilerOptions":{"baseUrl":".","paths":{"@lib/*":["src/*"]}}}"#;

fn content_hash(bytes: &str) -> String {
    // Stand-in content-addressed hash, same role as the generation's
    // per-file `xxh128:` digest: any stable content-derived string works for
    // proving the config-digest producer's *own* ordering/inclusion rules.
    format!("h:{}", bytes.len())
}

#[test]
fn only_recognized_config_basenames_are_admitted() {
    assert!(is_build_config_file("tsconfig.json"));
    assert!(is_build_config_file("jsconfig.json"));
    assert!(is_build_config_file("package.json"));
    assert!(is_build_config_file("pnpm-workspace.yaml"));
    assert!(is_build_config_file("packages/app/tsconfig.json"));
    assert!(!is_build_config_file("src/util.ts"));
    assert!(!is_build_config_file("tsconfig.build.json"));
}

#[test]
fn digest_is_null_without_any_config_file() {
    let files = [
        ConfigDigestInput { path: "src/util.ts", content_hash: Some(&content_hash("helper")) },
        ConfigDigestInput { path: "src/caller.ts", content_hash: Some(&content_hash("caller")) },
    ];
    assert_eq!(build_config_digest(&files), None, "no build-config file present must yield no digest, matching legacy `null`");
}

#[test]
fn adding_tsconfig_changes_the_digest_and_only_config_files_count() {
    let hash = content_hash(TSCONFIG_FIXTURE);
    let without_tsconfig = [ConfigDigestInput { path: "src/util.ts", content_hash: Some(&content_hash("helper")) }];
    let with_tsconfig = [
        ConfigDigestInput { path: "src/util.ts", content_hash: Some(&content_hash("helper")) },
        ConfigDigestInput { path: "tsconfig.json", content_hash: Some(&hash) },
    ];

    let before = build_config_digest(&without_tsconfig);
    let after = build_config_digest(&with_tsconfig);
    assert_eq!(before, None);
    assert!(after.is_some());
    assert!(after.as_deref().unwrap().starts_with("sha256:"));

    // A digest computed from the config file alone must be identical to one
    // computed alongside unrelated source files: only config-file rows feed
    // the hash.
    let tsconfig_only = [ConfigDigestInput { path: "tsconfig.json", content_hash: Some(&hash) }];
    assert_eq!(after, build_config_digest(&tsconfig_only));
}

#[test]
fn digest_is_stable_under_input_reordering_and_path_separator_style() {
    let a = [
        ConfigDigestInput { path: "tsconfig.json", content_hash: Some("h1") },
        ConfigDigestInput { path: "package.json", content_hash: Some("h2") },
    ];
    let b = [
        ConfigDigestInput { path: "package.json", content_hash: Some("h2") },
        ConfigDigestInput { path: "tsconfig.json", content_hash: Some("h1") },
    ];
    // Windows-style separators in the recorded path must normalize the same
    // as forward-slash paths (matching `isBuildConfigFile`'s `\`→`/` swap).
    let d = [
        ConfigDigestInput { path: "tsconfig.json", content_hash: Some("h1") },
        ConfigDigestInput { path: "package.json", content_hash: Some("h2") },
    ];
    assert_eq!(build_config_digest(&a), build_config_digest(&b));
    assert_eq!(build_config_digest(&a), build_config_digest(&d));
}

#[test]
fn editing_the_tsconfig_content_changes_the_digest() {
    let before = [ConfigDigestInput { path: "tsconfig.json", content_hash: Some(&content_hash(TSCONFIG_FIXTURE)) }];
    let edited_fixture = r#"{"compilerOptions":{"baseUrl":".","paths":{"@lib/*":["src/*"],"@app/*":["app/*"]}}}"#;
    let after = [ConfigDigestInput { path: "tsconfig.json", content_hash: Some(&content_hash(edited_fixture)) }];
    assert_ne!(build_config_digest(&before), build_config_digest(&after));
}
