use membrane_blueprint::graph::{build_generation, confidence_from_resolution_path, generation_identity_bodies, is_registered_relationship_kind, scan_repository, ConfidenceTier, GraphOptions, ScanOptions};
use membrane_blueprint::model::{GraphEdge, GraphNode};
use serde_json::json;
use membrane_blueprint::index::{tokenize_identifier, LexicalIndex};
use std::fs;
use tempfile::tempdir;

#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;

fn fixture() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.py"), "from .worker import run\n\nclass Runner:\n    def go(self):\n        return run()\n").unwrap();
    fs::write(dir.path().join("src/worker.py"), "def run():\n    return 1\n").unwrap();
    fs::write(dir.path().join("src/app.ts"), "import { run } from './worker'\nexport function start() { return run() }\n").unwrap();
    fs::write(dir.path().join("ignored.txt"), "not a code symbol").unwrap();
    dir
}

#[test]
fn scan_is_confined_sorted_and_typed() {
    let dir = fixture();
    let report = scan_repository(dir.path(), &ScanOptions::default()).unwrap();
    let paths: Vec<_> = report.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, vec!["ignored.txt", "src/app.ts", "src/main.py", "src/worker.py"]);
    let canonical_root = fs::canonicalize(dir.path()).unwrap();
    assert!(report.files.iter().all(|file| file.absolute_path.starts_with(&canonical_root)));
}

#[cfg(windows)]
#[test]
fn sharing_violation_is_reported_as_partial_then_recovers() {
    let dir = tempdir().unwrap();
    let blocked = dir.path().join("blocked.rs");
    fs::write(&blocked, "fn blocked() {}\n").unwrap();
    let handle = fs::OpenOptions::new().read(true).share_mode(0).open(&blocked).unwrap();

    let partial = scan_repository(dir.path(), &ScanOptions::default()).unwrap();
    assert!(partial.traversal_truncated);
    assert!(partial.truncation_reasons.iter().any(|reason| reason == "file_read_error"));
    assert!(partial.skipped.iter().any(|entry| entry.path == "blocked.rs" && entry.state == "unavailable" && entry.reason.starts_with("file_read_error:")));
    drop(handle);

    let recovered = scan_repository(dir.path(), &ScanOptions::default()).unwrap();
    assert!(!recovered.traversal_truncated);
    assert!(recovered.truncation_reasons.is_empty());
    assert!(recovered.files.iter().any(|file| file.path == "blocked.rs"));
}

#[test]
fn native_generation_is_stable_and_has_registered_edges() {
    let dir = fixture();
    let first = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let second = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    assert_eq!(first.generation_id, second.generation_id);
    assert_eq!(first.nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>(), second.nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>());
    assert!(first.edges.iter().all(|edge| is_registered_relationship_kind(&edge.kind)));
    assert!(first.files.iter().any(|file| file.language.as_deref() == Some("python")));
}

#[test]
fn compiler_confidence_ladder_is_explicit() {
    assert_eq!(confidence_from_resolution_path("compiler"), ConfidenceTier::ExactResolution);
    assert_eq!(confidence_from_resolution_path("same-file"), ConfidenceTier::SameFileLexical);
    assert_eq!(confidence_from_resolution_path("unknown"), ConfidenceTier::Unresolved);
}

#[test]
fn identifier_index_preserves_raw_spelling_before_normalization() {
    let tokens = tokenize_identifier("HTTPServer XMLHttpRequest");
    assert_eq!(tokens.iter().map(|token| token.raw.as_str()).collect::<Vec<_>>(), vec!["HTTP", "Server", "XML", "Http", "Request"]);
    assert_eq!(tokens.iter().map(|token| token.normalized.as_str()).collect::<Vec<_>>(), vec!["http", "server", "xml", "http", "request"]);
}

#[test]
fn index_is_generation_bound() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let index = LexicalIndex::build(&generation);
    assert_eq!(index.generation_id, generation.generation_id);
    assert!(!index.search("Runner").is_empty());
}

