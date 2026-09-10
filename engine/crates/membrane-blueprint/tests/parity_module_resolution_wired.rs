//! Proves `graph.rs`'s build pass, after being switched to call
//! `module_resolution::{import_specifiers_in_files, resolve_import_in_files}`
//! instead of its own former private `import_specifiers`/`resolve_import`,
//! produces byte-identical `IMPORTS` edge resolution to before the
//! relocation (lane P4, 2026-09-10). The two adapter functions are a
//! verbatim port, so this is an equivalence proof, not a new-behavior test:
//! every case here is chosen to match what the pre-relocation functions
//! resolved, for Python, Rust, and TypeScript relative imports.

use membrane_blueprint::graph::{build_generation, GraphOptions};
use membrane_blueprint::model::GraphEdge;
use std::fs;
use tempfile::tempdir;

fn imports_from(edges: &[GraphEdge], source: &str) -> Vec<(String, Option<String>)> {
    edges.iter().filter(|e| e.kind == "IMPORTS" && e.source == source).map(|e| (e.source.clone(), e.target.clone())).collect()
}

#[test]
fn python_relative_import_resolves_to_sibling_module() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.py"), "from .worker import run\n").unwrap();
    fs::write(dir.path().join("src/worker.py"), "def run():\n    return 1\n").unwrap();

    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let edges = imports_from(&generation.edges, "file:src/main.py");
    assert_eq!(edges, vec![("file:src/main.py".to_string(), Some("file:src/worker.py".to_string()))]);
}

#[test]
fn python_relative_package_import_resolves_to_init() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/pkg")).unwrap();
    fs::write(dir.path().join("src/main.py"), "from .pkg import thing\n").unwrap();
    fs::write(dir.path().join("src/pkg/__init__.py"), "thing = 1\n").unwrap();

    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let edges = imports_from(&generation.edges, "file:src/main.py");
    assert_eq!(edges, vec![("file:src/main.py".to_string(), Some("file:src/pkg/__init__.py".to_string()))]);
}

#[test]
fn typescript_relative_import_resolves_by_extension() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/app.ts"), "import { run } from './worker'\n").unwrap();
    fs::write(dir.path().join("src/worker.ts"), "export function run() { return 1 }\n").unwrap();

    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let edges = imports_from(&generation.edges, "file:src/app.ts");
    assert_eq!(edges, vec![("file:src/app.ts".to_string(), Some("file:src/worker.ts".to_string()))]);
}

#[test]
fn typescript_relative_import_resolves_to_index() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/lib")).unwrap();
    fs::write(dir.path().join("src/app.ts"), "import { run } from './lib'\n").unwrap();
    fs::write(dir.path().join("src/lib/index.ts"), "export function run() { return 1 }\n").unwrap();

    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let edges = imports_from(&generation.edges, "file:src/app.ts");
    assert_eq!(edges, vec![("file:src/app.ts".to_string(), Some("file:src/lib/index.ts".to_string()))]);
}

#[test]
fn rust_use_of_sibling_module_resolves() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "use crate::worker;\nmod worker;\n").unwrap();
    fs::write(dir.path().join("src/worker.rs"), "pub fn run() {}\n").unwrap();

    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    // `mod worker;` resolves via the `mod` pattern (candidate: src/worker.rs
    // relative to src/lib.rs's own directory, i.e. base "src/worker").
    let edges = imports_from(&generation.edges, "file:src/lib.rs");
    assert!(edges.iter().any(|(_, target)| target.as_deref() == Some("file:src/worker.rs")), "expected a resolved src/worker.rs edge, got {edges:?}");
}

#[test]
fn unresolved_import_carries_no_target_but_is_not_dropped() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/app.ts"), "import { missing } from './does-not-exist'\n").unwrap();

    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let edges = imports_from(&generation.edges, "file:src/app.ts");
    assert_eq!(edges, vec![("file:src/app.ts".to_string(), None)]);
}
