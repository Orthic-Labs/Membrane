//! Parity tests for the native JS/TS module-surface extraction already
//! carried on the `file` node's `evidence[0].moduleSurface` fact (see
//! `graph::module_surface` in `engine/crates/membrane-blueprint/src/graph.rs`,
//! consumed by `findings.rs`). This is the equivalent native replacement for
//! the legacy `blueprint/src/graph/module-surface.mjs` tree-sitter extractor:
//! same soundness rule — a module's export surface is only "closed" (usable
//! for a missing-binding finding) when every export-shaped construct in it
//! was recognized, and any unrecognized/dynamic construct opens the surface
//! instead of silently under- or over-reporting.
//!
//! These tests build real generations from fixture repositories through the
//! public `build_generation` API (no direct access to the private
//! `module_surface` helper) and assert on the resulting fact shape.

use membrane_blueprint::graph::{build_generation, GraphOptions};
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

fn surface_for<'a>(nodes: &'a [membrane_blueprint::model::GraphNode], path: &str) -> &'a Value {
    nodes
        .iter()
        .find(|node| node.kind == "file" && node.path.as_deref() == Some(path))
        .and_then(|node| node.evidence.first())
        .and_then(|evidence| evidence.get("moduleSurface"))
        .unwrap_or_else(|| panic!("no moduleSurface fact for {path}"))
}

#[test]
fn named_and_default_exports_are_closed_and_enumerated() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("m.ts"), "export const a = 1;\nexport function b() {}\nexport default class C {}\n").unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let surface = surface_for(&generation.nodes, "m.ts");
    assert_eq!(surface["parseStatus"], "ok");
    assert!(surface["open"].as_array().unwrap().is_empty(), "surface should be closed: {surface}");
    let names: Vec<&str> = surface["exports"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"b"));
    assert!(names.contains(&"default"));
}

#[test]
fn re_export_clause_records_export_and_request() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("m.ts"), "export { x as y } from \"./other\";\n").unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let surface = surface_for(&generation.nodes, "m.ts");
    assert!(surface["open"].as_array().unwrap().is_empty());
    assert_eq!(surface["exports"][0]["name"], "y");
    let request = &surface["requests"][0];
    assert_eq!(request["kind"], "reexport");
    assert_eq!(request["name"], "x");
    assert_eq!(request["localName"], "y");
    assert_eq!(request["specifier"], "./other");
}

#[test]
fn commonjs_module_exports_opens_the_surface() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("m.js"), "module.exports = { a: 1 };\n").unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let surface = surface_for(&generation.nodes, "m.js");
    assert!(surface["open"].as_array().unwrap().iter().any(|o| o["reason"] == "commonjs_exports"));
}

#[test]
fn export_assignment_opens_the_surface() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("m.ts"), "export = foo;\n").unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let surface = surface_for(&generation.nodes, "m.ts");
    assert!(surface["open"].as_array().unwrap().iter().any(|o| o["reason"] == "export_assignment"));
}

#[test]
fn ambient_module_declaration_opens_the_surface() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("m.ts"), "declare module \"x\" { export const a: number; }\n").unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let surface = surface_for(&generation.nodes, "m.ts");
    assert!(surface["open"].as_array().unwrap().iter().any(|o| o["reason"] == "ambient_module"));
}

#[test]
fn destructured_export_opens_the_surface() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("m.ts"), "export const { a, b } = obj;\n").unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let surface = surface_for(&generation.nodes, "m.ts");
    assert!(surface["open"].as_array().unwrap().iter().any(|o| o["reason"] == "destructured_export"));
}

#[test]
fn unsupported_extension_reports_no_language_and_no_findings_material() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("m.py"), "def f():\n    return 1\n").unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let surface = surface_for(&generation.nodes, "m.py");
    assert_eq!(surface["parseStatus"], "unsupported");
    assert!(surface["exports"].as_array().unwrap().is_empty());
}

#[test]
fn parse_error_opens_the_surface_rather_than_reporting_partial_exports() {
    let dir = tempdir().unwrap();
    // Unbalanced brace: the grammar cannot fully parse this file, so the
    // surface must report failed/open rather than guessing at exports.
    fs::write(dir.path().join("m.ts"), "export function broken( {\n").unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let surface = surface_for(&generation.nodes, "m.ts");
    assert_eq!(surface["parseStatus"], "failed");
    assert!(surface["open"].as_array().unwrap().iter().any(|o| o["reason"] == "parse_error"));
}

#[test]
fn imports_are_recorded_as_outbound_requests() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("m.ts"), "import { a, b as c } from \"./other\";\nimport d from \"./other\";\n").unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    let surface = surface_for(&generation.nodes, "m.ts");
    let requests = surface["requests"].as_array().unwrap();
    assert!(requests.iter().any(|r| r["kind"] == "import" && r["name"] == "a" && r["specifier"] == "./other"));
    assert!(requests.iter().any(|r| r["kind"] == "import" && r["name"] == "b" && r["localName"] == "c"));
    assert!(requests.iter().any(|r| r["kind"] == "import" && r["name"] == "default" && r["localName"] == "d"));
}
