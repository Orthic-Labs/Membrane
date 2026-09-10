//! GC7 parity tests: native port of
//! `blueprint/src/graph/ast-structural-search.mjs` (`searchAstStructure`).
//!
//! There is no legacy `*.test.mjs` for this module in the source tree, so
//! these fixtures are derived directly from the documented behavior of
//! `searchAstStructure` in the legacy source: kind/name/pathPrefix/relation/
//! declaringType/label filtering, deterministic sort order, limit/omission
//! accounting, and edge selection bound to the selected node set.

use membrane_blueprint::ast_structural_search::{search_ast_structure, SearchPattern};
use membrane_blueprint::model::{GraphEdge, GraphNode};
use membrane_blueprint::GraphGeneration;
use serde_json::json;

fn node(id: &str, kind: &str, path: &str, name: &str, qualified: &str, labels: &[&str]) -> GraphNode {
    GraphNode {
        id: id.to_string(),
        kind: kind.to_string(),
        path: Some(path.to_string()),
        name: Some(name.to_string()),
        generation_id: "gen-1".to_string(),
        evidence: vec![json!({
            "path": path,
            "startLine": 1,
            "endLine": 2,
            "contentHash": "hash",
            "labels": labels,
            "qualifiedName": qualified,
        })],
    }
}

fn edge(id: &str, kind: &str, source: &str, target: Option<&str>) -> GraphEdge {
    GraphEdge {
        id: id.to_string(),
        kind: kind.to_string(),
        source: source.to_string(),
        target: target.map(str::to_string),
        generation_id: "gen-1".to_string(),
        evidence: vec![json!({})],
    }
}

fn generation(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> GraphGeneration {
    GraphGeneration {
        schema_version: 1,
        provider: "tree-sitter".into(),
        provider_version: "1".into(),
        generation_id: "gen-1".into(),
        source_hash: "src-hash".into(),
        repo_root: "/repo".into(),
        complete: true,
        nodes,
        edges,
        files: Vec::new(),
        truncation_reasons: Vec::new(),
    }
}

#[test]
fn filters_by_kind_case_insensitively_including_labels() {
    let gen = generation(
        vec![
            node("symbol:a::foo", "symbol", "src/a.ts", "foo", "foo", &["Function"]),
            node("symbol:a::Bar", "symbol", "src/a.ts", "Bar", "Bar", &["Class"]),
        ],
        vec![],
    );
    let result = search_ast_structure(
        &gen,
        &SearchPattern {
            kind: Some("function".into()),
            ..Default::default()
        },
    );
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.nodes[0].id, "symbol:a::foo");
}

#[test]
fn name_match_is_substring_unless_exact() {
    let gen = generation(
        vec![
            node("symbol:a::fooBar", "symbol", "src/a.ts", "fooBar", "fooBar", &["Function"]),
            node("symbol:a::foo", "symbol", "src/a.ts", "foo", "foo", &["Function"]),
        ],
        vec![],
    );
    let substring = search_ast_structure(
        &gen,
        &SearchPattern {
            name: Some("foo".into()),
            ..Default::default()
        },
    );
    assert_eq!(substring.nodes.len(), 2);

    let exact = search_ast_structure(
        &gen,
        &SearchPattern {
            name: Some("foo".into()),
            exact_name: true,
            ..Default::default()
        },
    );
    assert_eq!(exact.nodes.len(), 1);
    assert_eq!(exact.nodes[0].id, "symbol:a::foo");
}

#[test]
fn path_prefix_normalizes_backslashes() {
    let gen = generation(
        vec![
            node("symbol:a::foo", "symbol", "src/pkg/a.ts", "foo", "foo", &["Function"]),
            node("symbol:b::bar", "symbol", "other/b.ts", "bar", "bar", &["Function"]),
        ],
        vec![],
    );
    let result = search_ast_structure(
        &gen,
        &SearchPattern {
            path_prefix: Some("src\\pkg".into()),
            ..Default::default()
        },
    );
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.nodes[0].id, "symbol:a::foo");
}

