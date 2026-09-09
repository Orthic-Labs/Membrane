//! Deterministic, read-only projections owned by the native Blueprint seam.
//!
//! Export never opens the graph store and never reparses repository sources.  It
//! operates only on the already loaded [`Generation`] plus the request bounds.

use crate::api::{BlueprintError, BlueprintRequest, RequestContext};
use crate::store::Generation;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const MAX_PACK_FINDINGS: usize = 100;
const MAX_MERMAID_NODES: usize = 256;

/// Return the complete current generation, or a bounded Mermaid projection.
/// JSON is intentionally lossless: it is the serde representation of the
/// loaded generation, not a second export schema or a store read.
pub fn execute_export(
    generation: &Generation,
    request: &BlueprintRequest,
    context: &RequestContext,
) -> Result<Value, BlueprintError> {
    context.check()?;
    if let Some(expected) = request.generation.as_deref().or_else(|| request.input.get("generation").and_then(Value::as_str)) {
        if generation.generation_id() != Some(expected) {
            return Err(BlueprintError::generation_mismatch(expected, generation.generation_id().unwrap_or("")));
        }
    }
    let format = request.input.get("format").and_then(Value::as_str).unwrap_or("json");
    let value = match format {
        "json" => serde_json::to_value(generation).map_err(|error| BlueprintError::malformed(error.to_string()))?,
        "mermaid" => mermaid_projection(generation, request, context)?,
        other => return Err(BlueprintError::invalid(format!("unsupported export format: {other}"))),
    };
    context.check()?;
    ensure_response_bytes(&value, context)?;
    Ok(value)
}

/// Build a portable, citation-preserving evidence pack from finding values.
/// Findings are selected before this seam is called; the seam applies the
/// portable pack cap and records any bounded omission rather than truncating a
/// finding identifier or citation.
pub fn build_evidence_pack(
    repo_id: Option<&str>,
    generation_id: &str,
    findings: &[Value],
) -> Result<Value, BlueprintError> {
    if generation_id.trim().is_empty() {
        return Err(BlueprintError::missing("generationId"));
    }
    let mut results = Vec::new();
    let mut omissions = Vec::new();
    for finding in findings.iter().take(MAX_PACK_FINDINGS) {
        results.push(pack_result(finding));
    }
    if findings.len() > MAX_PACK_FINDINGS {
        omissions.push(json!({"reason":"finding_cap","count":findings.len()-MAX_PACK_FINDINGS}));
    }
    let mut pack = Map::new();
    pack.insert("schemaVersion".into(), json!(1));
    pack.insert("kind".into(), json!("evidence-pack"));
    pack.insert("repoId".into(), repo_id.map(Value::from).unwrap_or(Value::Null));
    pack.insert("generationId".into(), Value::from(generation_id));
    pack.insert("providerTiers".into(), Value::Array(Vec::new()));
    pack.insert("results".into(), Value::Array(results));
    pack.insert("omissions".into(), Value::Array(omissions));
    pack.insert("continuationCursor".into(), Value::Null);
    pack.insert("packDigest".into(), Value::Null);
    let digest = digest_value(&Value::Object(pack.clone()))?;
    pack.insert("packDigest".into(), Value::from(digest));
    pack.insert("markdown".into(), Value::from(pack_markdown(&pack)));
    Ok(Value::Object(pack))
}

