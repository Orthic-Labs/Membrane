//! Native Rust port of the legacy cross-stack fact providers:
//! `blueprint/src/providers/frameworks/index.mjs` (D34: queue/event
//! producer-topic-consumer, DB/ORM read/write, and deployment facts) and
//! `blueprint/src/providers/frameworks/http/index.mjs` (D33: import-gated
//! HTTP route/handler extraction for JS/TS, Python, and Rust stacks).
//!
//! Ported field-for-field, regex-for-regex, from the legacy JavaScript so
//! behavior (including gate semantics and comment-stripping) matches
//! exactly. `framework_intelligence.rs` (lane GC11) ports a *different*
//! legacy module (`graph/framework-intelligence.mjs`, build-time DI/ORM/
//! config/RPC/UI augmentation of an existing generation) and is reused
//! as-is — this module does not duplicate it.

use regex::{Regex, RegexBuilder};
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use crate::model::{GraphEdge, GraphNode};
use super::{ProviderContext, ProviderOutput};

/// Cross-stack edge vocabulary, ported verbatim from `CROSS_STACK_EDGES`.
pub const CROSS_STACK_EDGES: [&str; 7] =
    ["PRODUCES", "CONSUMES", "GENERATES", "READS", "WRITES", "CONFIGURES", "DEPLOYS"];

/// Import-gate package lists, ported verbatim from `DOMAIN_GATES`.
pub fn domain_gate_packages(domain: &str) -> &'static [&'static str] {
    match domain {
        "event" => &["kafkajs", "kafka", "nats", "amqplib", "rabbitmq", "bullmq"],
        "database" => &[
            "prisma",
            "@prisma/client",
            "typeorm",
            "sequelize",
            "sqlalchemy",
            "django.db",
            "diesel",
            "sqlx",
        ],
        _ => &[],
    }
}

/// Framework stack import gates, ported verbatim from `FRAMEWORK_GATES`.
pub fn framework_gate_packages(stack: &str) -> &'static [&'static str] {
    match stack {
        "next-express" => &["next", "express", "fastify"],
        "fastapi-django" => &["fastapi", "django"],
        "tauri-axum" => &["tauri", "axum"],
        _ => &[],
    }
}

fn import_from_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?:from\s+|require\s*\(\s*)["']([^"']+)["']"#).unwrap())
}

fn import_stmt_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r"^\s*(?:from|import)\s+([A-Za-z_][\w.]*)")
            .multi_line(true)
            .build()
            .unwrap()
    })
}

/// Port of `importedPackages(text)`: extracts and lowercases import/require
/// specifiers, deduplicated in first-seen order.
pub fn imported_packages(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    let mut push = |raw: &str| {
        let lower = raw.to_lowercase();
        if seen.insert(lower.clone()) {
            ordered.push(lower);
        }
    };
    for cap in import_from_re().captures_iter(text) {
        push(&cap[1]);
    }
    for cap in import_stmt_re().captures_iter(text) {
        push(&cap[1]);
    }
    ordered
}

/// Port of `domainGateActive(imports, domain)`.
pub fn domain_gate_active(imports: &[String], domain: &str) -> bool {
    let gates = domain_gate_packages(domain);
    gates.iter().any(|gate| {
        imports.iter().any(|item| {
            item == gate
                || item.starts_with(&format!("{gate}/"))
                || item.starts_with(&format!("{gate}."))
        })
    })
}

/// Port of `frameworkGateActive(imports, stack)`.
pub fn framework_gate_active(imports: &[String], stack: &str) -> bool {
    let gates = framework_gate_packages(stack);
    gates
        .iter()
        .any(|gate| imports.iter().any(|imp| imp.contains(gate)))
}

/// A single cross-stack fact, shaped to cover event/database/deployment
/// facts uniformly (legacy emits differently-shaped objects per kind; the
/// union here keeps every field the legacy tests observe).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFact {
    pub kind: String,
    pub topic: Option<String>,
    pub name: Option<String>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub line: usize,
    pub edge: Option<String>,
    pub confidence: Option<String>,
    pub evidence: Option<String>,
}

