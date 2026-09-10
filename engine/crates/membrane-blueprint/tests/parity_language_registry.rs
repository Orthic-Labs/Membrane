//! Registry/dispatch coverage for every catalog language and extension.

use membrane_blueprint::{graph::{build_generation, GraphOptions}, lib_cli_languages};
use std::{collections::BTreeMap, fs};
use tempfile::tempdir;

const EXPECTED_LANGUAGES: &[&str] = &[
    "bash", "c", "c_sharp", "cpp", "css", "dart", "elisp", "elixir", "elm",
    "embedded_template", "go", "html", "java", "javascript", "json", "kotlin",
    "lua", "objc", "ocaml", "php", "python", "ql", "rescript", "ruby", "rust",
    "scala", "solidity", "swift", "systemrdl", "tlaplus", "toml", "tsx", "typescript",
    "vue", "yaml", "zig",
];

#[test]
fn catalog_records_and_extension_aliases_are_consistent() {
    let records = lib_cli_languages::language_capability_records();
    let mut by_language = BTreeMap::new();
    for record in &records {
        assert!(by_language.insert(record.language.as_str(), record).is_none(), "duplicate language {}", record.language);
        for extension in &record.extensions {
            let resolved = lib_cli_languages::language_by_extension(extension);
            assert_eq!(resolved["fallback"], false, "catalog extension must resolve: {extension}");
            assert_eq!(resolved["language"], record.language, "extension mapped to wrong language: {extension}");
        }
    }
    assert_eq!(by_language.keys().copied().collect::<Vec<_>>(), EXPECTED_LANGUAGES);
    assert_eq!(records.len(), EXPECTED_LANGUAGES.len());

    // These C++ header aliases are accepted by graph dispatch even though they
    // are intentionally not separate catalog records.
    let root = tempdir().unwrap();
    fs::write(root.path().join("probe.hh"), "class Probe {};").unwrap();
    fs::write(root.path().join("probe.hxx"), "class Probe {};").unwrap();
    let generation = build_generation(root.path(), &GraphOptions::default()).unwrap();
    for extension in ["hh", "hxx"] {
        let report = generation.files.iter().find(|file| file.path.ends_with(extension)).expect("alias report");
        assert_eq!(report.language.as_deref(), Some("cpp"));
        assert_eq!(report.parse_status, "ok", "C++ alias did not parse: {report:?}");
    }
}

#[test]
fn every_catalog_extension_reaches_native_parser_dispatch() {
    let root = tempdir().unwrap();
    let records = lib_cli_languages::language_capability_records();
    for record in &records {
        for extension in &record.extensions {
            fs::write(root.path().join(format!("probe_{extension}.{extension}")), "").unwrap();
        }
    }
    let generation = build_generation(root.path(), &GraphOptions::default()).unwrap();
    for record in &records {
        for extension in &record.extensions {
            let suffix = format!(".{extension}");
            let report = generation.files.iter().find(|file| file.path.ends_with(&suffix)).expect("catalog extension report");
            assert_eq!(report.language.as_deref(), Some(record.language.as_str()), "extension dispatch mismatch: {extension}");
            assert_ne!(report.parse_status, "unsupported", "catalog extension bypassed parser dispatch: {extension}");
            assert_ne!(report.parse_status, "failed", "catalog parser failed to initialize: {extension}: {report:?}");
        }
    }
}

#[test]
fn tsx_fixture_reaches_native_parser_with_clean_status() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("Runner.tsx"), include_str!("fixtures/languages/tsx/Runner.tsx")).unwrap();
    let generation = build_generation(root.path(), &GraphOptions::default()).unwrap();
    let report = generation.files.iter().find(|file| file.path.ends_with("Runner.tsx")).expect("TSX report");
    assert_eq!(report.language.as_deref(), Some("tsx"));
    assert_eq!(report.parse_status, "ok", "TSX fixture did not parse: {report:?}");
}