/// Convert the same finding values used by the findings service to SARIF 2.1.0.
/// Rule metadata is deduplicated and sorted by rule id; result order remains
/// caller order so fingerprints and citations are not rewritten.
pub fn findings_to_sarif(findings: &[Value], tool_version: &str) -> Value {
    let mut rules = BTreeMap::<String, Value>::new();
    for finding in findings {
        let rule_id = string_field(finding, "ruleId").unwrap_or_else(|| "blueprint.unknown".into());
        let level = sarif_level(string_field(finding, "severity").as_deref());
        rules.entry(rule_id.clone()).or_insert_with(|| json!({
            "id": rule_id,
            "name": string_field(finding, "ruleName").unwrap_or_else(|| string_field(finding, "ruleId").unwrap_or_else(|| "blueprint.unknown".into())),
            "shortDescription": {"text": string_field(finding, "ruleDescription").or_else(|| string_field(finding, "message")).unwrap_or_else(|| "Blueprint finding".into())},
            "defaultConfiguration": {"level": level},
        }));
    }
    let results = findings.iter().map(|finding| {
        let rule_id = string_field(finding, "ruleId").unwrap_or_else(|| "blueprint.unknown".into());
        let path = string_field(finding, "path").unwrap_or_default();
        let start = number_field(finding, "startLine").unwrap_or(1);
        let end = number_field(finding, "endLine").unwrap_or(start);
        let mut properties = Map::new();
        for key in ["generationId", "confidenceTier", "evidencePath"] {
            if let Some(value) = finding.get(key) { properties.insert(key.into(), value.clone()); }
        }
        json!({
            "ruleId": rule_id,
            "level": sarif_level(string_field(finding, "severity").as_deref()),
            "message": {"text": string_field(finding, "message").unwrap_or_else(|| "Blueprint finding".into())},
            "partialFingerprints": {"blueprintFinding": string_field(finding, "fingerprint").unwrap_or_default()},
            "locations": [{"physicalLocation": {"artifactLocation": {"uri": path}, "region": {"startLine": start, "endLine": end}}}],
            "properties": properties,
        })
    }).collect::<Vec<_>>();
    json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [{"tool": {"driver": {"name": "Blueprint", "version": tool_version, "rules": rules.into_values().collect::<Vec<_>>() }}, "results": results}],
    })
}

fn pack_result(finding: &Value) -> Value {
    let evidence = finding.get("evidence").or_else(|| finding.get("evidenceRows")).and_then(Value::as_array)
        .map(|rows| rows.iter().map(|row| json!({
            "path": row.get("path").cloned().unwrap_or(Value::Null),
            "startLine": row.get("startLine").cloned().unwrap_or(Value::Null),
            "endLine": row.get("endLine").cloned().unwrap_or(Value::Null),
            "contentHash": row.get("contentHash").cloned().unwrap_or(Value::Null),
        })).collect::<Vec<_>>()).unwrap_or_default();
    let span = finding.get("span").cloned().or_else(|| {
        if finding.get("startLine").is_some() || finding.get("endLine").is_some() {
            Some(json!({"startLine": finding.get("startLine").cloned().unwrap_or(json!(1)), "endLine": finding.get("endLine").cloned().unwrap_or(json!(1))}))
        } else { None }
    }).unwrap_or(Value::Null);
    json!({
        "id": finding.get("id").or_else(|| finding.get("fingerprint")).cloned().unwrap_or(Value::Null),
        "path": finding.get("path").cloned().unwrap_or(Value::Null),
        "span": span,
        "contentHash": finding.get("contentHash").cloned().unwrap_or(Value::Null),
        "confidenceTier": finding.get("confidenceTier").cloned().unwrap_or(Value::Null),
        "evidence": evidence,
    })
}

fn pack_markdown(pack: &Map<String, Value>) -> String {
    let repo = pack.get("repoId").and_then(Value::as_str).unwrap_or("");
    let generation = pack.get("generationId").and_then(Value::as_str).unwrap_or("");
    let digest = pack.get("packDigest").and_then(Value::as_str).unwrap_or("");
    let mut lines = vec![format!("# Evidence pack — {repo}"), String::new(), format!("Generation: `{generation}`"), format!("Digest: `{digest}`"), String::new(), "## Results".into(), String::new()];
    if let Some(results) = pack.get("results").and_then(Value::as_array) {
        for result in results {
            let id = result.get("id").and_then(Value::as_str).unwrap_or("");
            let path = result.get("path").and_then(Value::as_str).unwrap_or("");
            let span = result.get("span").and_then(Value::as_object).map(|span| format!(":{}-{}", span.get("startLine").and_then(Value::as_u64).unwrap_or(1), span.get("endLine").and_then(Value::as_u64).unwrap_or(1))).unwrap_or_default();
            let tier = result.get("confidenceTier").and_then(Value::as_str).unwrap_or("?");
            lines.push(format!("- `{id}` — {path}{span} ({tier})"));
        }
    }
    if let Some(omissions) = pack.get("omissions").and_then(Value::as_array) { if !omissions.is_empty() { lines.extend([String::new(), "## Omissions".into(), String::new()]); for omission in omissions { lines.push(format!("- {}", omission.get("reason").and_then(Value::as_str).unwrap_or("omitted"))); } } }
    lines.join("\n")
}