fn event_produce_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r#"(?:publish|produce|emit)\(\s*["'`]([A-Za-z0-9_.-]+)["'`]"#)
            .case_insensitive(true)
            .build()
            .unwrap()
    })
}

fn event_consume_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r#"(?:subscribe|consume|onMessage|handler)\(\s*["'`]([A-Za-z0-9_.-]+)["'`]"#)
            .case_insensitive(true)
            .build()
            .unwrap()
    })
}

/// Port of `extractEventFacts({ text, path })`.
pub fn extract_event_facts(text: &str, path: &str) -> Vec<StackFact> {
    let mut facts = Vec::new();
    for (idx, raw) in text.split(['\n']).enumerate() {
        let line_str = raw.trim_end_matches('\r');
        let line = line_str.trim();
        let line_no = idx + 1;
        if let Some(cap) = event_produce_re().captures(line) {
            facts.push(StackFact {
                kind: "producer".into(),
                topic: Some(cap[1].to_string()),
                name: None,
                action: None,
                resource_type: None,
                line: line_no,
                edge: Some("PRODUCES".into()),
                confidence: Some("CROSS_FILE_HEURISTIC".into()),
                evidence: Some(format!("{path}:{line_no}")),
            });
        }
        if let Some(cap) = event_consume_re().captures(line) {
            facts.push(StackFact {
                kind: "consumer".into(),
                topic: Some(cap[1].to_string()),
                name: None,
                action: None,
                resource_type: None,
                line: line_no,
                edge: Some("CONSUMES".into()),
                confidence: Some("CROSS_FILE_HEURISTIC".into()),
                evidence: Some(format!("{path}:{line_no}")),
            });
        }
    }
    facts
}

fn db_model_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r"(?:model|table)\s+([A-Za-z0-9_]+)")
            .case_insensitive(true)
            .build()
            .unwrap()
    })
}

fn db_read_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r"\b([A-Za-z_$][\w$]*)\.(find|get|query|select)\(")
            .case_insensitive(true)
            .build()
            .unwrap()
    })
}

fn db_write_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r"\b([A-Za-z_$][\w$]*)\.(insert|create|update|delete|save)\(")
            .case_insensitive(true)
            .build()
            .unwrap()
    })
}

/// Port of `extractDatabaseFacts({ text, path })`.
pub fn extract_database_facts(text: &str, path: &str) -> Vec<StackFact> {
    let mut facts = Vec::new();
    for (idx, raw) in text.split(['\n']).enumerate() {
        let line_str = raw.trim_end_matches('\r');
        let line = line_str.trim();
        let line_no = idx + 1;
        if let Some(cap) = db_model_re().captures(line) {
            facts.push(StackFact {
                kind: "model".into(),
                topic: None,
                name: Some(cap[1].to_string()),
                action: None,
                resource_type: None,
                line: line_no,
                edge: None,
                confidence: Some("CROSS_FILE_HEURISTIC".into()),
                evidence: Some(format!("{path}:{line_no}")),
            });
        }
        if let Some(cap) = db_read_re().captures(line) {
            facts.push(StackFact {
                kind: "read".into(),
                topic: None,
                name: Some(cap[1].to_string()),
                action: None,
                resource_type: None,
                line: line_no,
                edge: Some("READS".into()),
                confidence: Some("CROSS_FILE_HEURISTIC".into()),
                evidence: Some(format!("{path}:{line_no}")),
            });
        }
        if let Some(cap) = db_write_re().captures(line) {
            facts.push(StackFact {
                kind: "write".into(),
                topic: None,
                name: Some(cap[1].to_string()),
                action: None,
                resource_type: None,
                line: line_no,
                edge: Some("WRITES".into()),
                confidence: Some("CROSS_FILE_HEURISTIC".into()),
                evidence: Some(format!("{path}:{line_no}")),
            });
        }
    }
    facts
}

