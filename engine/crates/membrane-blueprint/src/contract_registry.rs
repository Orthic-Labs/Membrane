//! Native port of `blueprint/src/graph/contract-registry.mjs`.
//!
//! Reproduces the legacy JS contract registry rules exactly: contract
//! extraction from generation nodes, exact-key-only bridge stitching across
//! repositories, and trace assembly from bridges. No policy authority is
//! implied here; this is descriptive projection over already-published
//! generation data.

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

/// A minimal node view sufficient to reproduce the legacy extraction rules.
#[derive(Debug, Clone, Default)]
pub struct ContractNode {
    pub id: String,
    pub labels: Vec<String>,
    pub name: Option<String>,
    pub method: Option<String>,
    pub route_path: Option<String>,
    pub domain_identity_address: Option<String>,
    pub contract_schema: Option<Value>,
    pub contract_roles: Vec<String>,
    pub portable_id: Option<String>,
    pub evidence: Value,
}

#[derive(Debug, Clone, Default)]
pub struct ContractEdge {
    pub kind: String,
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Default)]
pub struct Generation {
    pub generation_id: Option<String>,
    pub nodes: Vec<ContractNode>,
    pub edges: Vec<ContractEdge>,
}

#[derive(Debug, Clone)]
pub struct Contract {
    pub contract_id: String,
    pub contract_key: String,
    pub repo_id: Option<String>,
    pub kind: String,
    pub address: String,
    pub schema: Option<Value>,
    pub roles: Vec<String>,
    pub node_id: String,
    pub portable_id: Option<String>,
    pub evidence: Value,
}

#[derive(Debug, Clone)]
pub struct ContractRegistry {
    pub schema_version: u32,
    pub repo_id: Option<String>,
    pub generation_id: Option<String>,
    pub contracts: Vec<Contract>,
}

#[derive(Debug, Clone)]
pub struct Bridge {
    pub id: String,
    pub contract_key: String,
    pub kind: String,
    pub address: String,
    pub consumer_repo_id: Option<String>,
    pub consumer_node_id: String,
    pub consumer_generation_id: Option<String>,
    pub provider_repo_id: Option<String>,
    pub provider_node_id: String,
    pub provider_generation_id: Option<String>,
    pub evidence: Value,
}

#[derive(Debug, Clone)]
pub struct BridgeProjection {
    pub schema_version: u32,
    pub bridges: Vec<Bridge>,
}

