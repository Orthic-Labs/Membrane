//! Parity tests for `entry_points::build_entry_point_registry`, ported from
//! the legacy `blueprint/src/graph/entry-points.mjs` behavior:
//!
//! - a node explicitly marked `entryPoint: true`, or carrying an
//!   `EntryPoint`/`entry_point` label (case-insensitively), is an "explicit"
//!   row regardless of its edges;
//! - absent an explicit marker, a `symbol` node with at least one outgoing
//!   edge and zero observed inbound edges is a "structural_candidate" row;
//! - structural candidates are suppressed entirely when
//!   `includeStructuralCandidates` is false;
//! - non-symbol nodes never become structural candidates even with the same
//!   zero-inbound/outgoing shape;
//! - rows are sorted by node id.

use membrane_blueprint::entry_points::{build_entry_point_registry, build_entry_point_registry_from};
use membrane_blueprint::store::Generation;
use serde_json::json;

fn generation() -> Generation {
    serde_json::from_value(json!({
        "manifest": {"generationId": "g1"},
        "nodes": [
            {"id": "symbol:src/main.ts::main", "kind": "symbol", "labels": ["Function"], "evidence": [{"line": 1}]},
            {"id": "symbol:src/lib.ts::helper", "kind": "symbol", "labels": ["Function"], "evidence": [{"line": 1}]},
            {"id": "symbol:src/cli.ts::run", "kind": "symbol", "entryPoint": true, "labels": ["Function"], "evidence": [{"line": 3}]},
            {"id": "symbol:src/tagged.ts::start", "kind": "symbol", "labels": ["Function", "EntryPoint"], "evidence": []},
            {"id": "file:src/main.ts", "kind": "file", "labels": ["File"], "evidence": [{"path": "src/main.ts"}]}
        ],
        "edges": [
            {"source": "symbol:src/main.ts::main", "target": "symbol:src/lib.ts::helper", "kind": "calls"}
        ],
        "fileReports": []
    }))
    .unwrap()
}

#[test]
fn explicit_marker_wins_regardless_of_edges() {
    let rows = build_entry_point_registry(&generation(), true);
    let run = rows.iter().find(|row| row["id"] == "symbol:src/cli.ts::run").expect("explicit entry point present");
    assert_eq!(run["authority"], "explicit");
    assert_eq!(run["reason"], "source_backed_entrypoint_marker");
}

#[test]
fn entrypoint_label_is_case_insensitive_and_explicit() {
    let rows = build_entry_point_registry(&generation(), true);
    let tagged = rows.iter().find(|row| row["id"] == "symbol:src/tagged.ts::start").expect("labeled entry point present");
    assert_eq!(tagged["authority"], "explicit");
}

#[test]
fn zero_inbound_outgoing_symbol_is_structural_candidate() {
    let rows = build_entry_point_registry(&generation(), true);
    let main = rows.iter().find(|row| row["id"] == "symbol:src/main.ts::main").expect("structural candidate present");
    assert_eq!(main["authority"], "structural_candidate");
    assert_eq!(main["reason"], "outgoing_with_zero_observed_inbound");
}

#[test]
fn node_with_inbound_edge_is_not_a_candidate() {
    let rows = build_entry_point_registry(&generation(), true);
    assert!(rows.iter().all(|row| row["id"] != "symbol:src/lib.ts::helper"));
}

#[test]
fn non_symbol_kind_never_becomes_a_structural_candidate() {
    // The file node has no incoming edge and (trivially) no outgoing edge
    // either, but even if it had one, only `kind: "symbol"` nodes qualify.
    let nodes = json!([
        {"id": "file:src/only.ts", "kind": "file", "labels": ["File"], "evidence": []}
    ]);
    let edges = json!([{"source": "file:src/only.ts", "target": "symbol:src/only.ts::x", "kind": "declares"}]);
    let rows = build_entry_point_registry_from(nodes.as_array().unwrap(), edges.as_array().unwrap(), true);
    assert!(rows.is_empty());
}

#[test]
fn structural_candidates_are_suppressed_when_disabled() {
    let rows = build_entry_point_registry(&generation(), false);
    assert!(rows.iter().all(|row| row["authority"] != "structural_candidate"));
    // Explicit rows still surface.
    assert!(rows.iter().any(|row| row["id"] == "symbol:src/cli.ts::run"));
    assert!(rows.iter().any(|row| row["id"] == "symbol:src/tagged.ts::start"));
}

#[test]
fn rows_are_sorted_by_id() {
    let rows = build_entry_point_registry(&generation(), true);
    let ids: Vec<&str> = rows.iter().map(|row| row["id"].as_str().unwrap()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn evidence_is_carried_through_verbatim() {
    let rows = build_entry_point_registry(&generation(), true);
    let run = rows.iter().find(|row| row["id"] == "symbol:src/cli.ts::run").unwrap();
    assert_eq!(run["evidence"], json!([{"line": 3}]));
}

#[test]
fn empty_generation_yields_empty_registry() {
    let empty: Generation = serde_json::from_value(json!({"nodes": [], "edges": []})).unwrap();
    assert!(build_entry_point_registry(&empty, true).is_empty());
}
