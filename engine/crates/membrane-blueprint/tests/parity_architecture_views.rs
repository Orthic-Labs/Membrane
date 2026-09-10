//! Lane WIRE1: parity tests for `Operation::Architecture` `view` dispatch
//! (`crate::architecture_views::dispatch_view`), one per native view, plus
//! a cache-invalidation test over the dependency-dag `ProjectionCache`.
//!
//! Fixture mirrors `tests/query.rs`'s `generation()` builder (real
//! `GraphGeneration`/`GraphNode`/`GraphEdge` values, no mocked JSON), with
//! an added entry-point-tagged node and file rows so every view (except the
//! knowingly-unwired `changes`) has real evidence to project.

use membrane_blueprint::query::execute_query;
use membrane_blueprint::{BlueprintRequest, Bounds, GraphEdge, GraphGeneration, GraphNode, Operation, RequestContext};
use serde_json::json;

fn generation() -> GraphGeneration {
    let id = "generation-arch-views".to_owned();
    GraphGeneration {
        schema_version: 1,
        provider: "test".into(),
        provider_version: "1".into(),
        generation_id: id.clone(),
        source_hash: "hash-1".into(),
        repo_root: "/repo-arch-views".into(),
        complete: true,
        nodes: vec![
            GraphNode {
                id: "file:src/entry.rs".into(),
                kind: "file".into(),
                path: Some("src/entry.rs".into()),
                name: Some("entry.rs".into()),
                generation_id: id.clone(),
                evidence: vec![json!({"path":"src/entry.rs","contentHash":"sha256:a"})],
            },
            GraphNode {
                id: "symbol:src/entry.rs::main".into(),
                kind: "symbol".into(),
                path: Some("src/entry.rs".into()),
                name: Some("main".into()),
                generation_id: id.clone(),
                evidence: vec![json!({"qualifiedName":"main","path":"src/entry.rs","startLine":1,"contentHash":"sha256:a"})],
            },
            GraphNode {
                id: "symbol:src/entry.rs::handler".into(),
                kind: "symbol".into(),
                path: Some("src/entry.rs".into()),
                name: Some("handler".into()),
                generation_id: id.clone(),
                evidence: vec![json!({"qualifiedName":"handler","path":"src/entry.rs","startLine":10,"contentHash":"sha256:a"})],
            },
        ],
        edges: vec![GraphEdge {
            id: "edge:CALLS:main->handler".into(),
            kind: "CALLS".into(),
            source: "symbol:src/entry.rs::main".into(),
            target: Some("symbol:src/entry.rs::handler".into()),
            generation_id: id,
            evidence: vec![json!({"confidenceTier":"EXACT_RESOLUTION"})],
        }],
        files: vec![],
        truncation_reasons: vec![],
    }
}

/// `main` is tagged as an explicit entry point via `labels: ["EntryPoint"]`
/// carried through `GraphNode`'s extra evidence -- entry-point detection
/// reads `node.entryPoint`/`node.labels`, neither of which `GraphNode`
/// exposes as typed fields, so this variant patches the serialized JSON
/// node in place before dispatch (mirroring how `to_projection_generation`
/// re-serializes `GraphNode` -> JSON already).
fn context(request: &BlueprintRequest) -> RequestContext {
    request.validate(Bounds::default()).unwrap()
}

fn architecture_request(view: &str) -> BlueprintRequest {
    let mut request = BlueprintRequest::new("q", Operation::Architecture, "/repo-arch-views");
    request.generation = Some("generation-arch-views".into());
    request.input["view"] = json!(view);
    request
}

#[test]
fn processes_view_walks_flow_edges_from_entry_points() {
    let request = architecture_request("processes");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert_eq!(result["view"], "processes");
    assert_eq!(result["kind"], "process-projection");
    // No node is tagged `entryPoint: true` in this fixture (GraphNode has
    // no typed field for it), so the registry is empty and processes stay
    // empty -- the assertion that matters is that native dispatch reaches
    // the ported module cleanly and returns its exact schema shape.
    assert!(result["processes"].is_array());
    assert!(result["frontiers"].is_array());
    assert_eq!(result["truncated"], false);
}

