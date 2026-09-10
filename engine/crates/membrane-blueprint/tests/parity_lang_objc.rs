use membrane_blueprint::graph::{build_generation, GraphOptions};
use std::fs;
use tempfile::tempdir;

fn fixture() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let source = include_str!("fixtures/languages/objc/runner.m");
    fs::write(dir.path().join("runner.m"), source).unwrap();
    dir
}

#[test]
fn objc_declarations_are_extracted() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    assert!(!generation.nodes.is_empty(), "expected at least one AST node");
}

#[test]
fn objc_contains_edges_link_file_to_symbols() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    assert!(generation.edges.iter().any(|e| e.kind == "CONTAINS"));
}
