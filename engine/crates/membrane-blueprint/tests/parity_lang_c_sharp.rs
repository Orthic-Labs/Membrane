use membrane_blueprint::graph::{build_generation, GraphOptions};
use std::fs;
use tempfile::tempdir;

fn fixture() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let source = include_str!("fixtures/languages/c_sharp/Runner.cs");
    fs::write(dir.path().join("Runner.cs"), source).unwrap();
    dir
}

#[test]
fn c_sharp_class_and_methods_are_extracted() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let names: Vec<&str> = generation.nodes.iter().filter_map(|n| n.name.as_deref()).collect();
    assert!(names.contains(&"Runner"), "expected Runner class symbol, got {names:?}");
    assert!(names.contains(&"Run"), "expected Run() method symbol, got {names:?}");
    assert!(names.contains(&"Start"), "expected Start() method symbol, got {names:?}");
}

#[test]
fn c_sharp_contains_edges_link_file_to_symbols() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    assert!(generation.edges.iter().any(|e| e.kind == "CONTAINS"));
}
