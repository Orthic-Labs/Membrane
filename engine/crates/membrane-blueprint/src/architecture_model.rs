// D40: architecture/layer model — components, layers, coupling, and hotspots
// derived from the graph, reproducible and generation-bound.
//
// Ported from blueprint/src/graph/architecture-model.mjs (JS legacy source).
// `assignLayers` is ported inline from blueprint/src/graph/analytics/index.mjs
// since buildArchitectureModel depends on it and no native equivalent exists
// yet in this crate.

use std::collections::BTreeMap;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub path: Option<String>,
    #[serde(rename = "startLine")]
    pub start_line: Option<i64>,
    #[serde(rename = "endLine")]
    pub end_line: Option<i64>,
    #[serde(rename = "contentHash")]
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArchNode {
    pub id: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArchEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub kind: Option<String>,
    /// Provider-owned edge evidence must survive the disposable projection;
    /// the earlier native adapter dropped it because `ArchEdge` only modeled
    /// topology.
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerAssignment {
    pub id: String,
    pub layer: i64,
}

/// Faithful port of `assignLayers` from analytics/index.mjs: longest-path
/// depth from any root (node with no incoming edges), computed via recursive
/// visit exactly as the JS does (roots visited in id-sorted order).
pub fn assign_layers(nodes: &[ArchNode], edges: &[ArchEdge]) -> Vec<LayerAssignment> {
    let mut incoming: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in nodes {
        incoming.entry(node.id.as_str()).or_default();
    }
    for edge in edges {
        if let Some(sources) = incoming.get_mut(edge.target.as_str()) {
            sources.push(edge.source.as_str());
        }
    }

    let mut layers: HashMap<String, i64> = HashMap::new();

    fn visit<'a>(
        node_id: &'a str,
        depth: i64,
        incoming: &HashMap<&'a str, Vec<&'a str>>,
        layers: &mut HashMap<String, i64>,
    ) {
        let current = *layers.get(node_id).unwrap_or(&0);
        layers.insert(node_id.to_string(), current.max(depth));
        // Iterate targets in a stable order (by target id) mirroring the
        // JS Map iteration order (insertion order == `incoming` build order,
        // which followed `nodes` order — we approximate with sorted keys for
        // determinism, matching final output tie-breaking below).
        let mut targets: Vec<&&str> = incoming.keys().collect();
        targets.sort();
        for target in targets {
            let sources = &incoming[target];
            if sources.contains(&node_id) {
                visit(target, depth + 1, incoming, layers);
            }
        }
    }

    let mut sorted_nodes: Vec<&ArchNode> = nodes.iter().collect();
    sorted_nodes.sort_by(|a, b| a.id.cmp(&b.id));
    for node in sorted_nodes {
        if incoming.get(node.id.as_str()).map(|v| v.len()).unwrap_or(0) == 0 {
            visit(node.id.as_str(), 0, &incoming, &mut layers);
        }
    }

    nodes
        .iter()
        .map(|node| LayerAssignment {
            id: node.id.clone(),
            layer: *layers.get(&node.id).unwrap_or(&0),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct AlgorithmInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerGroup {
    pub layer: i64,
    #[serde(rename = "nodeIds")]
    pub node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CouplingEntry {
    pub id: String,
    pub degree: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hotspot {
    pub id: String,
    pub degree: i64,
    pub candidate: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchitectureModel {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "generationId")]
    pub generation_id: Option<String>,
    pub algorithm: AlgorithmInfo,
    pub layers: Vec<LayerGroup>,
    pub coupling: Vec<CouplingEntry>,
    pub hotspots: Vec<Hotspot>,
    #[serde(rename = "nodeCount")]
    pub node_count: usize,
    #[serde(rename = "edgeCount")]
    pub edge_count: usize,
}

/// Faithful port of `buildArchitectureModel`.
pub fn build_architecture_model(
    nodes: &[ArchNode],
    edges: &[ArchEdge],
    generation_id: Option<String>,
) -> ArchitectureModel {
    let layers = assign_layers(nodes, edges);
    let mut by_layer: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    for entry in &layers {
        by_layer.entry(entry.layer).or_default().push(entry.id.clone());
    }

    let mut coupling: BTreeMap<String, i64> = BTreeMap::new();
    // Preserve JS insertion order for coupling.entries() output: since JS
    // uses a Map keyed by edge.source in edges-array order, but the final
    // `coupling` output is re-sorted by degree desc, we can compute directly.
    for edge in edges {
        *coupling.entry(edge.source.clone()).or_insert(0) += 1;
    }

    let mut hotspot_pairs: Vec<(String, i64)> = coupling.iter().map(|(k, v)| (k.clone(), *v)).collect();
    hotspot_pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let hotspots: Vec<Hotspot> = hotspot_pairs
        .into_iter()
        .take(20)
        .map(|(id, count)| Hotspot {
            id,
            degree: count,
            candidate: true,
        })
        .collect();

    let mut coupling_entries: Vec<CouplingEntry> = coupling
        .into_iter()
        .map(|(id, degree)| CouplingEntry { id, degree })
        .collect();
    coupling_entries.sort_by(|a, b| b.degree.cmp(&a.degree));

    let layer_groups: Vec<LayerGroup> = by_layer
        .into_iter()
        .map(|(layer, node_ids)| LayerGroup { layer, node_ids })
        .collect();

    ArchitectureModel {
        schema_version: 1,
        generation_id,
        algorithm: AlgorithmInfo {
            name: "layer-clustering".to_string(),
            version: "1.0.0".to_string(),
        },
        layers: layer_groups,
        coupling: coupling_entries,
        hotspots,
        node_count: nodes.len(),
        edge_count: edges.len(),
    }
}

fn normalize_slashes(path: &str) -> String {
    path.replace('\\', "/")
}

fn component_key(node: &ArchNode) -> String {
    let path = node
        .path
        .clone()
        .or_else(|| node.evidence.first().and_then(|e| e.path.clone()))
        .unwrap_or_default();
    let path = normalize_slashes(&path);
    if path.is_empty() {
        return "unlocated".to_string();
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > 1 {
        let take = std::cmp::min(2, parts.len() - 1);
        parts[..take].join("/")
    } else {
        ".".to_string()
    }
}

fn evidence(value: &ArchNode) -> Vec<EvidenceRef> {
    value
        .evidence
        .iter()
        .map(|item| EvidenceRef {
            path: item.path.clone().or_else(|| value.path.clone()),
            start_line: item.start_line,
            end_line: item.end_line,
            content_hash: item.content_hash.clone(),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub id: String,
    pub label: String,
    #[serde(rename = "nodeIds")]
    pub node_ids: Vec<String>,
    pub citations: Vec<EvidenceRef>,
    pub inferred: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Flow {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(rename = "edgeId")]
    pub edge_id: String,
    pub kind: Option<String>,
    pub citations: Vec<EvidenceRef>,
    pub inferred: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Omission {
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DisposableArchitectureProjection {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub kind: String,
    pub id: String,
    #[serde(rename = "generationId")]
    pub generation_id: Option<String>,
    pub authority: String,
    #[serde(rename = "plannerAuthority")]
    pub planner_authority: String,
    pub components: Vec<Component>,
    pub flows: Vec<Flow>,
    pub omissions: Vec<Omission>,
}

/// Faithful port of `buildDisposableArchitectureProjection`.
pub fn build_disposable_architecture_projection(
    nodes: &[ArchNode],
    edges: &[ArchEdge],
    generation_id: Option<String>,
    max_components: usize,
    max_flows: usize,
) -> DisposableArchitectureProjection {
    let node_by_id: HashMap<&str, &ArchNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let mut groups: BTreeMap<String, Vec<&ArchNode>> = BTreeMap::new();
    // BTreeMap keeps insertion order irrelevant since final output sorts by
    // key anyway (matching JS `.sort(([left],[right]) => left.localeCompare(right))`).
    for node in nodes {
        groups.entry(component_key(node)).or_default().push(node);
    }

    let groups_len = groups.len();
    let components: Vec<Component> = groups
        .into_iter()
        .take(max_components)
        .map(|(id, members)| {
            let mut node_ids: Vec<String> = members.iter().map(|n| n.id.clone()).collect();
            node_ids.sort();
            let mut citations: Vec<EvidenceRef> = members.iter().flat_map(|n| evidence(n)).collect();
            citations.sort_by(|a, b| {
                a.path
                    .clone()
                    .unwrap_or_else(|| "null".to_string())
                    .cmp(&b.path.clone().unwrap_or_else(|| "null".to_string()))
            });
            citations.truncate(50);
            Component {
                id: format!("component:{id}"),
                label: id,
                node_ids,
                citations,
                inferred: true,
            }
        })
        .collect();

    let mut component_for_node: HashMap<String, String> = HashMap::new();
    for component in &components {
        for node_id in &component.node_ids {
            component_for_node.insert(node_id.clone(), component.id.clone());
        }
    }

    let mut sorted_edges: Vec<&ArchEdge> = edges.iter().collect();
    sorted_edges.sort_by(|a, b| a.id.cmp(&b.id));

    let mut flows: Vec<Flow> = Vec::new();
    for edge in sorted_edges {
        let from = component_for_node.get(&edge.source).cloned();
        let to = component_for_node.get(&edge.target).cloned();
        let (from, to) = match (from, to) {
            (Some(f), Some(t)) if f != t => (f, t),
            _ => continue,
        };
        let mut citations: Vec<EvidenceRef> = Vec::new();
        citations.extend(edge.evidence.clone());
        if let Some(n) = node_by_id.get(edge.source.as_str()) {
            citations.extend(evidence(n));
        }
        if let Some(n) = node_by_id.get(edge.target.as_str()) {
            citations.extend(evidence(n));
        }
        flows.push(Flow {
            id: format!("flow:{}", edge.id),
            from,
            to,
            edge_id: edge.id.clone(),
            kind: edge.kind.clone(),
            citations,
            inferred: true,
        });
        if flows.len() >= max_flows {
            break;
        }
    }

    let mut omissions = Vec::new();
    if groups_len > components.len() {
        omissions.push(Omission {
            reason: "component_ceiling".to_string(),
            count: Some(groups_len - components.len()),
        });
    }
    if flows.len() >= max_flows {
        omissions.push(Omission {
            reason: "flow_ceiling".to_string(),
            count: None,
        });
    }

    let visible = serde_json::json!({
        "generationId": generation_id,
        "components": components.iter().map(|c| serde_json::to_value(c).unwrap()).collect::<Vec<_>>(),
        "flows": flows.iter().map(|f| serde_json::to_value(f).unwrap()).collect::<Vec<_>>(),
    });
    let digest = Sha256::digest(serde_json::to_string(&visible).unwrap_or_default().as_bytes());
    let id = format!("sha256:{}", hex::encode(digest));

    DisposableArchitectureProjection {
        schema_version: 1,
        kind: "DisposableArchitectureProjection".to_string(),
        id,
        generation_id,
        authority: "disposable_cited_view".to_string(),
        planner_authority: "none".to_string(),
        components,
        flows,
        omissions,
    }
}
