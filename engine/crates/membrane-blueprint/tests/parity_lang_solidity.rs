use membrane_blueprint::graph::{build_generation, GraphOptions};
use std::fs;
use tempfile::tempdir;

fn fixture() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let source = include_str!("fixtures/languages/solidity/Runner.sol");
    fs::write(dir.path().join("Runner.sol"), source).unwrap();
    dir
}

#[test]
fn solidity_declarations_are_extracted() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let names: Vec<&str> = generation.nodes.iter().filter_map(|n| n.name.as_deref()).collect();
    assert!(names.contains(&"Runner"), "expected Runner contract, got {names:?}");
    assert!(names.contains(&"run"), "expected run function, got {names:?}");
}

#[test]
fn solidity_contains_edges_link_file_to_symbols() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    assert!(generation.edges.iter().any(|e| e.kind == "CONTAINS"));
}