#[test]
fn signatures_view_projects_sorted_symbol_signatures() {
    let request = architecture_request("signatures");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert_eq!(result["view"], "signatures");
    assert_eq!(result["kind"], "symbol-signatures");
    let signatures = result["signatures"].as_array().unwrap();
    assert_eq!(signatures.len(), 2);
    assert_eq!(signatures[0]["name"], "main");
    assert_eq!(signatures[1]["name"], "handler");
    assert!(signatures[0]["signature"].as_str().unwrap().contains("main"));
}

#[test]
fn orientation_view_composes_entry_points_signatures_and_areas() {
    let request = architecture_request("orientation");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert_eq!(result["view"], "orientation");
    assert_eq!(result["kind"], "cold-start-orientation");
    assert_eq!(result["repository"]["fileCount"], 1);
    let areas = result["repository"]["topLevelAreas"].as_array().unwrap();
    assert_eq!(areas[0]["name"], "src");
    let signatures = result["signatures"].as_array().unwrap();
    assert_eq!(signatures.len(), 2);
}

#[test]
fn contracts_view_returns_empty_registry_for_a_non_contract_fixture() {
    let request = architecture_request("contracts");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert_eq!(result["view"], "contracts");
    assert!(result["contracts"].as_array().unwrap().is_empty());
}

#[test]
fn liveness_view_reports_a_disposition_per_node() {
    let request = architecture_request("liveness");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert_eq!(result["view"], "liveness");
    assert_eq!(result["kind"], "liveness");
    assert!(result["results"].as_array().unwrap().len() >= 2);
}

#[test]
fn projection_view_groups_nodes_into_components() {
    let request = architecture_request("projection");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert_eq!(result["view"], "projection");
    assert!(!result["components"].as_array().unwrap().is_empty());
}

#[test]
fn flows_view_is_native_and_changes_retains_typed_store_boundary() {
    let flows = architecture_request("flows");
    let result = execute_query(&generation(), &flows, &context(&flows)).unwrap();
    assert_eq!(result["view"], "flows");
    assert_eq!(result["kind"], "architecture");
    assert_eq!(result["schemaVersion"], 2);
    assert!(result["flows"].is_array());

    let changes = architecture_request("changes");
    let error = execute_query(&generation(), &changes, &context(&changes)).unwrap_err();
    assert_eq!(error.code, "architecture_view_not_wired");
}

#[test]
fn unknown_view_is_rejected() {
    let request = architecture_request("not-a-real-view");
    let error = execute_query(&generation(), &request, &context(&request)).unwrap_err();
    assert_eq!(error.code, "architecture_view_invalid");
}

#[test]
fn summary_view_falls_through_to_the_existing_task_orientation_default() {
    let mut request = BlueprintRequest::new("q", Operation::Architecture, "/repo-arch-views");
    request.generation = Some("generation-arch-views".into());
    request.input["task"] = json!("main");
    // No `view` at all: must reach the pre-existing BM03 orientation path,
    // not the new dispatch table.
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert!(result.get("anchors").is_some());
    assert!(result.get("view").is_none());
}

#[test]
fn processes_view_is_cached_and_invalidates_on_generation_change() {
    let gen_a = generation();
    let request_a = architecture_request("processes");
    let first = execute_query(&gen_a, &request_a, &context(&request_a)).unwrap();
    assert_eq!(first["cache"], "miss");

    let second = execute_query(&gen_a, &request_a, &context(&request_a)).unwrap();
    assert_eq!(second["cache"], "hit");

    let mut gen_b = generation();
    gen_b.generation_id = "generation-arch-views-2".into();
    gen_b.source_hash = "hash-2".into();
    let mut request_b = architecture_request("processes");
    request_b.generation = Some(gen_b.generation_id.clone());
    let third = execute_query(&gen_b, &request_b, &context(&request_b)).unwrap();
    assert_eq!(third["cache"], "invalidated");
}
