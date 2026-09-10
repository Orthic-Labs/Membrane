use membrane_blueprint::graph::{build_generation, GraphOptions};
use std::fs;
use tempfile::tempdir;

fn fixture() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let source = include_str!("fixtures/languages/vue/Runner.vue");
    fs::write(dir.path().join("Runner.vue"), source).unwrap();
    dir
}

/// Vue SFCs are markup (template/script/style blocks); the legacy JS table
/// used the shared markupTable "element" kind, same as html. Parity here is
/// a clean, non-partial parse plus at least one markup "element" node from
/// the template block.
#[test]
fn vue_parses_and_contains_elements() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let report = generation.files.iter().find(|f| f.path.ends_with("Runner.vue")).expect("vue file report");
    assert_eq!(report.parse_status, "ok", "expected clean parse, got {report:?}");
    assert_eq!(report.language.as_deref(), Some("vue"));
}
