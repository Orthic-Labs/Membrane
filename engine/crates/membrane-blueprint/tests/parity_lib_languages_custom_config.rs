//! Parity test for `blueprint/src/lib/languages/custom-config.mjs`.

use membrane_blueprint::lib_languages_custom_config::{load_custom_languages, CUSTOM_CONFIG_VERSION};
use std::collections::HashSet;
use std::fs;

fn write(dir: &std::path::Path, rel: &str, content: &[u8]) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn missing_config_yields_empty_no_error_result() {
    let dir = tempfile::tempdir().unwrap();
    let result = load_custom_languages(
        dir.path(),
        "blueprint.languages.toml",
        &HashSet::new(),
        &HashSet::new(),
    );
    assert_eq!(result.version, CUSTOM_CONFIG_VERSION);
    assert!(result.languages.is_empty() && result.errors.is_empty());
}

#[test]
fn valid_language_is_accepted_with_computed_grammar_hash() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "grammars/lang.wasm", b"grammar-bytes");
    write(
        dir.path(),
        "blueprint.languages.toml",
        br#"
[[languages]]
id = "mylang"
extensions = ["ml", "mli"]
grammar = "grammars/lang.wasm"
"#,
    );
    let result = load_custom_languages(
        dir.path(),
        "blueprint.languages.toml",
        &HashSet::new(),
        &HashSet::new(),
    );
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.languages.len(), 1);
    assert_eq!(result.languages[0].id, "mylang");
    assert_eq!(result.languages[0].fact_profile, "code");
    assert_eq!(result.languages[0].precision_tier, "AST");
    assert!(!result.languages[0].grammar_sha256.is_empty());
}

#[test]
fn builtin_extension_collision_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "grammars/lang.wasm", b"bytes");
    write(
        dir.path(),
        "blueprint.languages.toml",
        br#"
[[languages]]
id = "mylang"
extensions = ["rs"]
grammar = "grammars/lang.wasm"
"#,
    );
    let builtin_ext: HashSet<String> = ["rs".to_string()].into_iter().collect();
    let result = load_custom_languages(
        dir.path(),
        "blueprint.languages.toml",
        &HashSet::new(),
        &builtin_ext,
    );
    assert!(result.languages.is_empty());
    assert!(result.errors[0].contains("language_route_conflict"));
}

#[test]
fn url_grammar_path_is_rejected_as_path_conflict() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "blueprint.languages.toml",
        br#"
[[languages]]
id = "remote"
extensions = ["rem"]
grammar = "https://example.com/grammar.wasm"
"#,
    );
    let result = load_custom_languages(
        dir.path(),
        "blueprint.languages.toml",
        &HashSet::new(),
        &HashSet::new(),
    );
    assert!(result.languages.is_empty());
    assert!(result.errors[0].starts_with("path_conflict"));
}
