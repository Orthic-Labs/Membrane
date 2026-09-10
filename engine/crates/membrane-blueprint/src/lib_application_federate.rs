//! Native port of `Operation::Federate`, mirroring `service.mjs`'s
//! `federate` operation and `blueprint/src/lib/federation/index.mjs`'s
//! `routeFederatedQuery`/`composeFederatedSlices`.
//!
//! Cross-repo routing here reuses the existing per-request confinement
//! (`engine::confined_root`, which requires `input.repoRoot` to equal the
//! request's own canonical scope root) by building and fully re-validating
//! one independent `BlueprintRequest`/`RequestContext` pair per repository
//! through `BlueprintApi::dispatch`, rather than composing a second planner.
//! Each repo's slice is confined to its own root exactly as a standalone
//! call to that operation would be.
//!
//! Scoped deviation (recorded in this lane's receipt): legacy's named
//! federation `group` config (`defineFederationGroup`) is not accepted --
//! this lane only accepts `input.repositories`/`input.allowedRepoIds` (the
//! shipped callers confirmed by LIB5's blocker note all pass repositories
//! explicitly, not a named group).
//!
//! Gap 4 (lane STORE2, closed): `contractBridges`/`traces` are now produced
//! by `crate::contract_registry::{build_contract_registry, stitch_contract_traces}`
//! -- the same native port `membrane-federation`'s `lib_federation_index.rs`
//! already calls (`membrane_blueprint::contract_registry::stitch_contract_traces`).
//! `membrane-federation` depends on `membrane-blueprint`, so this crate calls
//! `contract_registry` directly rather than through `membrane-federation`
//! (a `membrane-blueprint -> membrane-federation` edge would be circular).
//! Each federated repository's own persisted generation (loaded read-only
//! from that repo's own store, independent of the per-repo query slice
//! result) is converted into a `contract_registry::Generation` and stitched
//! across every repository exactly as `build_contract_registry`/
//! `stitch_contract_traces` already do for the single-crate case.

use crate::api::{BlueprintApi, BlueprintError, BlueprintRequest, CancellationToken};
use crate::contract_registry::{self, ContractEdge, ContractNode};
use crate::model::Operation;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

fn contract_node_from_json(node: &Value) -> ContractNode {
    ContractNode {
        id: node.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
        labels: node.get("labels").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default(),
        name: node.get("name").and_then(Value::as_str).map(str::to_owned),
        method: node.get("method").and_then(Value::as_str).map(str::to_owned),
        route_path: node.get("routePath").and_then(Value::as_str).map(str::to_owned),
        domain_identity_address: node.get("domainIdentity").and_then(|d| d.get("address")).and_then(Value::as_str).map(str::to_owned),
        contract_schema: node.get("contractSchema").cloned().filter(|v| !v.is_null()),
        contract_roles: node.get("contractRoles").and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .or_else(|| node.get("contractRole").and_then(Value::as_str).map(|r| vec![r.to_owned()]))
            .unwrap_or_default(),
        portable_id: node.get("portableId").and_then(Value::as_str).map(str::to_owned),
        evidence: node.get("evidence").cloned().unwrap_or(json!([])),
    }
}

fn contract_edge_from_json(edge: &Value) -> ContractEdge {
    ContractEdge {
        kind: edge.get("kind").and_then(Value::as_str).unwrap_or_default().to_owned(),
        source: edge.get("source").and_then(Value::as_str).unwrap_or_default().to_owned(),
        target: edge.get("target").and_then(Value::as_str).unwrap_or_default().to_owned(),
    }
}

/// Load one repo's own persisted generation, read-only, and convert it into
/// a `contract_registry::ContractRegistry`. Returns `None` (rather than an
/// error) when that repo has no persisted generation yet -- a repo simply
/// contributes no contracts to the stitched projection, matching legacy's
/// `slice.contracts` being empty/absent for a repo that returned no result.
fn contract_registry_for_repo(repo_root: &str, repo_id: &str) -> Option<contract_registry::ContractRegistry> {
    let db_path = crate::security::canonical_root(Path::new(repo_root)).ok().map(|root| root.join(".agent").join("graph").join("graph.db"))?;
    if !db_path.exists() {
        return None;
    }
    let connection = crate::store::open_store_read_only(&db_path).ok()?;
    let generation = crate::store::load_generation(&connection).ok()??;
    let contract_generation = contract_registry::Generation {
        generation_id: generation.generation_id().map(str::to_owned),
        nodes: generation.nodes.iter().map(contract_node_from_json).collect(),
        edges: generation.edges.iter().map(contract_edge_from_json).collect(),
    };
    Some(contract_registry::build_contract_registry(&contract_generation, Some(repo_id)))
}

