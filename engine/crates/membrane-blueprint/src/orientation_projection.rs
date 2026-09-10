//! Native Rust port of `blueprint/src/graph/orientation.mjs`
//! (`buildColdStartOrientation`).
//!
//! Composes the entry-point registry, contract registry, symbol-signature
//! projection, and weak convention evidence into one cold-start orientation
//! payload. Operates on the same `nodes`/`edges` JSON shape carried by
//! [`crate::store::Generation`].

use crate::contract_registry::{self, Contract, ContractEdge, ContractNode};
use crate::conventions::{detect_project_conventions, ConventionFile, DetectOptions, WeakEvidence};
use crate::entry_points::build_entry_point_registry;
use crate::signature_projection::{project_symbol_signatures, SignatureProjectionOptions};
use crate::store::Generation;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct OrientationOptions {
    pub signature_limit: Option<i64>,
    pub entry_point_limit: Option<i64>,
    pub contract_limit: Option<i64>,
}

pub(crate) fn contract_node_from_json(node: &Value) -> ContractNode {
    ContractNode {
        id: node.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
        labels: node
            .get("labels")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        name: node.get("name").and_then(Value::as_str).map(str::to_owned),
        method: node.get("method").and_then(Value::as_str).map(str::to_owned),
        route_path: node.get("routePath").and_then(Value::as_str).map(str::to_owned),
        domain_identity_address: node
            .get("domainIdentity")
            .and_then(|d| d.get("address"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        contract_schema: node.get("contractSchema").cloned().filter(|v| !v.is_null()),
        contract_roles: node
            .get("contractRoles")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .or_else(|| node.get("contractRole").and_then(Value::as_str).map(|r| vec![r.to_owned()]))
            .unwrap_or_default(),
        portable_id: node.get("portableId").and_then(Value::as_str).map(str::to_owned),
        evidence: node.get("evidence").cloned().unwrap_or(json!([])),
    }
}

pub(crate) fn contract_edge_from_json(edge: &Value) -> ContractEdge {
    ContractEdge {
        kind: edge.get("kind").and_then(Value::as_str).unwrap_or_default().to_owned(),
        source: edge.get("source").and_then(Value::as_str).unwrap_or_default().to_owned(),
        target: edge.get("target").and_then(Value::as_str).unwrap_or_default().to_owned(),
    }
}

/// Convert a [`crate::store::Generation`] into a `contract_registry::Generation`.
pub fn contract_generation_from_store(generation: &Generation, repo_id: Option<&str>) -> contract_registry::Generation {
    let _ = repo_id;
    contract_registry::Generation {
        generation_id: generation.manifest.as_ref().and_then(|m| m.get("generationId")).and_then(Value::as_str).map(str::to_owned),
        nodes: generation.nodes.iter().map(contract_node_from_json).collect(),
        edges: generation.edges.iter().map(contract_edge_from_json).collect(),
    }
}

pub fn contract_to_json(contract: &Contract) -> Value {
    json!({
        "contractId": contract.contract_id,
        "contractKey": contract.contract_key,
        "repoId": contract.repo_id,
        "kind": contract.kind,
        "address": contract.address,
        "schema": contract.schema,
        "roles": contract.roles,
        "nodeId": contract.node_id,
        "portableId": contract.portable_id,
        "evidence": contract.evidence,
    })
}

pub fn contract_registry_to_json(registry: &contract_registry::ContractRegistry) -> Value {
    json!({
        "schemaVersion": registry.schema_version,
        "repoId": registry.repo_id,
        "generationId": registry.generation_id,
        "contracts": registry.contracts.iter().map(contract_to_json).collect::<Vec<_>>(),
    })
}

fn weak_evidence_to_json(item: &WeakEvidence) -> Value {
    json!({
        "kind": item.kind,
        "evidenceClass": item.evidence_class,
        "claim": item.claim,
        "support": item.support,
        "total": item.total,
        "coverage": item.coverage,
        "examples": item.examples,
        "counterexamples": item.counterexamples,
        "policyAuthority": item.policy_authority,
    })
}

/// Mirrors `buildColdStartOrientation(generation, files, options)`. `files`
/// carries `{ path }` rows the same way the legacy caller derives them from
/// `generation.nodes` filtered to `kind === "file"`.
pub fn build_cold_start_orientation(generation: &Generation, files: &[String], options: &OrientationOptions) -> Value {
    let generation_id = generation
        .manifest
        .as_ref()
        .and_then(|m| m.get("generationId"))
        .cloned()
        .unwrap_or(Value::Null);

    // JS destructuring defaults apply only when absent. The entry/contract
    // slices then apply `Math.max(1, value)`, while signature projection owns
    // its own Number/limit normalization.
    let signature_limit = options.signature_limit.unwrap_or(40);
    let entry_point_limit = options.entry_point_limit.unwrap_or(24).max(1) as usize;
    let contract_limit = options.contract_limit.unwrap_or(40).max(1) as usize;

    let mut directories: BTreeMap<String, u64> = BTreeMap::new();
    for path in files {
        let normalized = path.replace('\\', "/");
        if normalized.is_empty() {
            continue;
        }
        let dir = if let Some(idx) = normalized.find('/') { normalized[..idx].to_string() } else { ".".to_string() };
        *directories.entry(dir).or_insert(0) += 1;
    }
    let mut top_level_areas: Vec<(String, u64)> = directories.into_iter().collect();
    top_level_areas.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top_level_areas: Vec<Value> = top_level_areas
        .into_iter()
        .take(20)
        .map(|(name, count)| json!({ "name": name, "fileCount": count }))
        .collect();

    let all_entries = build_entry_point_registry(generation, false);
    let entry_points: Vec<Value> = all_entries
        .iter()
        .take(entry_point_limit)
        .map(|entry| {
            let node = entry.get("node").cloned().unwrap_or(Value::Null);
            // Keep undefined legacy properties absent while retaining the
            // explicit null used by the source's `name ?? null` fallback.
            let mut row = serde_json::Map::new();
            if let Some(value) = node.get("id").filter(|value| !value.is_null()) {
                row.insert("id".to_string(), value.clone());
            }
            for key in ["kind", "confidence"] {
                if let Some(value) = entry.get(key).filter(|value| !value.is_null()) {
                    row.insert(key.to_string(), value.clone());
                }
            }
            if let Some(value) = node.get("path").filter(|value| !value.is_null()) {
                row.insert("path".to_string(), value.clone());
            }
            let name = node.get("qualifiedName").filter(|value| !value.is_null())
                .or_else(|| node.get("name").filter(|value| !value.is_null()))
                .cloned().unwrap_or(Value::Null);
            row.insert("name".to_string(), name);
            row.insert("evidence".to_string(), entry.get("evidence").cloned().unwrap_or_else(|| json!([])));
            Value::Object(row)
        })
        .collect();

    let contract_generation = contract_generation_from_store(generation, None);
    let registry = contract_registry::build_contract_registry(&contract_generation, None);
    let all_contracts_len = registry.contracts.len();
    let contracts: Vec<Value> = registry.contracts.iter().take(contract_limit).map(contract_to_json).collect();

    let signatures = project_symbol_signatures(generation, &SignatureProjectionOptions { limit: Some(signature_limit), ..Default::default() });
    let signature_rows = signatures.get("signatures").cloned().unwrap_or_else(|| json!([]));
    let signatures_truncated = signatures.get("truncated").and_then(Value::as_bool).unwrap_or(false);

    let convention_files: Vec<ConventionFile> = files.iter().map(|path| ConventionFile { path: path.clone() }).collect();
    let conventions = detect_project_conventions(&convention_files, &DetectOptions::default());
    let convention_evidence: Vec<Value> = conventions.evidence.iter().map(weak_evidence_to_json).collect();

    let mut omissions: Vec<Value> = Vec::new();
    if signatures_truncated {
        omissions.push(json!({ "reason": "signature_limit", "limit": signature_limit }));
    }
    if all_entries.len() > entry_point_limit {
        omissions.push(json!({ "reason": "entrypoint_limit", "limit": entry_point_limit }));
    }
    if all_contracts_len > contract_limit {
        omissions.push(json!({ "reason": "contract_limit", "limit": contract_limit }));
    }

    json!({
        "schemaVersion": 1,
        "kind": "cold-start-orientation",
        "generationId": generation_id,
        "repository": {
            "fileCount": files.len(),
            "topLevelAreas": top_level_areas,
        },
        "entryPoints": entry_points,
        "contracts": contracts,
        "signatures": signature_rows,
        "conventions": convention_evidence,
        "omissions": omissions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation_from(nodes: Vec<Value>) -> Generation {
        Generation { manifest: Some(json!({ "generationId": "gen-1" })), nodes, ..Default::default() }
    }

    #[test]
    fn composes_entrypoints_signatures_and_top_level_areas() {
        let generation = generation_from(vec![
            json!({ "id": "a", "kind": "symbol", "labels": ["Function"], "name": "a", "path": "src/a.rs", "entryPoint": true, "evidence": [{"path": "src/a.rs"}] }),
        ]);
        let files = vec!["src/a.rs".to_string(), "src/b.rs".to_string(), "docs/readme.md".to_string()];
        let out = build_cold_start_orientation(&generation, &files, &OrientationOptions::default());
        assert_eq!(out["kind"], json!("cold-start-orientation"));
        assert_eq!(out["repository"]["fileCount"], json!(3));
        let areas = out["repository"]["topLevelAreas"].as_array().unwrap();
        assert_eq!(areas[0]["name"], json!("src"));
        assert_eq!(areas[0]["fileCount"], json!(2));
        let entry_points = out["entryPoints"].as_array().unwrap();
        assert_eq!(entry_points.len(), 1);
        assert_eq!(entry_points[0]["id"], json!("a"));
        let signatures = out["signatures"].as_array().unwrap();
        assert_eq!(signatures.len(), 1);
    }

    #[test]
    fn reports_entrypoint_limit_omission_when_truncated() {
        let nodes: Vec<Value> = (0..3)
            .map(|i| json!({ "id": format!("e{i}"), "kind": "symbol", "name": format!("e{i}"), "path": "src/a.rs", "entryPoint": true, "evidence": [{"path": "src/a.rs"}] }))
            .collect();
        let generation = generation_from(nodes);
        let out = build_cold_start_orientation(&generation, &[], &OrientationOptions { entry_point_limit: Some(1), ..Default::default() });
        let omissions = out["omissions"].as_array().unwrap();
        assert!(omissions.iter().any(|o| o["reason"] == json!("entrypoint_limit")));
        assert_eq!(out["entryPoints"].as_array().unwrap().len(), 1);
    }
}
