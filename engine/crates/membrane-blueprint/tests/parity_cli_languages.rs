//! Parity tests for `blueprint languages --json`
//! (`blueprint/scripts/cli/commands.mjs:453` case `"languages"` ->
//! `blueprint/src/graph/language-registry.mjs`'s `languagesJson()`),
//! ported to the native `membrane_blueprint::cli::languages` dispatch.

use membrane_blueprint::cli;

#[test]
fn languages_json_shape_matches_legacy_top_level_keys() {
    let payload = cli::languages();
    assert_eq!(payload["schemaVersion"], 1);
    assert!(payload["digest"].is_string());
    let languages = payload["languages"].as_array().expect("languages array");
    assert!(!languages.is_empty());
}

#[test]
fn every_language_record_has_the_legacy_capability_fields() {
    let payload = cli::languages();
    for entry in payload["languages"].as_array().unwrap() {
        assert!(entry.get("language").and_then(|v| v.as_str()).is_some());
        assert!(entry.get("extensions").and_then(|v| v.as_array()).is_some());
        assert!(entry.get("factProfile").and_then(|v| v.as_str()).is_some());
        assert!(entry.get("precisionTier").and_then(|v| v.as_str()).is_some());
        assert!(entry.get("limits").is_some());
    }
}

#[test]
fn digest_is_deterministic_across_calls() {
    let a = cli::languages();
    let b = cli::languages();
    assert_eq!(a["digest"], b["digest"]);
    assert_eq!(a, b);
}