#[test]
fn internal_symlinks_are_admitted_but_escape_and_dangling_links_are_typed_skips() {
    let dir = fixture();
    let internal = dir.path().join("src/worker-link.py");
    let outside_dir = tempdir().unwrap();
    let outside = outside_dir.path().join("outside.py");
    fs::write(&outside, "def outside():\n    return 1\n").unwrap();
    if link_file(&dir.path().join("src/worker.py"), &internal).is_err()
        || link_file(&outside, &dir.path().join("escape.py")).is_err()
        || link_file(&dir.path().join("src/missing.py"), &dir.path().join("dangling.py")).is_err() {
        return;
    }
    let report = scan_repository(dir.path(), &ScanOptions::default()).unwrap();
    assert!(report.files.iter().any(|file| file.path == "src/worker-link.py"));
    assert!(!report.files.iter().any(|file| file.path == "escape.py" || file.path == "dangling.py"));
    assert!(report.skipped.iter().any(|item| item.path == "escape.py" && item.reason == "symlink_escape_root"));
    assert!(report.skipped.iter().any(|item| item.path == "dangling.py" && item.reason.starts_with("dangling_symlink:")));
}

#[test]
fn generation_body_serialization_is_ordered_and_excludes_storage_generation_id() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let (nodes, edges) = generation_identity_bodies(&generation.nodes, &generation.edges);
    assert!(nodes.starts_with("[{\"id\":"));
    assert!(nodes.find("\"kind\"").unwrap() > nodes.find("\"id\"").unwrap());
    assert!(edges.find("\"source\"").unwrap() > edges.find("\"kind\"").unwrap());
    assert!(!nodes.contains("generation_id"));
    assert!(!edges.contains("generation_id"));
}

#[test]
fn generation_body_matches_donor_field_order_golden() {
    let nodes = vec![GraphNode {
        id: "symbol:a.ts::run".into(), kind: "symbol".into(), path: Some("a.ts".into()), name: Some("run".into()), generation_id: "storage-only".into(),
        evidence: vec![json!({"labels":["Function"],"qualifiedName":"run","confidence":0.75,"provider":"lexical"})],
    }];
    let edges = vec![GraphEdge {
        id: "edge:CALLS:symbol:a.ts::run->symbol:b.ts::run".into(), kind: "CALLS".into(), source: "symbol:a.ts::run".into(), target: Some("symbol:b.ts::run".into()), generation_id: "storage-only".into(),
        evidence: vec![json!({"confidence":0.75,"confidenceTier":"SAME_FILE_LEXICAL","resolved":true,"specifier":null,"provider":"lexical"})],
    }];
    let (node_body, edge_body) = generation_identity_bodies(&nodes, &edges);
    assert_eq!(node_body, r#"[{"id":"symbol:a.ts::run","kind":"symbol","labels":["Function"],"name":"run","qualifiedName":"run","path":"a.ts","confidence":0.75,"evidence":[{"confidence":0.75,"labels":["Function"],"provider":"lexical","qualifiedName":"run"}],"factProvider":{"id":"lexical","version":"repo-local-deterministic-v4"}}]"#);
    assert_eq!(edge_body, r#"[{"id":"edge:CALLS:symbol:a.ts::run->symbol:b.ts::run","kind":"CALLS","source":"symbol:a.ts::run","target":"symbol:b.ts::run","confidence":0.75,"confidenceTier":"SAME_FILE_LEXICAL","resolved":true,"specifier":null,"evidence":[{"confidence":0.75,"confidenceTier":"SAME_FILE_LEXICAL","provider":"lexical","resolved":true,"specifier":null}],"factProvider":{"id":"lexical","version":"repo-local-deterministic-v4"}}]"#);
}

#[test]
fn module_surface_is_lossless_and_generation_bound() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("source.ts"), "export const value = 1;\nexport default function run() {}\n").unwrap();
    fs::write(dir.path().join("consumer.ts"), "import run, { value as renamed } from './source';\nimport * as ns from './source';\nexport { value as forwarded } from './source';\nexport * from './source';\n").unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let source = generation.nodes.iter().find(|node| node.id == "file:source.ts").unwrap();
    let surface = &source.evidence[0]["moduleSurface"];
    assert_eq!(surface["parseStatus"], "ok");
    assert!(surface["exports"].as_array().unwrap().iter().any(|item| item["name"] == "value"));
    assert!(surface["exports"].as_array().unwrap().iter().any(|item| item["name"] == "default"));
    let consumer = generation.nodes.iter().find(|node| node.id == "file:consumer.ts").unwrap();
    let requests = consumer.evidence[0]["moduleSurface"]["requests"].as_array().unwrap();
    assert!(requests.iter().any(|item| item["kind"] == "namespace" && item["localName"] == "ns"));
    assert!(requests.iter().any(|item| item["kind"] == "reexport" && item["name"] == "value"));
    assert!(consumer.evidence[0]["moduleSurface"]["starReexports"].as_array().unwrap().iter().any(|item| item["specifier"] == "./source"));
    assert_eq!(source.generation_id, generation.generation_id);
}

