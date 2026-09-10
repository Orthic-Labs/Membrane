//! Parity tests for `contract_registry`, ported from the legacy
//! `blueprint/src/graph/contract-registry.mjs` behavior exercised by
//! `blueprint/tests/framework-process-contracts.test.mjs`'s
//! "contract registry and bridge stitching join only exact contract keys"
//! case:
//!
//! - a `ToolContract` node tagged with a provider role becomes a `tool`
//!   contract keyed by kind+address+schema;
//! - a same-address `ToolContract` node tagged with a consumer role in a
//!   different repo bridges to it, and only it (a near-miss address never
//!   joins);
//! - bridge stitching never joins consumer and provider rows from the same
//!   `repoId`;
//! - traces assemble consumer-then-provider steps from bridges.

use membrane_blueprint::contract_registry::{
    bridge_contract_registries, build_contract_registry, stitch_contract_traces, ContractNode, Generation,
};
use serde_json::json;

fn ev(path: &str) -> serde_json::Value {
    json!([{ "path": path, "startLine": 1, "endLine": 1, "contentHash": format!("h:{path}") }])
}

#[test]
fn contract_registry_and_bridge_stitching_join_only_exact_contract_keys() {
    let provider_tool = ContractNode {
        id: "tool:src/api.ts::ping".to_string(),
        labels: vec!["ToolContract".to_string()],
        name: Some("ping".to_string()),
        domain_identity_address: Some("ping".to_string()),
        contract_roles: vec!["provider".to_string()],
        evidence: ev("src/api.ts"),
        ..Default::default()
    };
    let provider = Generation {
        generation_id: Some("g1".to_string()),
        nodes: vec![provider_tool],
        edges: vec![],
    };
    let a = build_contract_registry(&provider, Some("provider-repo"));
    assert_eq!(a.contracts.len(), 1);
    assert_eq!(a.contracts[0].kind, "tool");
    assert_eq!(a.contracts[0].address, "ping");
    assert!(a.contracts[0].roles.iter().any(|r| r == "provider"));

    // A near-miss address ("pinger") must never bridge with "ping": exact
    // contract keys only, never fuzzy name resemblance.
    let consumer_exact = ContractNode {
        id: "tool:src/client.ts::ping".to_string(),
        labels: vec!["ToolContract".to_string()],
        name: Some("ping".to_string()),
        domain_identity_address: Some("ping".to_string()),
        contract_roles: vec!["consumer".to_string()],
        evidence: ev("src/client.ts"),
        ..Default::default()
    };
    let consumer_near_miss = ContractNode {
        id: "tool:src/client.ts::pinger".to_string(),
        labels: vec!["ToolContract".to_string()],
        name: Some("pinger".to_string()),
        domain_identity_address: Some("pinger".to_string()),
        contract_roles: vec!["consumer".to_string()],
        evidence: ev("src/client.ts"),
        ..Default::default()
    };
    let consumer = Generation {
        generation_id: Some("g2".to_string()),
        nodes: vec![consumer_exact, consumer_near_miss],
        edges: vec![],
    };
    let b = build_contract_registry(&consumer, Some("consumer-repo"));
    assert_eq!(b.contracts.len(), 2);

    let bridges = bridge_contract_registries(&[a.clone(), b.clone()]);
    assert_eq!(bridges.bridges.len(), 1);
    assert_eq!(bridges.bridges[0].address, "ping");
    assert_eq!(bridges.bridges[0].consumer_repo_id.as_deref(), Some("consumer-repo"));
    assert_eq!(bridges.bridges[0].provider_repo_id.as_deref(), Some("provider-repo"));

    let traces = stitch_contract_traces(&[a, b]);
    assert_eq!(traces.traces.len(), 1);
    let roles: Vec<&str> = traces.traces[0].steps.iter().map(|s| s.role).collect();
    assert_eq!(roles, vec!["consumer", "provider"]);
}

#[test]
fn bridge_stitching_never_joins_same_repo_provider_and_consumer() {
    let provider_role = ContractNode {
        id: "tool:a".to_string(),
        labels: vec!["ToolContract".to_string()],
        domain_identity_address: Some("shared".to_string()),
        contract_roles: vec!["provider".to_string()],
        evidence: json!([]),
        ..Default::default()
    };
    let consumer_role = ContractNode {
        id: "tool:b".to_string(),
        labels: vec!["ToolContract".to_string()],
        domain_identity_address: Some("shared".to_string()),
        contract_roles: vec!["consumer".to_string()],
        evidence: json!([]),
        ..Default::default()
    };
    let generation = Generation {
        generation_id: Some("g1".to_string()),
        nodes: vec![provider_role, consumer_role],
        edges: vec![],
    };
    let registry = build_contract_registry(&generation, Some("same-repo"));
    let bridges = bridge_contract_registries(&[registry]);
    assert!(bridges.bridges.is_empty(), "same-repo provider/consumer must never bridge");
}

#[test]
fn http_route_kind_and_address_derive_from_method_and_path() {
    let node = ContractNode {
        id: "route:1".to_string(),
        labels: vec!["HttpRoute".to_string()],
        method: Some("post".to_string()),
        route_path: Some("/widgets".to_string()),
        evidence: json!([]),
        ..Default::default()
    };
    let generation = Generation {
        generation_id: Some("g1".to_string()),
        nodes: vec![node],
        edges: vec![],
    };
    let registry = build_contract_registry(&generation, Some("repo"));
    assert_eq!(registry.contracts.len(), 1);
    assert_eq!(registry.contracts[0].kind, "http");
    assert_eq!(registry.contracts[0].address, "POST /widgets");
}

#[test]
fn node_without_a_recognized_contract_label_is_never_extracted() {
    let node = ContractNode {
        id: "file:src/api.ts".to_string(),
        labels: vec!["File".to_string()],
        evidence: json!([]),
        ..Default::default()
    };
    let generation = Generation {
        generation_id: Some("g1".to_string()),
        nodes: vec![node],
        edges: vec![],
    };
    let registry = build_contract_registry(&generation, Some("repo"));
    assert!(registry.contracts.is_empty());
}

#[test]
fn produces_and_routes_to_edges_grant_the_provider_role() {
    let target = ContractNode {
        id: "tool:x".to_string(),
        labels: vec!["ToolContract".to_string()],
        domain_identity_address: Some("x".to_string()),
        evidence: json!([]),
        ..Default::default()
    };
    let generation = Generation {
        generation_id: Some("g1".to_string()),
        nodes: vec![target],
        edges: vec![membrane_blueprint::contract_registry::ContractEdge {
            kind: "PRODUCES".to_string(),
            source: "symbol:producer".to_string(),
            target: "tool:x".to_string(),
        }],
    };
    let registry = build_contract_registry(&generation, Some("repo"));
    assert_eq!(registry.contracts.len(), 1);
    assert!(registry.contracts[0].roles.iter().any(|r| r == "provider"));
}