fn deploy_action_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r"^(\s*-\s*)?(?:uses|action|step|deploy)\s*:\s*([A-Za-z0-9_.-]+)")
            .case_insensitive(true)
            .build()
            .unwrap()
    })
}

fn deploy_tf_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"^\s*resource\s+"([^"]+)""#).unwrap())
}

/// Port of `extractDeploymentFacts({ text, path })`.
pub fn extract_deployment_facts(text: &str, path: &str) -> Vec<StackFact> {
    let mut facts = Vec::new();
    for (idx, raw) in text.split(['\n']).enumerate() {
        let line_str = raw.trim_end_matches('\r');
        let line = line_str.trim();
        let line_no = idx + 1;
        if let Some(cap) = deploy_action_re().captures(line) {
            facts.push(StackFact {
                kind: "deploy".into(),
                topic: None,
                name: None,
                action: Some(cap[2].to_string()),
                resource_type: None,
                line: line_no,
                edge: Some("DEPLOYS".into()),
                confidence: Some("CROSS_FILE_HEURISTIC".into()),
                evidence: Some(format!("{path}:{line_no}")),
            });
        }
        if let Some(cap) = deploy_tf_re().captures(line) {
            facts.push(StackFact {
                kind: "resource".into(),
                topic: None,
                name: None,
                action: None,
                resource_type: Some(cap[1].to_string()),
                line: line_no,
                edge: Some("CONFIGURES".into()),
                confidence: Some("CROSS_FILE_HEURISTIC".into()),
                evidence: Some(format!("{path}:{line_no}")),
            });
        }
    }
    facts
}

/// A single HTTP route or handler fact, ported from `extractRoutes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteFact {
    pub kind: String, // "route" | "handler"
    pub method: Option<String>,
    pub path: Option<String>,
    pub handler: Option<String>,
    pub name: Option<String>,
    pub line: usize,
    pub confidence: String,
    pub evidence: String,
}

fn next_express_route_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(
            r#"(?:app|router)\.(get|post|put|delete|patch)\(\s*["'`]([^"'`]+)["'`]\s*,\s*([A-Za-z_$][\w$]*)?"#,
        )
        .case_insensitive(true)
        .build()
        .unwrap()
    })
}

fn next_export_default_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"export\s+default\s+async\s+function\s+([A-Za-z0-9_]+)").unwrap())
}

fn fastapi_route_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r#"@app\.(get|post|put|delete|patch)\(\s*["'`]([^"'`]+)["'`]"#)
            .case_insensitive(true)
            .build()
            .unwrap()
    })
}

fn fastapi_def_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^def\s+([A-Za-z0-9_]+)\s*\(").unwrap())
}

fn axum_route_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r#"\.route\(\s*["'`]([^"'`]+)["'`]"#)
            .case_insensitive(true)
            .build()
            .unwrap()
    })
}

fn axum_fn_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"async\s+fn\s+([A-Za-z0-9_]+)").unwrap())
}

fn trailing_line_comment_slashes_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"//.*$").unwrap())
}

fn trailing_line_comment_hash_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"#.*$").unwrap())
}