fn bridge_to_json(bridge: &contract_registry::Bridge) -> Value {
    json!({
        "id": bridge.id,
        "contractKey": bridge.contract_key,
        "kind": bridge.kind,
        "address": bridge.address,
        "consumer": { "repoId": bridge.consumer_repo_id, "nodeId": bridge.consumer_node_id, "generationId": bridge.consumer_generation_id },
        "provider": { "repoId": bridge.provider_repo_id, "nodeId": bridge.provider_node_id, "generationId": bridge.provider_generation_id },
        "evidence": bridge.evidence,
    })
}

fn trace_to_json(trace: &contract_registry::Trace) -> Value {
    json!({
        "id": trace.id,
        "contractKey": trace.contract_key,
        "steps": trace.steps.iter().map(|s| json!({"repoId": s.repo_id, "nodeId": s.node_id, "role": s.role, "generationId": s.generation_id})).collect::<Vec<_>>(),
        "evidence": trace.evidence,
    })
}

const ALLOWED_OPERATIONS: [&str; 4] = ["search", "recall", "impact", "architecture"];

fn fail(code: &'static str, message: impl Into<String>) -> BlueprintError {
    BlueprintError::new(code, message.into())
}

struct RepoSpec {
    repo_id: String,
    repo_root: String,
    generation: Option<String>,
}

