use membrane_blueprint::graph::{build_generation, GraphOptions};
use std::fs;
use tempfile::tempdir;

fn fixture() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let source = include_str!("fixtures/languages/yaml/runner.yaml");
    fs::write(dir.path().join("runner.yaml"), source).unwrap();
    dir
}

#[test]
fn yaml_parses_without_error() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    assert!(generation.files.iter().any(|r| r.path.ends_with("runner.yaml") && r.parse_status == "ok"));
}

#[test]
fn yaml_contains_edges_link_file_to_symbols() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    assert!(generation.edges.iter().any(|e| e.kind == "CONTAINS"));
}
