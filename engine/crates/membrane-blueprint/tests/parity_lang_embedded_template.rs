use membrane_blueprint::graph::{build_generation, GraphOptions};
use std::fs;
use tempfile::tempdir;

fn fixture() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let source = include_str!("fixtures/languages/embedded_template/runner.erb");
    fs::write(dir.path().join("runner.erb"), source).unwrap();
    dir
}

/// embedded_template (ERB) has no declaration-shaped node kinds (function,
/// class, etc.) in its own grammar — the legacy JS table only extracted
/// generic "element" markup for this profile, which ERB's grammar does not
/// emit either. Parity here means the native parser accepts the file with
/// tree-sitter and reports a clean (non-partial) parse, not that symbols are
/// fabricated where the grammar proves none.
#[test]
fn embedded_template_parses_without_errors() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let report = generation.files.iter().find(|f| f.path.ends_with("runner.erb")).expect("erb file report");
    assert_eq!(report.parse_status, "ok", "expected clean parse, got {report:?}");
    assert_eq!(report.language.as_deref(), Some("embedded_template"));
}
