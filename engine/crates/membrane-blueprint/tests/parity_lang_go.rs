use membrane_blueprint::graph::{build_generation, GraphOptions};
use std::fs;
use tempfile::tempdir;

fn fixture() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let source = include_str!("fixtures/languages/go/main.go");
    fs::write(dir.path().join("main.go"), source).unwrap();
    dir
}

#[test]
fn go_functions_and_types_are_extracted() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let names: Vec<&str> = generation
        .nodes
        .iter()
        .filter_map(|n| n.name.as_deref())
        .collect();
    assert!(names.contains(&"run"), "expected run() symbol, got {names:?}");
    assert!(names.contains(&"start"), "expected start() symbol, got {names:?}");
    assert!(names.contains(&"Runner"), "expected Runner type symbol, got {names:?}");
}

#[test]
fn go_contains_edges_link_file_to_symbols() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    assert!(
        generation.edges.iter().any(|e| e.kind == "CONTAINS"),
        "expected CONTAINS edges from file to go symbols"
    );
}
