use membrane_blueprint::graph::{build_generation, GraphOptions};
use std::fs;
use tempfile::tempdir;

fn fixture() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let source = include_str!("fixtures/languages/rescript/Runner.res");
    fs::write(dir.path().join("Runner.res"), source).unwrap();
    dir
}

#[test]
fn rescript_declarations_are_extracted() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let names: Vec<&str> = generation.nodes.iter().filter_map(|n| n.name.as_deref()).collect();
    assert!(names.contains(&"config"), "expected config type, got {names:?}");
    assert!(names.contains(&"Inner"), "expected Inner module, got {names:?}");
}

#[test]
fn rescript_contains_edges_link_file_to_symbols() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    assert!(generation.edges.iter().any(|e| e.kind == "CONTAINS"));
}