#[test]
fn module_surface_extracts_multiline_esm_without_closed_surface_loss() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("source.ts"), "export const value = 1;\nexport default function run() {}\n").unwrap();
    fs::write(dir.path().join("consumer.ts"), "import run, {\n  value as renamed,\n} from './source';\nexport {\n  value as forwarded,\n} from './source';\nexport *\n  from './source';\n").unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let consumer = generation.nodes.iter().find(|node| node.id == "file:consumer.ts").unwrap();
    let surface = &consumer.evidence[0]["moduleSurface"];
    assert_eq!(surface["parseStatus"], "ok");
    let requests = surface["requests"].as_array().unwrap();
    assert!(requests.iter().any(|item| item["name"] == "value" && item["localName"] == "renamed"));
    assert!(requests.iter().any(|item| item["kind"] == "reexport" && item["name"] == "value"));
    assert!(surface["starReexports"].as_array().unwrap().iter().any(|item| item["specifier"] == "./source"));
    assert!(surface["open"].as_array().unwrap().is_empty());
}

#[test]
fn module_surface_marks_unsupported_parse_and_commonjs_open() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("legacy.cjs"), "module.exports = function legacy() {};\n").unwrap();
    fs::write(dir.path().join("notes.txt"), "export const notCode = true;\n").unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let legacy = generation.nodes.iter().find(|node| node.id == "file:legacy.cjs").unwrap();
    assert_eq!(legacy.evidence[0]["moduleSurface"]["parseStatus"], "ok");
    assert!(legacy.evidence[0]["moduleSurface"]["open"].as_array().unwrap().iter().any(|item| item["reason"] == "commonjs_exports"));
    let notes = generation.nodes.iter().find(|node| node.id == "file:notes.txt").unwrap();
    assert_eq!(notes.evidence[0]["moduleSurface"]["parseStatus"], "unsupported");
}

#[test]
fn incomplete_or_stale_index_requires_authoritative_fallback() {
    let dir = fixture();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let mut index = LexicalIndex::build(&generation);
    index.complete = false;
    let incomplete = index.candidates_for_literal("Runner", Some(&generation.generation_id), Some(&generation.source_hash));
    assert!(incomplete.fallback_required);
    assert_eq!(incomplete.reason.as_deref(), Some("index_incomplete"));
    let stale = LexicalIndex::build(&generation).candidates_for_literal("Runner", Some("wrong-generation"), Some(&generation.source_hash));
    assert!(stale.fallback_required);
    assert!(index.authoritative_literal_candidates("Runner", &generation).iter().any(|id| id.contains("Runner")));
}

#[cfg(unix)]
fn link_file(source: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> { std::os::unix::fs::symlink(source, link) }
#[cfg(windows)]
fn link_file(source: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> { std::os::windows::fs::symlink_file(source, link) }
