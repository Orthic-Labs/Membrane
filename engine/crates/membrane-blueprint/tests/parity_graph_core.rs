// Parity port for the "blueprint/src/graph (core)" NCL-02 cluster.
//
// Legacy source: blueprint/src/graph/ignored-prefixes.mjs (canonical ignore-dir/file
// policy shared by graph discovery and native watch snapshots) and the rust-language
// branch of blueprint/src/graph/language-extractors.mjs + language-registry.mjs,
// exercised historically by blueprint/tests/python-rust-graph.test.mjs (struct/impl
// extraction for a Rust fixture pair).
//
// Native counterpart: engine/crates/membrane-blueprint/src/graph.rs
// (`is_canonical_ignored_dir`, `is_canonical_ignored_file`, and the tree-sitter
// `rust` branch of `ast_facts`/`walk_ast`, reached only through `scan_repository`
// and `build_generation` since those helpers are private). Neither had a dedicated
// native test prior to this file; `tests/graph.rs` covers python/js/ts fixtures and
// the registered-relationship-kind/generation-identity surface but not canonical
// ignore-list exclusion or Rust struct/impl extraction.
//
// This file proves only the native behavior that exists today. It does not assert
// anything about `normalizeIgnoredPrefixes`/`pathMatchesIgnoredPrefix` (the legacy
// *configurable* prefix-list API): there is no native equivalent of that dynamic,
// caller-supplied ignore-list surface, only the fixed canonical list below.

use membrane_blueprint::graph::{build_generation, scan_repository, GraphOptions, ScanOptions};
use std::fs;
use tempfile::tempdir;

#[test]
fn canonical_ignored_dirs_and_files_are_excluded_from_scan() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    fs::write(dir.path().join("node_modules/pkg/index.js"), "module.exports = 1;\n").unwrap();
    fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    fs::write(dir.path().join("target/debug/build.rs"), "fn build() {}\n").unwrap();
    fs::create_dir_all(dir.path().join("docs")).unwrap();
    fs::write(dir.path().join("docs/product.md"), "# generated\n").unwrap();
    fs::write(dir.path().join("Thumbs.db"), "binary").unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn kept() {}\n").unwrap();

    let report = scan_repository(dir.path(), &ScanOptions::default()).unwrap();
    let paths: Vec<_> = report.files.iter().map(|file| file.path.as_str()).collect();

    assert!(paths.contains(&"src/lib.rs"));
    assert!(!paths.iter().any(|p| p.starts_with("node_modules/")));
    assert!(!paths.iter().any(|p| p.starts_with("target/")));
    assert!(!paths.contains(&"docs/product.md"));
    assert!(!paths.contains(&"Thumbs.db"));
}

#[test]
fn agent_dot_prefixed_directories_are_canonically_ignored() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-workspace")).unwrap();
    fs::write(dir.path().join(".agent-workspace/scratch.py"), "x = 1\n").unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/keep.py"), "y = 1\n").unwrap();

    let report = scan_repository(dir.path(), &ScanOptions::default()).unwrap();
    let paths: Vec<_> = report.files.iter().map(|file| file.path.as_str()).collect();
    assert!(paths.contains(&"src/keep.py"));
    assert!(!paths.iter().any(|p| p.starts_with(".agent-workspace/")));
}

#[test]
fn rust_struct_and_impl_method_are_extracted() {
    // Mirrors the fixture pair from blueprint/tests/python-rust-graph.test.mjs
    // (rust/lib.rs calling into rust/store.rs's OrderStore::save).
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("rust")).unwrap();
    fs::write(
        dir.path().join("rust/lib.rs"),
        "mod store;\nuse crate::store::OrderStore;\n\npub fn route_order() {\n    OrderStore::save();\n}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("rust/store.rs"),
        "pub struct OrderStore;\n\nimpl OrderStore {\n    pub fn save() {\n        persist();\n    }\n}\n",
    )
    .unwrap();

    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();

    let struct_node = generation
        .nodes
        .iter()
        .find(|node| node.path.as_deref() == Some("rust/store.rs") && node.name.as_deref() == Some("OrderStore"))
        .expect("OrderStore struct node must be extracted");
    let labels = struct_node.evidence[0]["labels"].as_array().unwrap();
    assert!(labels.iter().any(|label| label == "Class"), "struct_item must be labeled Class, got {labels:?}");

    // Both the regex-based lexical pass (which tracks `impl` blocks) and the
    // tree-sitter AST pass produce a "save" node here under different
    // qualified names -- select the impl-scoped one (`OrderStore.save`) by its
    // qualifiedName evidence field, since that is the one the legacy
    // impl_stack tracking in blueprint/src/graph/language-extractors.mjs
    // asserted as a method of OrderStore, not a bare top-level function.
    let save_method = generation
        .nodes
        .iter()
        .find(|node| node.path.as_deref() == Some("rust/store.rs") && node.evidence[0]["qualifiedName"] == "OrderStore.save")
        .expect("OrderStore::save method node must be extracted");
    let save_labels = save_method.evidence[0]["labels"].as_array().unwrap();
    assert!(save_labels.iter().any(|label| label == "Method"), "impl method must be labeled Method, got {save_labels:?}");

    let route_fn = generation
        .nodes
        .iter()
        .find(|node| node.path.as_deref() == Some("rust/lib.rs") && node.name.as_deref() == Some("route_order"))
        .expect("route_order function node must be extracted");
    let route_labels = route_fn.evidence[0]["labels"].as_array().unwrap();
    assert!(route_labels.iter().any(|label| label == "Function"), "top-level fn must be labeled Function, got {route_labels:?}");
}