#[derive(Debug, Clone)]
pub struct TraceStep {
    pub repo_id: Option<String>,
    pub node_id: String,
    pub role: &'static str,
    pub generation_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Trace {
    pub id: String,
    pub contract_key: String,
    pub steps: Vec<TraceStep>,
    pub evidence: Value,
}

#[derive(Debug, Clone)]
pub struct TraceProjection {
    pub schema_version: u32,
    pub bridges: Vec<Bridge>,
    pub traces: Vec<Trace>,
}

/// Deterministic, key-sorted JSON serialization matching the legacy `stable()`
/// helper, followed by sha256 hexdigest, matching `digest()`.
fn stable(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(stable).collect()),
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for key in keys {
                out.insert(key.clone(), stable(map.get(key).unwrap()));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn digest(value: &Value) -> String {
    let stabilized = stable(value);
    let serialized = serde_json::to_string(&stabilized).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    let hex = hex::encode(hasher.finalize());
    format!("sha256:{hex}")
}

fn strip_sha256_prefix(key: &str) -> &str {
    key.strip_prefix("sha256:").unwrap_or(key)
}

fn roles_for_node(generation: &Generation, node: &ContractNode) -> Vec<String> {
    let mut roles: std::collections::BTreeSet<String> = node.contract_roles.iter().cloned().collect();
    for edge in &generation.edges {
        if edge.target != node.id {
            continue;
        }
        match edge.kind.as_str() {
            "PRODUCES" => {
                roles.insert("provider".to_string());
            }
            "CONSUMES" | "USES" => {
                roles.insert("consumer".to_string());
            }
            "ROUTES_TO" => {
                roles.insert("provider".to_string());
            }
            _ => {}
        }
    }
    roles.into_iter().collect()
}

fn contract_from_node(generation: &Generation, node: &ContractNode, repo_id: Option<&str>) -> Option<Contract> {
    let labels: std::collections::HashSet<&str> = node.labels.iter().map(|s| s.as_str()).collect();
    let mut kind: Option<&'static str> = None;
    let mut address: Option<String> = None;

    if labels.contains("HttpRoute") {
        kind = Some("http");
        let method = node.method.clone().unwrap_or_else(|| "ANY".to_string()).to_uppercase();
        let path = node.route_path.clone().or_else(|| node.name.clone()).unwrap_or_default();
        address = Some(format!("{method} {path}"));
    } else if labels.contains("EventTopic") {
        kind = Some("event");
        address = node.domain_identity_address.clone().or_else(|| node.name.clone());
    } else if labels.contains("ToolContract") {
        kind = Some("tool");
        address = node.domain_identity_address.clone().or_else(|| node.name.clone());
    } else if labels.contains("UiRoute") {
        kind = Some("ui_route");
        address = node.domain_identity_address.clone().or_else(|| node.name.clone());
    }

    let (kind, address) = match (kind, address) {
        (Some(k), Some(a)) if !a.is_empty() => (k, a),
        _ => return None,
    };

    let schema = node.contract_schema.clone();
    let key_input = json!({
        "kind": kind,
        "address": address,
        "schema": schema.clone().unwrap_or(Value::Null),
    });
    let contract_key = digest(&key_input);
    let contract_id = format!("contract:{kind}:{}", strip_sha256_prefix(&contract_key));

    Some(Contract {
        contract_id,
        contract_key,
        repo_id: repo_id.map(|s| s.to_string()),
        kind: kind.to_string(),
        address,
        schema,
        roles: roles_for_node(generation, node),
        node_id: node.id.clone(),
        portable_id: node.portable_id.clone(),
        evidence: node.evidence.clone(),
    })
}

pub fn build_contract_registry(generation: &Generation, repo_id: Option<&str>) -> ContractRegistry {
    let mut contracts: Vec<Contract> = generation
        .nodes
        .iter()
        .filter_map(|node| contract_from_node(generation, node, repo_id))
        .collect();
    contracts.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.address.cmp(&right.address))
            .then_with(|| {
                let l = left.repo_id.clone().unwrap_or_default();
                let r = right.repo_id.clone().unwrap_or_default();
                l.cmp(&r)
            })
    });
    ContractRegistry {
        schema_version: 1,
        repo_id: repo_id.map(|s| s.to_string()),
        generation_id: generation.generation_id.clone(),
        contracts,
    }
}

/// Exact contract bridges only. Names that merely resemble one another never join.
pub fn bridge_contract_registries(registries: &[ContractRegistry]) -> BridgeProjection {
    let mut by_key: std::collections::BTreeMap<String, Vec<&Contract>> = std::collections::BTreeMap::new();
    for registry in registries {
        for contract in &registry.contracts {
            by_key.entry(contract.contract_key.clone()).or_default().push(contract);
        }
    }

    let mut bridges = Vec::new();
    for (contract_key, contracts) in &by_key {
        let providers: Vec<&&Contract> = contracts.iter().filter(|c| c.roles.iter().any(|r| r == "provider")).collect();
        let consumers: Vec<&&Contract> = contracts.iter().filter(|c| c.roles.iter().any(|r| r == "consumer")).collect();
        for consumer in &consumers {
            for provider in &providers {
                let (Some(consumer_repo), Some(provider_repo)) = (consumer.repo_id.as_deref(), provider.repo_id.as_deref()) else {
                    continue;
                };
                if consumer_repo == provider_repo {
                    continue;
                }
                let id_input = json!({
                    "contractKey": contract_key,
                    "consumer": consumer_repo,
                    "provider": provider_repo,
                });
                let id_digest = digest(&id_input);
                let mut evidence = Vec::new();
                if let Value::Array(arr) = &consumer.evidence {
                    evidence.extend(arr.iter().cloned());
                }
                if let Value::Array(arr) = &provider.evidence {
                    evidence.extend(arr.iter().cloned());
                }
                let consumer_generation_id = registries.iter().find(|r| r.repo_id.as_deref() == Some(consumer_repo)).and_then(|r| r.generation_id.clone());
                let provider_generation_id = registries.iter().find(|r| r.repo_id.as_deref() == Some(provider_repo)).and_then(|r| r.generation_id.clone());
                bridges.push(Bridge {
                    id: format!("bridge:{}", strip_sha256_prefix(&id_digest)),
                    contract_key: contract_key.clone(),
                    kind: consumer.kind.clone(),
                    address: consumer.address.clone(),
                    consumer_repo_id: consumer.repo_id.clone(),
                    consumer_node_id: consumer.node_id.clone(),
                    consumer_generation_id,
                    provider_repo_id: provider.repo_id.clone(),
                    provider_node_id: provider.node_id.clone(),
                    provider_generation_id,
                    evidence: Value::Array(evidence),
                });
            }
        }
    }
    bridges.sort_by(|a, b| a.id.cmp(&b.id));
    BridgeProjection { schema_version: 1, bridges }
}

