use membrane_blueprint::graph::{build_generation, GraphOptions};
use std::fs;
use tempfile::tempdir;

fn fixture() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let source = include_str!("fixtures/languages/toml/runner.toml");
    fs::write(dir.path().join("runner.toml"), source).unwrap();
    dir
}

#[test]
fn toml_parses_without_error() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    assert!(generation.files.iter().any(|r| r.path.ends_with("runner.toml") && r.parse_status == "ok"));
}

#[test]
fn toml_contains_edges_link_file_to_symbols() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    assert!(generation.edges.iter().any(|e| e.kind == "CONTAINS"));
}