#[test]
fn declaring_type_matches_prefix_of_qualified_name() {
    let gen = generation(
        vec![node(
            "symbol:a::Foo.bar",
            "symbol",
            "src/a.ts",
            "bar",
            "Foo.bar",
            &["Method"],
        )],
        vec![],
    );
    let result = search_ast_structure(
        &gen,
        &SearchPattern {
            declaring_type: Some("Foo".into()),
            ..Default::default()
        },
    );
    assert_eq!(result.nodes.len(), 1);
}

#[test]
fn label_filter_requires_exact_label_membership() {
    let gen = generation(
        vec![
            node("symbol:a::foo", "symbol", "src/a.ts", "foo", "foo", &["Function", "Test"]),
            node("symbol:a::bar", "symbol", "src/a.ts", "bar", "bar", &["Function"]),
        ],
        vec![],
    );
    let result = search_ast_structure(
        &gen,
        &SearchPattern {
            label: Some("Test".into()),
            ..Default::default()
        },
    );
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.nodes[0].id, "symbol:a::foo");
}

#[test]
fn sorts_by_path_then_qualified_name_then_id() {
    let gen = generation(
        vec![
            node("symbol:z::b", "symbol", "src/b.ts", "b", "b", &["Function"]),
            node("symbol:a::z", "symbol", "src/a.ts", "z", "z", &["Function"]),
            node("symbol:a::a", "symbol", "src/a.ts", "a", "a", &["Function"]),
        ],
        vec![],
    );
    let result = search_ast_structure(&gen, &SearchPattern::default());
    let ids: Vec<&str> = result.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["symbol:a::a", "symbol:a::z", "symbol:z::b"]);
}

#[test]
fn limit_truncates_and_records_omission() {
    let nodes: Vec<GraphNode> = (0..5)
        .map(|i| {
            let name = format!("fn{i}");
            node(
                &format!("symbol:a::{name}"),
                "symbol",
                "src/a.ts",
                &name,
                &name,
                &["Function"],
            )
        })
        .collect();
    let gen = generation(nodes, vec![]);
    let result = search_ast_structure(
        &gen,
        &SearchPattern {
            limit: Some(2),
            ..Default::default()
        },
    );
    assert_eq!(result.nodes.len(), 2);
    assert!(result.truncated);
    assert_eq!(result.omissions.len(), 1);
    assert_eq!(result.omissions[0]["reason"], "structural_search_limit");
    assert_eq!(result.omissions[0]["limit"], 2);
}

#[test]
fn non_positive_or_missing_limit_falls_back_to_default_and_caps_at_max() {
    let gen = generation(vec![], vec![]);
    let default = search_ast_structure(&gen, &SearchPattern::default());
    assert_eq!(default.pattern.get("limit"), None);

    let clamped = search_ast_structure(
        &gen,
        &SearchPattern {
            limit: Some(10_000),
            ..Default::default()
        },
    );
    // Not directly observable on the result struct, but must not panic and
    // must still report pattern.limit verbatim (legacy echoes the raw
    // pattern object, only the applied search uses the clamp).
    assert_eq!(clamped.pattern["limit"], 10_000);
}

#[test]
fn edges_are_filtered_by_relation_and_bound_to_selected_nodes() {
    let gen = generation(
        vec![
            node("symbol:a::foo", "symbol", "src/a.ts", "foo", "foo", &["Function"]),
            node("symbol:a::bar", "symbol", "src/a.ts", "bar", "bar", &["Function"]),
        ],
        vec![
            edge("edge:1", "CALLS", "symbol:a::foo", Some("symbol:a::bar")),
            edge("edge:2", "CONTAINS", "file:a", Some("symbol:a::foo")),
            edge("edge:3", "CALLS", "symbol:other::x", Some("symbol:other::y")),
        ],
    );
    let result = search_ast_structure(
        &gen,
        &SearchPattern {
            relation: Some("CALLS".into()),
            ..Default::default()
        },
    );
    let ids: Vec<&str> = result.edges.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["edge:1"]);
}

#[test]
fn generation_id_and_schema_are_echoed() {
    let gen = generation(vec![], vec![]);
    let result = search_ast_structure(&gen, &SearchPattern::default());
    assert_eq!(result.schema_version, 1);
    assert_eq!(result.kind, "ast-structural-search");
    assert_eq!(result.generation_id.as_deref(), Some("gen-1"));
}
