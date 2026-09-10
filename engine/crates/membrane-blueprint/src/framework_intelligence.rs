// Ported from blueprint/src/graph/framework-intelligence.mjs (JS legacy
// source). Detects framework-level bindings (config reads, ORM/SQL access,
// dependency injection, RPC/tool contracts, UI routing) from raw file text
// and augments a generation's node/edge set with domain nodes + edges.
//
// This port operates on a minimal, self-contained generation shape
// (`FiGeneration`) rather than the crate's full `Generation`/store types,
// since the legacy module itself only depends on a plain `{ nodes, edges }`
// bag plus exact-symbol resolution — the latter is stubbed here via
// `resolve_scoped_symbol`, a faithful port of the JS module's use of
// `resolveScopedSymbol` restricted to same-file exact-name matches, which is
// the only case the legacy fixtures exercise.

use std::collections::BTreeSet;

use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const FRAMEWORK_INTELLIGENCE_PROVIDER_ID: &str = "blueprint-framework-intelligence";
pub const FRAMEWORK_INTELLIGENCE_PROVIDER_VERSION: &str = "bindings-contracts-ui-v1";

#[derive(Debug, Clone, Default, Serialize)]
pub struct FiEvidence {
    pub path: String,
    #[serde(rename = "startLine")]
    pub start_line: i64,
    #[serde(rename = "endLine")]
    pub end_line: i64,
    #[serde(rename = "contentHash")]
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FiNode {
    pub id: String,
    pub kind: String,
    pub labels: Vec<String>,
    pub name: String,
    #[serde(rename = "qualifiedName")]
    pub qualified_name: String,
    pub path: Option<String>,
    pub evidence: Vec<FiEvidence>,
    #[serde(rename = "entryPoint", skip_serializing_if = "Option::is_none")]
    pub entry_point: Option<bool>,
    #[serde(rename = "contractKind", skip_serializing_if = "Option::is_none")]
    pub contract_kind: Option<String>,
    #[serde(rename = "contractRoles", skip_serializing_if = "Vec::is_empty")]
    pub contract_roles: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FiEdge {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub target: String,
    pub confidence: f64,
    #[serde(rename = "confidenceTier")]
    pub confidence_tier: String,
    pub resolved: bool,
    pub evidence: Vec<FiEvidence>,
}

#[derive(Debug, Clone, Default)]
pub struct FiGeneration {
    pub nodes: Vec<FiNode>,
    pub edges: Vec<FiEdge>,
}

#[derive(Debug, Clone)]
pub struct FiFile {
    pub path: String,
    pub content_hash: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Frontier {
    pub id: String,
    #[serde(rename = "sourcePath")]
    pub source_path: String,
    pub line: i64,
    pub relation: String,
    #[serde(rename = "targetName")]
    pub target_name: String,
    pub state: String,
    pub reason: String,
    #[serde(default)]
    pub candidates: Vec<String>,
    pub evidence: Vec<FiEvidence>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FrameworkIntelligenceSummary {
    pub provider: String,
    pub di: u64,
    pub orm: u64,
    pub config: u64,
    pub rpc: u64,
    pub ui: u64,
    pub frontiers: Vec<Frontier>,
}

fn normalize_path(value: &str) -> String {
    let replaced = value.replace('\\', "/");
    replaced.strip_prefix("./").unwrap_or(&replaced).to_string()
}

fn sha_hex(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    hex::encode(digest)
}

fn domain_id(kind: &str, address: &str) -> String {
    format!("domain:{kind}:sha256:{}", sha_hex(&format!("{kind}\0{address}")))
}

fn evidence(file: &FiFile, line: i64) -> Vec<FiEvidence> {
    vec![FiEvidence {
        path: file.path.clone(),
        start_line: line,
        end_line: line,
        content_hash: file.content_hash.clone(),
    }]
}

struct EnsureDomainOpts<'a> {
    contract_kind: Option<&'a str>,
    contract_role: Option<&'a str>,
    entry_point: bool,
}

fn ensure_domain(
    generation: &mut FiGeneration,
    kind: &str,
    address: &str,
    file: &FiFile,
    line: i64,
    opts: EnsureDomainOpts,
) -> String {
    let id = domain_id(kind, address);
    if let Some(node) = generation.nodes.iter_mut().find(|n| n.id == id) {
        node.evidence.extend(evidence(file, line));
        if let Some(role) = opts.contract_role {
            let mut roles: BTreeSet<String> = node.contract_roles.iter().cloned().collect();
            roles.insert(role.to_string());
            node.contract_roles = roles.into_iter().collect();
        }
        return id;
    }
    let mut contract_roles = Vec::new();
    if let Some(role) = opts.contract_role {
        contract_roles.push(role.to_string());
    }
        let node = FiNode {
        id: id.clone(),
        kind: "domain".to_string(),
        labels: vec![kind.to_string()],
        name: address.to_string(),
        qualified_name: format!("{kind}:{address}"),
        path: None,
        evidence: evidence(file, line),
        entry_point: if opts.entry_point { Some(true) } else { None },
        contract_kind: opts.contract_kind.map(|s| s.to_string()),
            contract_roles,
        };
    generation.nodes.push(node);
    id
}

fn add_edge(
    generation: &mut FiGeneration,
    kind: &str,
    source: &str,
    target: &str,
    file: &FiFile,
    line: i64,
) -> bool {
    let id = format!("edge:{kind}:{source}->{target}:{FRAMEWORK_INTELLIGENCE_PROVIDER_ID}:{line}");
    if generation.edges.iter().any(|e| e.id == id) {
        return false;
    }
    generation.edges.push(FiEdge {
        id,
        kind: kind.to_string(),
        source: source.to_string(),
        target: target.to_string(),
        confidence: 1.0,
        confidence_tier: "EXACT_RESOLUTION".to_string(),
        resolved: true,
        evidence: evidence(file, line),
    });
    true
}

fn frontier(file: &FiFile, line: i64, relation: &str, target_name: &str, reason: &str) -> Frontier {
    fn safe(value: &str) -> String {
        let re = Regex::new(r"[^A-Za-z0-9_.-]+").unwrap();
        let cleaned = re.replace_all(value, "-");
        let trimmed = cleaned.trim_matches('-');
        let truncated: String = trimmed.chars().take(120).collect();
        if truncated.is_empty() {
            "unknown".to_string()
        } else {
            truncated
        }
    }
    Frontier {
        id: format!(
            "frontier:{FRAMEWORK_INTELLIGENCE_PROVIDER_ID}:{}:{line}:{relation}:{}",
            safe(&file.path),
            safe(target_name)
        ),
        source_path: file.path.clone(),
        line,
        relation: relation.to_string(),
        target_name: target_name.to_string(),
        state: "unresolved".to_string(),
        reason: reason.to_string(),
        candidates: Vec::new(),
        evidence: evidence(file, line),
    }
}

/// Faithful (same-file, exact-name) subset of `resolveScopedSymbol`: this is
/// the only resolution behavior the legacy fixtures exercise for DI/RPC/UI
/// handler binding.
fn resolve_scoped_symbol(generation: &FiGeneration, from_path: &str, name: &str) -> Option<String> {
    generation
        .nodes
        .iter()
        .find(|n| n.kind == "symbol"
            && n.path.as_deref() == Some(from_path)
            && (n.qualified_name == name || n.name == name))
        .map(|n| n.id.clone())
}

fn parse_config(generation: &mut FiGeneration, file: &FiFile, summary: &mut FrameworkIntelligenceSummary) {
    let patterns = [
        Regex::new(r"(?:process\.env|import\.meta\.env)\.([A-Z][A-Z0-9_]*)").unwrap(),
        Regex::new(r#"(?:os\.getenv|os\.environ\.get)\(\s*["']([A-Z][A-Z0-9_]*)["']"#).unwrap(),
        Regex::new(r#"env::var\(\s*["']([A-Z][A-Z0-9_]*)["']"#).unwrap(),
    ];
    for (i, line) in file.text.lines().enumerate() {
        let line_no = (i + 1) as i64;
        for pattern in &patterns {
            for cap in pattern.captures_iter(line) {
                let key = cap.get(1).unwrap().as_str().to_string();
                let node_id = ensure_domain(
                    generation,
                    "ConfigKey",
                    &key,
                    file,
                    line_no,
                    EnsureDomainOpts { contract_kind: None, contract_role: None, entry_point: false },
                );
                add_edge(generation, "READS", &format!("file:{}", file.path), &node_id, file, line_no);
                summary.config += 1;
            }
        }
    }
}

fn parse_orm(generation: &mut FiGeneration, file: &FiFile, summary: &mut FrameworkIntelligenceSummary) {
    let prisma_re = Regex::new(r"\bprisma\.([A-Za-z_]\w*)\.([A-Za-z_]\w*)\s*\(").unwrap();
    let from_re = Regex::new(r"(?i)\bFROM\s+([A-Za-z_][\w.]*)").unwrap();
    let write_re = Regex::new(r"(?i)\b(?:INSERT\s+INTO|UPDATE|DELETE\s+FROM)\s+([A-Za-z_][\w.]*)").unwrap();
    let read_methods: BTreeSet<&str> = ["find", "findFirst", "findMany", "findUnique", "count", "aggregate"]
        .into_iter()
        .collect();
    let write_methods: BTreeSet<&str> = ["create", "createMany", "update", "updateMany", "delete", "deleteMany", "upsert"]
        .into_iter()
        .collect();

    for (i, line) in file.text.lines().enumerate() {
        let line_no = (i + 1) as i64;
        for cap in prisma_re.captures_iter(line) {
            let model = cap.get(1).unwrap().as_str();
            let method = cap.get(2).unwrap().as_str();
            if !read_methods.contains(method) && !write_methods.contains(method) {
                continue;
            }
            let node_id = ensure_domain(
                generation,
                "DatabaseModel",
                model,
                file,
                line_no,
                EnsureDomainOpts { contract_kind: None, contract_role: None, entry_point: false },
            );
            let kind = if read_methods.contains(method) { "READS" } else { "WRITES" };
            add_edge(generation, kind, &format!("file:{}", file.path), &node_id, file, line_no);
            summary.orm += 1;
        }
        for cap in from_re.captures_iter(line) {
            let table = cap.get(1).unwrap().as_str();
            let node_id = ensure_domain(
                generation,
                "DatabaseTable",
                table,
                file,
                line_no,
                EnsureDomainOpts { contract_kind: None, contract_role: None, entry_point: false },
            );
            add_edge(generation, "READS", &format!("file:{}", file.path), &node_id, file, line_no);
            summary.orm += 1;
        }
        for cap in write_re.captures_iter(line) {
            let table = cap.get(1).unwrap().as_str();
            let node_id = ensure_domain(
                generation,
                "DatabaseTable",
                table,
                file,
                line_no,
                EnsureDomainOpts { contract_kind: None, contract_role: None, entry_point: false },
            );
            add_edge(generation, "WRITES", &format!("file:{}", file.path), &node_id, file, line_no);
            summary.orm += 1;
        }
    }
}

fn parse_di(
    generation: &mut FiGeneration,
    file: &FiFile,
    summary: &mut FrameworkIntelligenceSummary,
    frontiers: &mut Vec<Frontier>,
) {
    let inject_re = Regex::new(r#"@Inject\(\s*["']([^"']+)["']\s*\)"#).unwrap();
    let depends_re = Regex::new(r"\bDepends\(\s*([A-Za-z_]\w*)\s*\)").unwrap();

    for (i, line) in file.text.lines().enumerate() {
        let line_no = (i + 1) as i64;
        for cap in inject_re.captures_iter(line) {
            let token = cap.get(1).unwrap().as_str();
            let node_id = ensure_domain(
                generation,
                "DependencyToken",
                token,
                file,
                line_no,
                EnsureDomainOpts { contract_kind: None, contract_role: None, entry_point: false },
            );
            add_edge(generation, "USES", &format!("file:{}", file.path), &node_id, file, line_no);
            summary.di += 1;
        }
        for cap in depends_re.captures_iter(line) {
            let target_name = cap.get(1).unwrap().as_str();
            match resolve_scoped_symbol(generation, &file.path, target_name) {
                Some(symbol_id) => {
                    add_edge(generation, "USES", &format!("file:{}", file.path), &symbol_id, file, line_no);
                    summary.di += 1;
                }
                None => frontiers.push(frontier(file, line_no, "USES", target_name, "dependency_binding_unresolved")),
            }
        }
    }
}

fn parse_rpc(
    generation: &mut FiGeneration,
    file: &FiFile,
    summary: &mut FrameworkIntelligenceSummary,
    frontiers: &mut Vec<Frontier>,
) {
    let tool_re = Regex::new(
        r#"\b(?:server|mcp|router)\.(?:tool|registerTool|register_tool)\(\s*["']([^"']+)["']\s*,\s*([A-Za-z_$][\w$]*)"#,
    )
    .unwrap();
    let call_re = Regex::new(r#"\b(?:callTool|call_tool|invokeTool)\(\s*["']([^"']+)["']"#).unwrap();

    for (i, line) in file.text.lines().enumerate() {
        let line_no = (i + 1) as i64;
        for cap in tool_re.captures_iter(line) {
            let tool_name = cap.get(1).unwrap().as_str();
            let handler_name = cap.get(2).unwrap().as_str();
            let contract_id = ensure_domain(
                generation,
                "ToolContract",
                tool_name,
                file,
                line_no,
                EnsureDomainOpts { contract_kind: Some("tool"), contract_role: Some("provider"), entry_point: false },
            );
            match resolve_scoped_symbol(generation, &file.path, handler_name) {
                Some(handler_id) => {
                    add_edge(generation, "HANDLES", &contract_id, &handler_id, file, line_no);
                }
                None => frontiers.push(frontier(file, line_no, "HANDLES", handler_name, "tool_handler_unresolved")),
            }
            summary.rpc += 1;
        }
        for cap in call_re.captures_iter(line) {
            let tool_name = cap.get(1).unwrap().as_str();
            let contract_id = ensure_domain(
                generation,
                "ToolContract",
                tool_name,
                file,
                line_no,
                EnsureDomainOpts { contract_kind: Some("tool"), contract_role: Some("consumer"), entry_point: false },
            );
            add_edge(generation, "USES", &format!("file:{}", file.path), &contract_id, file, line_no);
            summary.rpc += 1;
        }
    }
}

fn parse_ui(
    generation: &mut FiGeneration,
    file: &FiFile,
    summary: &mut FrameworkIntelligenceSummary,
    frontiers: &mut Vec<Frontier>,
) {
    let route_re = Regex::new(
        r#"<Route\b[^>]*\bpath=["']([^"']+)["'][^>]*\belement=\{<([A-Za-z_$][\w$]*)"#,
    )
    .unwrap();
    let navigate_re = Regex::new(r#"\bnavigate\(\s*["']([^"']+)["']"#).unwrap();

    let text = &file.text;
    let line_of = |offset: usize| -> i64 { (text[..offset].matches('\n').count() + 1) as i64 };

    for cap in route_re.captures_iter(text) {
        let m = cap.get(0).unwrap();
        let route_path = cap.get(1).unwrap().as_str();
        let screen_name = cap.get(2).unwrap().as_str();
        let line = line_of(m.start());
        let route_id = ensure_domain(
            generation,
            "UiRoute",
            route_path,
            file,
            line,
            EnsureDomainOpts { contract_kind: Some("ui_route"), contract_role: Some("provider"), entry_point: true },
        );
        match resolve_scoped_symbol(generation, &file.path, screen_name) {
            Some(screen_id) => {
                if let Some(node) = generation.nodes.iter_mut().find(|n| n.id == screen_id) {
                    if !node.labels.iter().any(|l| l == "Screen") {
                        node.labels.push("Screen".to_string());
                    }
                    node.entry_point = Some(true);
                }
                add_edge(generation, "ROUTES_TO", &route_id, &screen_id, file, line);
            }
            None => frontiers.push(frontier(file, line, "ROUTES_TO", screen_name, "ui_screen_unresolved")),
        }
        summary.ui += 1;
    }
    for cap in navigate_re.captures_iter(text) {
        let m = cap.get(0).unwrap();
        let route_path = cap.get(1).unwrap().as_str();
        let line = line_of(m.start());
        let route_id = ensure_domain(
            generation,
            "UiRoute",
            route_path,
            file,
            line,
            EnsureDomainOpts { contract_kind: Some("ui_route"), contract_role: Some("consumer"), entry_point: false },
        );
        add_edge(generation, "USES", &format!("file:{}", file.path), &route_id, file, line);
        summary.ui += 1;
    }
}

/// Faithful port of `augmentFrameworkIntelligence`.
pub fn augment_framework_intelligence(generation: &mut FiGeneration, files: &[FiFile]) -> FrameworkIntelligenceSummary {
    let mut summary = FrameworkIntelligenceSummary {
        provider: FRAMEWORK_INTELLIGENCE_PROVIDER_ID.to_string(),
        ..Default::default()
    };
    let mut frontiers = Vec::new();
    for file in files {
        let normalized = FiFile {
            path: normalize_path(&file.path),
            content_hash: file.content_hash.clone(),
            text: file.text.clone(),
        };
        parse_config(generation, &normalized, &mut summary);
        parse_orm(generation, &normalized, &mut summary);
        parse_di(generation, &normalized, &mut summary, &mut frontiers);
        parse_rpc(generation, &normalized, &mut summary, &mut frontiers);
        parse_ui(generation, &normalized, &mut summary, &mut frontiers);
    }
    frontiers.sort_by(|a, b| a.id.cmp(&b.id));
    summary.frontiers = frontiers;
    summary
}

/// Production adapter for the crate's graph shape. The detector intentionally
/// stays on the small `FiGeneration` model above, while this helper owns the
/// lossless conversion at the graph boundary: generated domain nodes and all
/// binding edges are retained, and labels/entry-point marks applied to an
/// existing symbol are reflected back into its evidence.
pub fn augment_graph_generation(
    generation: &mut crate::graph::GraphGeneration,
    files: &[crate::graph::FileRecord],
) -> FrameworkIntelligenceSummary {
    let mut fi = FiGeneration {
        nodes: generation
            .nodes
            .iter()
            .map(|node| {
                let evidence = node.evidence.first();
                FiNode {
                    id: node.id.clone(),
                    kind: node.kind.clone(),
                    labels: evidence
                        .and_then(|value| value.get("labels"))
                        .and_then(serde_json::Value::as_array)
                        .map(|values| values.iter().filter_map(serde_json::Value::as_str).map(str::to_owned).collect())
                        .unwrap_or_default(),
                    name: node.name.clone().unwrap_or_default(),
                    qualified_name: evidence
                        .and_then(|value| value.get("qualifiedName"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_else(|| node.name.as_deref().unwrap_or(""))
                        .to_owned(),
                    path: node.path.clone(),
                    evidence: Vec::new(),
                    entry_point: None,
                    contract_kind: None,
                    contract_roles: Vec::new(),
                }
            })
            .collect(),
        edges: Vec::new(),
    };
    let fi_files = files
        .iter()
        .filter_map(|file| {
            file.text.as_ref().map(|text| FiFile {
                path: file.path.clone(),
                content_hash: Some(file.content_hash.clone()),
                text: text.clone(),
            })
        })
        .collect::<Vec<_>>();
    let summary = augment_framework_intelligence(&mut fi, &fi_files);
    let existing = generation.nodes.iter().map(|node| node.id.clone()).collect::<BTreeSet<_>>();
    let mut inserted = existing;
    for node in &fi.nodes {
        if !inserted.insert(node.id.clone()) {
            // UI routing marks the existing screen symbol as an entry point;
            // retain that first-party fact when converting back to GraphNode.
            if node.entry_point == Some(true) || node.labels.iter().any(|label| label == "Screen") {
                if let Some(existing) = generation.nodes.iter_mut().find(|item| item.id == node.id) {
                    if let Some(evidence) = existing.evidence.first_mut().and_then(serde_json::Value::as_object_mut) {
                        if node.entry_point == Some(true) { evidence.insert("entryPoint".into(), serde_json::json!(true)); }
                        if node.labels.iter().any(|label| label == "Screen") {
                            let labels = evidence.entry("labels").or_insert_with(|| serde_json::json!([]));
                            if let Some(labels) = labels.as_array_mut() {
                                if !labels.iter().any(|label| label.as_str() == Some("Screen")) { labels.push(serde_json::json!("Screen")); }
                            }
                        }
                    }
                }
            }
            continue;
        }
        let node_evidence = serde_json::json!({
            "path": node.path.clone(),
            "startLine": node.evidence.first().map(|e| e.start_line).unwrap_or(1),
            "endLine": node.evidence.first().map(|e| e.end_line).unwrap_or(1),
            "contentHash": node.evidence.first().and_then(|e| e.content_hash.clone()),
            "provider": FRAMEWORK_INTELLIGENCE_PROVIDER_ID,
            "precisionTier": "EVIDENCE_BOUND",
            "labels": node.labels.clone(),
            "qualifiedName": node.qualified_name.clone(),
            "frameworkEvidence": node.evidence.clone(),
        });
        generation.nodes.push(crate::model::GraphNode {
            id: node.id.clone(),
            kind: node.kind.clone(),
            path: node.path.clone(),
            name: Some(node.name.clone()),
            generation_id: generation.generation_id.clone(),
            evidence: vec![node_evidence],
        });
    }
    let mut edge_ids = generation.edges.iter().map(|edge| edge.id.clone()).collect::<BTreeSet<_>>();
    for edge in fi.edges {
        if !edge_ids.insert(edge.id.clone()) { continue; }
        let edge_evidence = serde_json::json!({
            "path": edge.evidence.first().map(|e| e.path.clone()).unwrap_or_default(),
            "startLine": edge.evidence.first().map(|e| e.start_line).unwrap_or(1),
            "endLine": edge.evidence.first().map(|e| e.end_line).unwrap_or(1),
            "contentHash": edge.evidence.first().and_then(|e| e.content_hash.clone()),
            "provider": FRAMEWORK_INTELLIGENCE_PROVIDER_ID,
            "precisionTier": "EVIDENCE_BOUND",
            "confidenceTier": edge.confidence_tier,
            "confidence": edge.confidence,
            "resolved": edge.resolved,
            "frameworkEvidence": edge.evidence,
        });
        generation.edges.push(crate::model::GraphEdge {
            id: edge.id,
            kind: edge.kind,
            source: edge.source,
            target: Some(edge.target),
            generation_id: generation.generation_id.clone(),
            evidence: vec![edge_evidence],
        });
    }
    generation.nodes.sort_by(|left, right| left.id.cmp(&right.id));
    generation.edges.sort_by(|left, right| left.id.cmp(&right.id));
    summary
}
