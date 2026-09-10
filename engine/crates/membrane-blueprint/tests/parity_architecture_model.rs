// Parity tests for src/architecture_model.rs against the rules in the
// legacy blueprint/src/graph/architecture-model.mjs, using fixtures ported
// from blueprint/tests/graph-analytics.test.mjs ("layers are deterministic
// with stable tie-breaking") and hand-derived component/flow cases matching
// the legacy module's own path-grouping and citation-cap behavior.

use membrane_blueprint::architecture_model::{
    assign_layers, build_architecture_model, build_disposable_architecture_projection, ArchEdge, ArchNode,
    EvidenceRef,
};

fn node(id: &str) -> ArchNode {
    ArchNode { id: id.to_string(), path: None, evidence: vec![] }
}

fn node_with_path(id: &str, path: &str) -> ArchNode {
    ArchNode {
        id: id.to_string(),
        path: Some(path.to_string()),
        evidence: vec![EvidenceRef {
            path: Some(path.to_string()),
            start_line: Some(1),
            end_line: Some(1),
            content_hash: Some(format!("h:{path}")),
        }],
    }
}

fn edge(id: &str, source: &str, target: &str) -> ArchEdge {
    ArchEdge { id: id.to_string(), source: source.to_string(), target: target.to_string(), kind: None, evidence: vec![] }
}

#[test]
fn layers_are_deterministic_with_stable_tie_breaking() {
    // Ported 1:1 from graph-analytics.test.mjs "layers are deterministic
    // with stable tie-breaking".
    let nodes = vec![node("a"), node("b"), node("c")];
    let edges = vec![edge("e1", "a", "b"), edge("e2", "b", "c")];
    let layers = assign_layers(&nodes, &edges);
    let by_id: std::collections::HashMap<_, _> = layers.iter().map(|l| (l.id.clone(), l.layer)).collect();
    assert_eq!(by_id["a"], 0);
    assert_eq!(by_id["b"], 1);
    assert_eq!(by_id["c"], 2);
}

#[test]
fn architecture_model_groups_layers_coupling_and_hotspots() {
    // a -> b, a -> c, b -> c: a has out-degree 2 (highest coupling/hotspot).
    let nodes = vec![node("a"), node("b"), node("c")];
    let edges = vec![edge("e1", "a", "b"), edge("e2", "a", "c"), edge("e3", "b", "c")];
    let model = build_architecture_model(&nodes, &edges, Some("gen-1".to_string()));

    assert_eq!(model.schema_version, 1);
    assert_eq!(model.generation_id.as_deref(), Some("gen-1"));
    assert_eq!(model.algorithm.name, "layer-clustering");
    assert_eq!(model.node_count, 3);
    assert_eq!(model.edge_count, 3);

    // layer 0 = {a}, layer 1 = {b}, layer 2 = {c}
    let layer0 = model.layers.iter().find(|l| l.layer == 0).unwrap();
    assert_eq!(layer0.node_ids, vec!["a".to_string()]);
    let layer2 = model.layers.iter().find(|l| l.layer == 2).unwrap();
    assert_eq!(layer2.node_ids, vec!["c".to_string()]);

    // coupling sorted by degree desc: a=2, b=1
    assert_eq!(model.coupling[0].id, "a");
    assert_eq!(model.coupling[0].degree, 2);

    // hotspots: top by degree desc then id asc, capped at 20, all candidate=true
    assert_eq!(model.hotspots[0].id, "a");
    assert!(model.hotspots.iter().all(|h| h.candidate));
}

#[test]
fn disposable_projection_groups_components_by_first_two_path_segments() {
    // Mirrors componentKey(): path "src/api/handler.ts" -> component "src/api".
    let nodes = vec![
        node_with_path("symbol:a", "src/api/handler.ts"),
        node_with_path("symbol:b", "src/api/other.ts"),
        node_with_path("symbol:c", "src/ui/screen.tsx"),
    ];
    let edges = vec![
        edge("e1", "symbol:a", "symbol:c"),
        edge("e2", "symbol:a", "symbol:b"), // same component -> no flow
    ];
    let projection = build_disposable_architecture_projection(&nodes, &edges, Some("gen-2".to_string()), 100, 200);

    assert_eq!(projection.kind, "DisposableArchitectureProjection");
    assert_eq!(projection.authority, "disposable_cited_view");
    assert_eq!(projection.planner_authority, "none");
    assert!(projection.id.starts_with("sha256:"));

    let component_ids: Vec<&str> = projection.components.iter().map(|c| c.label.as_str()).collect();
    assert!(component_ids.contains(&"src/api"));
    assert!(component_ids.contains(&"src/ui"));

    // Only e1 crosses components (src/api -> src/ui); e2 stays within src/api.
    assert_eq!(projection.flows.len(), 1);
    assert_eq!(projection.flows[0].edge_id, "e1");
    assert!(projection.omissions.is_empty());
}

#[test]
fn disposable_projection_reports_component_ceiling_omission() {
    let nodes = vec![
        node_with_path("symbol:a", "src/a/x.ts"),
        node_with_path("symbol:b", "src/b/x.ts"),
        node_with_path("symbol:c", "src/c/x.ts"),
    ];
    let projection = build_disposable_architecture_projection(&nodes, &[], None, 2, 200);
    assert_eq!(projection.components.len(), 2);
    assert!(projection.omissions.iter().any(|o| o.reason == "component_ceiling" && o.count == Some(1)));
}

#[test]
fn architecture_model_deterministic_across_repeated_calls() {
    let nodes = vec![node("a"), node("b")];
    let edges = vec![edge("e1", "a", "b")];
    let m1 = build_architecture_model(&nodes, &edges, Some("g".to_string()));
    let m2 = build_architecture_model(&nodes, &edges, Some("g".to_string()));
    assert_eq!(
        serde_json::to_string(&m1).unwrap(),
        serde_json::to_string(&m2).unwrap()
    );
}
