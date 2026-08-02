use crypt::blueprint_closure::{
    close_blueprint_dependencies, BlueprintClosureOptionsV1, BlueprintEdgeKindV1,
    BlueprintGraphEdgeV1,
};

fn edge(from: &str, to: &str, kind: BlueprintEdgeKindV1) -> BlueprintGraphEdgeV1 {
    BlueprintGraphEdgeV1::new(from, to, kind)
}

#[test]
fn closes_all_supported_blueprint_edge_kinds_in_breadth_first_order() {
    let graph = vec![
        edge("handler", "handler::parse", BlueprintEdgeKindV1::Symbol),
        edge("handler", "service", BlueprintEdgeKindV1::Call),
        edge("handler", "config", BlueprintEdgeKindV1::Import),
        edge("handler", "handler_test", BlueprintEdgeKindV1::Test),
        edge("service", "repository", BlueprintEdgeKindV1::Call),
    ];

    let closure = close_blueprint_dependencies(
        &["handler".into()],
        &graph,
        BlueprintClosureOptionsV1::default(),
    );

    assert_eq!(
        closure
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node.depth))
            .collect::<Vec<_>>(),
        vec![
            ("handler", 0),
            ("handler::parse", 1),
            ("service", 1),
            ("config", 1),
            ("handler_test", 1),
            ("repository", 2),
        ]
    );
    assert_eq!(closure.included_edges.len(), 5);
    assert!(!closure.truncated_by_depth);
    assert!(!closure.truncated_by_node_limit);
}

#[test]
fn terminates_cycles_without_duplicate_nodes_or_edges() {
    let graph = vec![
        edge("a", "b", BlueprintEdgeKindV1::Call),
        edge("b", "c", BlueprintEdgeKindV1::Import),
        edge("c", "a", BlueprintEdgeKindV1::Test),
    ];

    let closure =
        close_blueprint_dependencies(&["a".into()], &graph, BlueprintClosureOptionsV1::default());

    assert_eq!(
        closure
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
    assert_eq!(closure.included_edges.len(), 2);
    assert_eq!(closure.cycle_edges_skipped, 1);
}

#[test]
fn stops_expansion_at_depth_limit_without_losing_boundary_nodes() {
    let graph = vec![
        edge("a", "b", BlueprintEdgeKindV1::Call),
        edge("b", "c", BlueprintEdgeKindV1::Call),
        edge("c", "d", BlueprintEdgeKindV1::Call),
    ];
    let options = BlueprintClosureOptionsV1 {
        max_depth: 1,
        max_nodes: 10,
    };

    let closure = close_blueprint_dependencies(&["a".into()], &graph, options);

    assert_eq!(
        closure
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert_eq!(closure.included_edges.len(), 1);
    assert!(closure.truncated_by_depth);
    assert!(!closure.truncated_by_node_limit);
}

#[test]
fn applies_node_cap_before_admitting_more_dependencies() {
    let graph = vec![
        edge("a", "b", BlueprintEdgeKindV1::Call),
        edge("a", "c", BlueprintEdgeKindV1::Import),
        edge("a", "d", BlueprintEdgeKindV1::Test),
    ];
    let options = BlueprintClosureOptionsV1 {
        max_depth: 2,
        max_nodes: 2,
    };

    let closure = close_blueprint_dependencies(&["a".into()], &graph, options);

    assert_eq!(
        closure
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert_eq!(closure.included_edges.len(), 1);
    assert!(closure.truncated_by_node_limit);
}