pub fn stitch_contract_traces(registries: &[ContractRegistry]) -> TraceProjection {
    let bridge_projection = bridge_contract_registries(registries);
    let traces = bridge_projection
        .bridges
        .iter()
        .map(|bridge| Trace {
            id: format!("trace:{}", bridge.id.strip_prefix("bridge:").unwrap_or(&bridge.id)),
            contract_key: bridge.contract_key.clone(),
            steps: vec![
                TraceStep {
                    repo_id: bridge.consumer_repo_id.clone(),
                    node_id: bridge.consumer_node_id.clone(),
                    role: "consumer",
                    generation_id: bridge.consumer_generation_id.clone(),
                },
                TraceStep {
                    repo_id: bridge.provider_repo_id.clone(),
                    node_id: bridge.provider_node_id.clone(),
                    role: "provider",
                    generation_id: bridge.provider_generation_id.clone(),
                },
            ],
            evidence: bridge.evidence.clone(),
        })
        .collect();
    TraceProjection {
        schema_version: 1,
        bridges: bridge_projection.bridges,
        traces,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(path: &str) -> Value {
        json!([{ "path": path, "startLine": 1, "endLine": 1, "contentHash": format!("h:{path}") }])
    }

    #[test]
    fn extracts_tool_contract_and_bridges_exact_keys() {
        let mut provider = Generation {
            generation_id: Some("g1".to_string()),
            nodes: vec![ContractNode {
                id: "tool:ping".to_string(),
                labels: vec!["ToolContract".to_string()],
                name: Some("ping".to_string()),
                domain_identity_address: Some("ping".to_string()),
                contract_roles: vec!["provider".to_string()],
                evidence: ev("src/api.ts"),
                ..Default::default()
            }],
            edges: vec![],
        };
        let a = build_contract_registry(&mut provider, Some("provider-repo"));
        assert_eq!(a.contracts.len(), 1);
        assert_eq!(a.contracts[0].kind, "tool");
        assert_eq!(a.contracts[0].address, "ping");

        let consumer = Generation {
            generation_id: Some("g2".to_string()),
            nodes: vec![ContractNode {
                id: "tool:ping-consumer".to_string(),
                labels: vec!["ToolContract".to_string()],
                name: Some("ping".to_string()),
                domain_identity_address: Some("ping".to_string()),
                contract_roles: vec!["consumer".to_string()],
                evidence: ev("src/client.ts"),
                ..Default::default()
            }],
            edges: vec![],
        };
        let b = build_contract_registry(&consumer, Some("consumer-repo"));

        let bridges = bridge_contract_registries(&[a.clone(), b.clone()]);
        assert_eq!(bridges.bridges.len(), 1);
        assert_eq!(bridges.bridges[0].address, "ping");

        let traces = stitch_contract_traces(&[a, b]);
        assert_eq!(traces.traces.len(), 1);
        let roles: Vec<&str> = traces.traces[0].steps.iter().map(|s| s.role).collect();
        assert_eq!(roles, vec!["consumer", "provider"]);
    }

    #[test]
    fn same_repo_never_bridges() {
        let node_provider = ContractNode {
            id: "tool:ping".to_string(),
            labels: vec!["ToolContract".to_string()],
            domain_identity_address: Some("ping".to_string()),
            contract_roles: vec!["provider".to_string()],
            evidence: json!([]),
            ..Default::default()
        };
        let node_consumer = ContractNode {
            id: "tool:ping2".to_string(),
            labels: vec!["ToolContract".to_string()],
            domain_identity_address: Some("ping".to_string()),
            contract_roles: vec!["consumer".to_string()],
            evidence: json!([]),
            ..Default::default()
        };
        let generation = Generation {
            generation_id: Some("g1".to_string()),
            nodes: vec![node_provider, node_consumer],
            edges: vec![],
        };
        let registry = build_contract_registry(&generation, Some("same-repo"));
        let bridges = bridge_contract_registries(&[registry]);
        assert!(bridges.bridges.is_empty());
    }
}