/// Port of `extractRoutes({ stack, text, path })`. Deterministic,
/// evidence-carrying route/handler extraction gated by legacy line-comment
/// stripping so a route mentioned only inside a comment never fabricates a
/// route.
pub fn extract_routes(stack: &str, text: &str, path: &str) -> Vec<RouteFact> {
    let mut routes = Vec::new();
    for (idx, raw) in text.split(['\n']).enumerate() {
        let raw_line = raw.trim_end_matches('\r');
        let mut line = raw_line.trim().to_string();
        if line.starts_with("//")
            || line.starts_with('#')
            || line.starts_with("/*")
            || line.starts_with('*')
        {
            continue;
        }
        // Strip inline comments so `code; // app.get('/x')` never fabricates a route.
        line = trailing_line_comment_slashes_re()
            .replace(&line, "")
            .to_string();
        line = trailing_line_comment_hash_re()
            .replace(&line, "")
            .to_string();
        let line = line.trim();
        let line_no = idx + 1;

        match stack {
            "next-express" => {
                if let Some(cap) = next_express_route_re().captures(line) {
                    routes.push(RouteFact {
                        kind: "route".into(),
                        method: Some(cap[1].to_uppercase()),
                        path: Some(cap[2].to_string()),
                        handler: cap.get(3).map(|m| m.as_str().to_string()),
                        name: None,
                        line: line_no,
                        confidence: "CROSS_FILE_HEURISTIC".into(),
                        evidence: format!("{path}:{line_no}"),
                    });
                }
                if let Some(cap) = next_export_default_re().captures(line) {
                    routes.push(RouteFact {
                        kind: "handler".into(),
                        method: None,
                        path: None,
                        handler: None,
                        name: Some(cap[1].to_string()),
                        line: line_no,
                        confidence: "CROSS_FILE_HEURISTIC".into(),
                        evidence: format!("{path}:{line_no}"),
                    });
                }
            }
            "fastapi-django" => {
                if let Some(cap) = fastapi_route_re().captures(line) {
                    routes.push(RouteFact {
                        kind: "route".into(),
                        method: Some(cap[1].to_uppercase()),
                        path: Some(cap[2].to_string()),
                        handler: None,
                        name: None,
                        line: line_no,
                        confidence: "CROSS_FILE_HEURISTIC".into(),
                        evidence: format!("{path}:{line_no}"),
                    });
                }
                if let Some(cap) = fastapi_def_re().captures(line) {
                    routes.push(RouteFact {
                        kind: "handler".into(),
                        method: None,
                        path: None,
                        handler: None,
                        name: Some(cap[1].to_string()),
                        line: line_no,
                        confidence: "CROSS_FILE_HEURISTIC".into(),
                        evidence: format!("{path}:{line_no}"),
                    });
                }
            }
            "tauri-axum" => {
                if let Some(cap) = axum_route_re().captures(line) {
                    routes.push(RouteFact {
                        kind: "route".into(),
                        method: Some("ANY".into()),
                        path: Some(cap[1].to_string()),
                        handler: None,
                        name: None,
                        line: line_no,
                        confidence: "CROSS_FILE_HEURISTIC".into(),
                        evidence: format!("{path}:{line_no}"),
                    });
                }
                if let Some(cap) = axum_fn_re().captures(line) {
                    routes.push(RouteFact {
                        kind: "handler".into(),
                        method: None,
                        path: None,
                        handler: None,
                        name: Some(cap[1].to_string()),
                        line: line_no,
                        confidence: "CROSS_FILE_HEURISTIC".into(),
                        evidence: format!("{path}:{line_no}"),
                    });
                }
            }
            _ => {}
        }
    }
    routes
}

/// Native registry entry for the gated cross-stack provider. The legacy
/// provider emits domain facts against the current file; native registry
/// carries those attributes in evidence because GraphNode/GraphEdge are
/// closed storage shapes.
pub const PROVIDER_ID: &str = "blueprint-frameworks";
pub const PROVIDER_VERSION: &str = "gated-evidence-v1";

fn safe_name(value: &str) -> String {
    let mut out: String = value.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') { c } else { '-' }).collect();
    out.truncate(100);
    if out.is_empty() { "fact".into() } else { out }
}

fn fact_node(file: &crate::graph::FileRecord, category: &str, name: &str, line: usize, attributes: serde_json::Value) -> (GraphNode, GraphEdge) {
    let id = format!("domain:{PROVIDER_ID}:{}:{category}:{}:{line}", file.path, safe_name(name));
    let mut evidence = json!({
        "path": file.path,
        "startLine": line,
        "endLine": line,
        "contentHash": file.content_hash,
        "provider": PROVIDER_ID,
        "providerVersion": PROVIDER_VERSION,
        "confidenceTier": "CROSS_FILE_HEURISTIC",
        "confidence": 0.75,
    });
    if let (Some(base), Some(extra)) = (evidence.as_object_mut(), attributes.as_object()) {
        for (key, value) in extra { base.insert(key.clone(), value.clone()); }
    }
    let node = GraphNode { id: id.clone(), kind: "domain".into(), path: Some(file.path.clone()), name: Some(name.into()), generation_id: String::new(), evidence: vec![evidence.clone()] };
    let edge_kind = attributes.get("edge").and_then(|v| v.as_str()).unwrap_or("CONTAINS");
    let edge = GraphEdge { id: format!("edge:{edge_kind}:file:{}->{id}:{PROVIDER_ID}:{line}", file.path), kind: edge_kind.into(), source: format!("file:{}", file.path), target: Some(id), generation_id: String::new(), evidence: vec![evidence] };
    (node, edge)
}