pub fn execute_federate(request: &BlueprintRequest, cancellation: &CancellationToken) -> Result<Value, BlueprintError> {
    let repositories_value = request.input.get("repositories").and_then(Value::as_array).cloned().unwrap_or_default();
    if repositories_value.is_empty() || repositories_value.len() > 16 {
        return Err(fail("federation_bounds_invalid", "federation requires 1 to 16 explicit repositories"));
    }
    let mut repositories = Vec::with_capacity(repositories_value.len());
    let mut seen_ids = BTreeSet::new();
    for entry in &repositories_value {
        let repo_id = entry.get("repoId").and_then(Value::as_str).filter(|v| !v.is_empty())
            .ok_or_else(|| fail("repository_duplicate", "each federated repository needs one unique repoId"))?
            .to_owned();
        if !seen_ids.insert(repo_id.clone()) {
            return Err(fail("repository_duplicate", "each federated repository needs one unique repoId"));
        }
        let repo_root = entry.get("repoRoot").and_then(Value::as_str).filter(|v| !v.is_empty())
            .ok_or_else(|| fail("repository_duplicate", "each federated repository needs a repoRoot"))?
            .to_owned();
        let generation = entry.get("generation").and_then(Value::as_str).map(str::to_owned);
        repositories.push(RepoSpec { repo_id, repo_root, generation });
    }

    let allowed: BTreeSet<String> = match request.input.get("allowedRepoIds").and_then(Value::as_array) {
        Some(values) => values.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        None => repositories.iter().map(|r| r.repo_id.clone()).collect(),
    };
    for repo in &repositories {
        if !allowed.contains(&repo.repo_id) {
            return Err(fail("repository_not_allowed", format!("repo {} is outside the explicit federation allowlist", repo.repo_id)));
        }
    }

    let operation_name = request.input.get("operation").and_then(Value::as_str).unwrap_or("recall");
    if !ALLOWED_OPERATIONS.contains(&operation_name) {
        return Err(fail("federation_operation_invalid", format!("unsupported federated operation {operation_name}")));
    }
    let operation = Operation::parse(operation_name)
        .ok_or_else(|| fail("federation_operation_invalid", format!("unsupported federated operation {operation_name}")))?;

    let query_input = request.input.get("query").cloned().unwrap_or_else(|| json!({}));

    let mut slices = Vec::with_capacity(repositories.len());
    let operation_target = crate::engine::native_blueprint_operation();
    for repo in &repositories {
        let mut child_input = query_input.clone();
        if let Value::Object(map) = &mut child_input {
            map.insert("repoRoot".into(), json!(repo.repo_root));
            map.insert("repoId".into(), json!(repo.repo_id));
            if let Some(generation) = &repo.generation {
                map.insert("generation".into(), json!(generation));
            }
        }
        let mut child_request = BlueprintRequest::new(format!("{}-federate-{}", request.request_id, repo.repo_id), operation, repo.repo_root.clone());
        child_request.input = child_input;
        child_request.repo_id = Some(repo.repo_id.clone());
        child_request.generation = repo.generation.clone();
        child_request.deadline_ms = request.deadline_ms;
        let response = operation_target.dispatch(child_request, cancellation.clone());
        let observed_generation = response.generation.clone();
        let response_error = response.error;
        let result = response.result;
        let (generation_id, results, omissions, receipt_id, contracts) = if response.ok {
            let result = result.unwrap_or(Value::Null);
            let generation_id = result.get("generationId").cloned()
                .or_else(|| observed_generation.map(Value::String))
                .or_else(|| repo.generation.clone().map(Value::String))
                .unwrap_or_else(|| json!("unavailable"));
            let mut omissions = result.get("omissions").cloned().unwrap_or_else(|| json!([]));
            if !omissions.is_array() {
                omissions = json!([{
                    "reason": "malformed_omissions",
                    "repoId": repo.repo_id,
                    "detail": "native Blueprint omissions must be an array",
                }]);
            }
            let results = if result.is_null() { json!([]) } else { json!([result.clone()]) };
            let receipt_id = result.get("freshnessReceipt").and_then(|receipt| receipt.get("receiptId")).cloned()
                .or_else(|| result.get("freshnessReceiptId").cloned())
                .unwrap_or(Value::Null);
            let contracts = result.get("contracts").cloned().unwrap_or_else(|| json!([]));
            (generation_id, results, omissions, receipt_id, contracts)
        } else {
            let error = response_error.unwrap_or_else(|| fail("blueprint_unavailable", "native Blueprint request failed"));
            let generation_id = observed_generation.map(Value::String)
                .or_else(|| repo.generation.clone().map(Value::String))
                .unwrap_or_else(|| json!("unavailable"));
            let omissions = json!([{
                "reason": "repository_query_failed",
                "repoId": repo.repo_id,
                "code": error.code,
                "message": error.message,
            }]);
            (generation_id, json!([]), omissions, Value::Null, json!([]))
        };
        slices.push(json!({
            "repoId": repo.repo_id,
            "repoRoot": repo.repo_root,
            "generationId": generation_id,
            "receiptId": receipt_id,
            "results": results,
            "omissions": omissions,
            "contracts": contracts,
        }));
    }

    let repos: Vec<Value> = slices
        .iter()
        .map(|slice| json!({
            "repoId": slice["repoId"],
            "repoRoot": slice["repoRoot"],
            "generationId": slice["generationId"],
        }))
        .collect();

    // Gap 4 (lane STORE2): stitch contract bridges/traces across each
    // repository's own persisted generation, exactly as
    // `contract_registry::stitch_contract_traces` already does for the
    // single-repo case.
    let mut federation_omissions: Vec<Value> = Vec::new();
    let mut registries = Vec::with_capacity(repositories.len());
    for repo in &repositories {
        match contract_registry_for_repo(&repo.repo_root, &repo.repo_id) {
            Some(registry) => registries.push(registry),
            None => federation_omissions.push(json!({
                "reason": "repository_generation_unavailable",
                "repoId": repo.repo_id,
                "detail": "no persisted Blueprint generation to source contracts from",
            })),
        }
    }
    let stitching = contract_registry::stitch_contract_traces(&registries);

    Ok(json!({
        "schemaVersion": 1,
        "kind": "federated",
        "groupName": Value::Null,
        "repos": repos,
        "plannerAuthority": "external",
        "selection": "unranked_repository_slices",
        "slices": slices,
        "results": slices.iter().map(|slice| json!({"repoId": slice["repoId"], "generationId": slice["generationId"], "results": slice["results"]})).collect::<Vec<_>>(),
        "contractBridges": stitching.bridges.iter().map(bridge_to_json).collect::<Vec<_>>(),
        "traces": stitching.traces.iter().map(trace_to_json).collect::<Vec<_>>(),
        "omissions": federation_omissions,
    }))
}