fn mermaid_projection(generation: &Generation, request: &BlueprintRequest, context: &RequestContext) -> Result<Value, BlueprintError> {
    let limit = request.input.get("limit").and_then(Value::as_u64).unwrap_or(60).min(context.bounds.max_paths as u64).min(MAX_MERMAID_NODES as u64).max(1) as usize;
    let mut nodes = generation.nodes.iter().filter_map(|value| serde_json::from_value::<crate::model::GraphNode>(value.clone()).ok()).collect::<Vec<_>>();
    nodes.sort_by(|a,b| a.id.cmp(&b.id));
    let truncated = nodes.len() > limit;
    nodes.truncate(limit);
    let ids = nodes.iter().map(|node| node.id.clone()).collect::<std::collections::BTreeSet<_>>();
    let mut edges = generation.edges.iter().filter_map(|value| serde_json::from_value::<crate::model::GraphEdge>(value.clone()).ok()).filter(|edge| edge.target.as_ref().is_some_and(|target| ids.contains(&edge.source) && ids.contains(target))).collect::<Vec<_>>();
    edges.sort_by(|a,b| (a.source.clone(), a.target.clone().unwrap_or_default(), a.kind.clone()).cmp(&(b.source.clone(), b.target.clone().unwrap_or_default(), b.kind.clone())));
    edges.truncate(limit.saturating_mul(2));
    let aliases = nodes.iter().enumerate().map(|(index,node)| (node.id.clone(), format!("n{index}"))).collect::<BTreeMap<_,_>>();
    let provider = generation.provider.as_ref().and_then(|value| value.get("id")).and_then(Value::as_str).or_else(|| generation.provider.as_ref().and_then(Value::as_str)).unwrap_or("native-rust");
    let mut lines = vec!["flowchart LR".into(), format!("%% provider: {provider}"), "%% view: architecture".into(), format!("%% truncated: {truncated}")];
    for node in &nodes { context.check()?; let label = node.name.as_deref().or(node.path.as_deref()).unwrap_or(&node.id); lines.push(format!("  {}[\"{}:{}\"]", aliases[&node.id], node.kind, escape_mermaid(&label.chars().take(80).collect::<String>()))); }
    for edge in &edges { context.check()?; if let Some(target)=edge.target.as_ref() { lines.push(format!("  {} -->|\"{}\"| {}", aliases[&edge.source], escape_mermaid(&edge.kind), aliases[target])); } }
    let text = format!("{}\n", lines.join("\n"));
    let mut result = Map::new(); result.insert("schemaVersion".into(), json!(1)); result.insert("kind".into(), json!("mermaid")); result.insert("generationId".into(), Value::from(generation.generation_id().unwrap_or(""))); result.insert("text".into(), Value::from(text)); result.insert("omissions".into(), if truncated { json!([{"reason":"node_cap","count":generation.nodes.len()-limit}]) } else { json!([]) }); Ok(Value::Object(result))
}

fn escape_mermaid(value: &str) -> String { value.replace('\\', "/").replace('"', "'").replace('\n', " ") }
fn string_field(value: &Value, key: &str) -> Option<String> { value.get(key).and_then(Value::as_str).map(str::to_owned) }
fn number_field(value: &Value, key: &str) -> Option<u64> { value.get(key).and_then(Value::as_u64) }
fn sarif_level(severity: Option<&str>) -> &'static str { match severity { Some("error") => "error", Some("info") => "note", _ => "warning" } }
fn digest_value(value: &Value) -> Result<String, BlueprintError> { let bytes = serde_json::to_vec(value).map_err(|error| BlueprintError::malformed(error.to_string()))?; Ok(hex::encode(Sha256::digest(bytes))) }
fn ensure_response_bytes(value: &Value, context: &RequestContext) -> Result<(), BlueprintError> { let bytes = serde_json::to_vec(value).map_err(|error| BlueprintError::malformed(error.to_string()))?; if bytes.len() > context.bounds.max_response_bytes { Err(BlueprintError::oversized("response_bytes")) } else { Ok(()) } }
