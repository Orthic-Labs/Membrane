// Parity test for legacy blueprint/src/lib/federation/index.mjs, ported
// natively as engine/crates/membrane-federation/src/lib_federation_index.rs.
//
// Scenarios translated from blueprint/tests/federation-groups-contracts.test.mjs:
//   - "named federation groups validate unique bounded repository membership"
//   - "federated slices stitch only exact contract bridges without merging node spaces"
//   - "routeFederatedQuery accepts a named group and preserves per-repo generations"

use membrane_federation::lib_federation_index::{compose_federated_slices, define_federation_group, route_federated_query};
use membrane_federation::route_native_federated_query;
use membrane_blueprint::{BlueprintApi, BlueprintRequest, BlueprintResponse, CancellationToken as NativeCancellation};
use serde_json::json;
use std::sync::{Arc, Mutex};

fn provider_contract() -> serde_json::Value {
    json!({"contractId": "c", "contractKey": "sha256:k", "repoId": "provider", "kind": "tool", "address": "ping", "schema": null, "roles": ["provider"], "nodeId": "tool:ping", "evidence": []})
}
fn consumer_contract() -> serde_json::Value {
    json!({"contractId": "c", "contractKey": "sha256:k", "repoId": "consumer", "kind": "tool", "address": "ping", "schema": null, "roles": ["consumer"], "nodeId": "call:ping", "evidence": []})
}

#[test]
fn named_federation_groups_validate_unique_bounded_repository_membership() {
    let group = define_federation_group("payments", &[json!({"repoId": "a"}), json!({"repoId": "b"})]).unwrap();
    assert_eq!(group["name"], "payments");
    let repo_ids: Vec<&str> = group["repositories"].as_array().unwrap().iter().map(|r| r["repoId"].as_str().unwrap()).collect();
    assert_eq!(repo_ids, vec!["a", "b"]);

    let err = define_federation_group("bad", &[json!({"repoId": "a"}), json!({"repoId": "a"})]).unwrap_err();
    assert_eq!(err.code, "repository_duplicate");
}

#[test]
fn federated_slices_stitch_only_exact_contract_bridges_without_merging_node_spaces() {
    let slices = vec![
        json!({"repoId": "consumer", "generationId": "g1", "results": [], "contracts": [consumer_contract()]}),
        json!({"repoId": "provider", "generationId": "g2", "results": [], "contracts": [provider_contract()]}),
    ];
    let result = compose_federated_slices(&slices, Some("tools")).unwrap();
    assert_eq!(result["groupName"], "tools");
    assert_eq!(result["contractBridges"].as_array().unwrap().len(), 1);
    assert_eq!(result["traces"].as_array().unwrap().len(), 1);
    let steps: Vec<&str> = result["traces"][0]["steps"].as_array().unwrap().iter().map(|s| s["repoId"].as_str().unwrap()).collect();
    assert_eq!(steps, vec!["consumer", "provider"]);
    assert_eq!(result["slices"].as_array().unwrap().len(), 2);
}

#[test]
fn route_federated_query_accepts_a_named_group_and_preserves_per_repo_generations() {
    let repos = vec![json!({"repoId": "a", "generation": "ga"}), json!({"repoId": "b", "generation": "gb"})];
    let allowed = vec!["a".to_owned(), "b".to_owned()];
    let result = route_federated_query(
        Some(("g", &repos)),
        &[],
        &allowed,
        "architecture",
        &json!({"view": "contracts"}),
        |repo, _operation, _input| Ok(json!({"generationId": repo["generation"], "contracts": []})),
    )
    .unwrap();
    assert_eq!(result["groupName"], "g");
    let generation_ids: Vec<&str> = result["repos"].as_array().unwrap().iter().map(|r| r["generationId"].as_str().unwrap()).collect();
    assert_eq!(generation_ids, vec!["ga", "gb"]);
}

struct NativeApi {
    requests: Mutex<Vec<BlueprintRequest>>,
}

impl BlueprintApi for NativeApi {
    fn dispatch(&self, request: BlueprintRequest, _cancellation: NativeCancellation) -> BlueprintResponse {
        let generation = request.generation.clone().unwrap_or_else(|| "native-generation".into());
        self.requests.lock().unwrap().push(request.clone());
        BlueprintResponse::success(request.request_id, Some(generation.clone()), json!({
            "generationId": generation,
            "state": "complete",
            "freshness": { "revision": generation, "stale": false },
            "candidateSet": {
                "schemaVersion": 1,
                "state": "complete",
                "candidates": [{
                    "id": "node:source",
                    "layer": 3,
                    "sourceKind": "graph",
                    "sourceRef": "src/lib.rs",
                    "sourceHash": "sha256:source",
                    "trustClass": "workspace_tracked",
                    "instructionPolicy": "data_only",
                    "providerScore": 0.0,
                    "estimatedTokens": 1,
                    "protected": false,
                    "exact": true,
                    "recoverable": true,
                    "resolver": "blueprint_graph_generation",
                    "text": "source"
                }]
            },
            "omissions": []
        }))
    }
}

#[test]
fn native_route_dispatches_recall_per_repository_and_keeps_source_bound_slices() {
    let api = Arc::new(NativeApi { requests: Mutex::new(Vec::new()) });
    let repositories = vec![
        json!({"repoId":"a", "repoRoot":"C:/repo-a", "generation":"ga"}),
        json!({"repoId":"b", "repoRoot":"C:/repo-b", "generation":"gb"}),
    ];
    let result = route_native_federated_query(
        api.clone(), "request", None, &repositories, &["a".into(), "b".into()],
        "recall", &json!({"seed":"src/lib.rs"}), std::time::Duration::from_secs(1),
        NativeCancellation::new(),
    ).unwrap();
    let requests = api.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.method.as_str() == "recall"));
    assert_eq!(requests[0].input["repoId"], requests[0].repo_id.clone().unwrap());
    let generations: Vec<&str> = result["repos"].as_array().unwrap().iter()
        .map(|repo| repo["generationId"].as_str().unwrap()).collect();
    assert_eq!(generations, vec!["ga", "gb"]);
    for slice in result["slices"].as_array().unwrap() {
        assert_eq!(slice["results"].as_array().unwrap().len(), 1);
        assert_eq!(slice["results"][0]["candidateSet"]["candidates"][0]["sourceRef"], "src/lib.rs");
        assert!(slice["omissions"].as_array().unwrap().is_empty());
    }
}

#[test]
fn native_route_exposes_cancellation_as_per_repository_omissions() {
    let api = Arc::new(NativeApi { requests: Mutex::new(Vec::new()) });
    let cancellation = NativeCancellation::new();
    cancellation.cancel();
    let result = route_native_federated_query(
        api.clone(), "request", None,
        &[json!({"repoId":"a", "repoRoot":"C:/repo-a", "generation":"ga"})],
        &["a".into()], "recall", &json!({}), std::time::Duration::from_secs(1), cancellation,
    ).unwrap();
    assert!(api.requests.lock().unwrap().is_empty());
    assert_eq!(result["slices"][0]["results"].as_array().unwrap().len(), 0);
    assert_eq!(result["slices"][0]["omissions"][0]["code"], "request_cancelled");
}