/// Run provider over in-memory scanned files. Gating is per file, matching
/// addFrameworkEvidence in legacy build pipeline.
pub fn run(ctx: &ProviderContext<'_>) -> ProviderOutput {
    let mut output = ProviderOutput::default();
    for file in ctx.files {
        let text = file.text.as_deref().unwrap_or("");
        let imports = imported_packages(text);
        let stacks: Vec<&str> = ["next-express", "fastapi-django", "tauri-axum"].into_iter().filter(|stack| framework_gate_active(&imports, stack)).collect();
        let event_gate = domain_gate_active(&imports, "event");
        let database_gate = domain_gate_active(&imports, "database");
        let lower_path = file.path.replace('\\', "/").to_ascii_lowercase();
        let deployment_gate = lower_path.contains(".github/workflows")
            || lower_path.split('/').any(|segment| matches!(segment, "deploy" | "deployment" | "infrastructure"))
            || lower_path.rsplit('/').next().is_some_and(|n| n == "dockerfile");
        for stack in stacks {
            for fact in extract_routes(stack, text, &file.path) {
                let name = if fact.kind == "route" { format!("{} {}", fact.method.as_deref().unwrap_or("ANY"), fact.path.as_deref().unwrap_or("")) } else { fact.name.clone().unwrap_or_default() };
                let edge = if fact.kind == "route" { "ROUTES_TO" } else { "CONTAINS" };
                let attrs = json!({"edge": edge, "stack": stack, "method": fact.method, "routePath": fact.path, "handler": fact.handler, "factConfidence": fact.confidence, "legacyEvidence": fact.evidence});
                let (node, edge) = fact_node(file, if fact.kind == "route" { "HttpRoute" } else { "HttpHandler" }, &name, fact.line, attrs);
                output.nodes.push(node);
                output.edges.push(edge);
            }
        }
        if event_gate {
            for fact in extract_event_facts(text, &file.path) {
                let name = fact.topic.clone().unwrap_or_default();
                let attrs = json!({"edge": fact.edge, "frameworkDomain": "event", "role": fact.kind, "factConfidence": fact.confidence, "legacyEvidence": fact.evidence});
                let (node, edge) = fact_node(file, "EventTopic", &name, fact.line, attrs);
                output.nodes.push(node);
                output.edges.push(edge);
            }
        }
        if database_gate {
            for fact in extract_database_facts(text, &file.path).into_iter().filter(|f| f.name.is_some() && f.edge.is_some()) {
                let name = fact.name.clone().unwrap_or_default();
                let attrs = json!({"edge": fact.edge, "frameworkDomain": "database", "role": fact.kind, "factConfidence": fact.confidence, "legacyEvidence": fact.evidence});
                let (node, edge) = fact_node(file, "DatabaseModel", &name, fact.line, attrs);
                output.nodes.push(node);
                output.edges.push(edge);
            }
        }
        if deployment_gate {
            for fact in extract_deployment_facts(text, &file.path) {
                let name = fact.action.clone().or(fact.resource_type.clone()).unwrap_or_default();
                if name.is_empty() { continue; }
                let attrs = json!({"edge": fact.edge, "frameworkDomain": "deployment", "role": fact.kind, "factConfidence": fact.confidence, "legacyEvidence": fact.evidence});
                let (node, edge) = fact_node(file, "Deployment", &name, fact.line, attrs);
                output.nodes.push(node);
                output.edges.push(edge);
            }
        }
    }
    output
}
