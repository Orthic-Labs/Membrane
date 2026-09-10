//! Native Rust port of `blueprint/src/graph/signature-projection.mjs`.
//!
//! Projects a bounded, sorted list of symbol/class signatures from a
//! generation's nodes. Operates on the same `nodes` JSON shape carried by
//! [`crate::store::Generation`].

use crate::store::Generation;
use serde_json::{json, Value};

const SIGNATURE_LABELS: [&str; 8] =
    ["Function", "Method", "Class", "Interface", "Trait", "Struct", "Test", "Screen"];

#[derive(Debug, Clone, Default)]
pub struct SignatureProjectionOptions {
    pub limit: Option<i64>,
    pub path_prefix: Option<String>,
    pub kinds: Option<Vec<String>>,
}

fn labels_of(node: &Value) -> Vec<String> {
    node.get("labels")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn is_signature_candidate(node: &Value) -> bool {
    let kind = node.get("kind").and_then(Value::as_str).unwrap_or("");
    if kind == "symbol" || kind == "class" {
        return true;
    }
    labels_of(node).iter().any(|label| SIGNATURE_LABELS.contains(&label.as_str()))
}

fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn signature_text(node: &Value) -> String {
    let labels = labels_of(node);
    let kind = labels
        .first()
        .cloned()
        .or_else(|| node.get("kind").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| "symbol".to_string());
    let name = node
        .get("qualifiedName")
        .and_then(Value::as_str)
        .or_else(|| node.get("name").and_then(Value::as_str))
        .or_else(|| node.get("id").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let declared = node
        .get("signature")
        .and_then(Value::as_str)
        .or_else(|| node.get("rawDeclaredType").and_then(Value::as_str))
        .or_else(|| node.get("semanticSignature").and_then(Value::as_str));
    let receiver = node
        .get("receiverType")
        .and_then(Value::as_str)
        .or_else(|| node.get("declaringType").and_then(Value::as_str));
    let line = node
        .get("evidence")
        .and_then(Value::as_array)
        .and_then(|e| e.first())
        .and_then(|e| e.get("startLine"));
    let path = node.get("path").and_then(Value::as_str);

    let mut parts: Vec<String> = vec![kind, name];
    if let Some(receiver) = receiver.filter(|value| !value.is_empty()) {
        parts.push(format!("receiver={receiver}"));
    }
    if let Some(declared) = declared.filter(|value| !value.is_empty()) {
        let collapsed = declared.split_whitespace().collect::<Vec<_>>().join(" ");
        if !collapsed.is_empty() {
            parts.push(format!(":: {collapsed}"));
        }
    }
    if let Some(path) = path {
        // Legacy JS uses `line ? ... : ""`; numeric zero is therefore not
        // rendered even though it is a valid JSON value.
        let line_suffix = line
            .and_then(|l| if l.is_null() || l.as_i64() == Some(0) { None } else { Some(l.to_string()) })
            .map(|l| format!(":{l}"))
            .unwrap_or_default();
        parts.push(format!("@ {path}{line_suffix}"));
    }
    parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Mirrors `projectSymbolSignatures(generation, options)`.
pub fn project_symbol_signatures(generation: &Generation, options: &SignatureProjectionOptions) -> Value {
    let generation_id = generation
        .manifest
        .as_ref()
        .and_then(|m| m.get("generationId"))
        .cloned()
        .unwrap_or(Value::Null);

    // Match `Math.max(1, Math.min(1000, Number(limit) || 200))`: zero (and
    // absent input) selects the default rather than becoming a one-row cap.
    let cap = options.limit.filter(|value| *value != 0).unwrap_or(200).max(1).min(1000) as usize;
    let allowed_kinds: Option<Vec<String>> = options
        .kinds
        .as_ref()
        .filter(|k| !k.is_empty())
        .map(|k| k.iter().map(|s| s.to_lowercase()).collect());

    let mut rows: Vec<Value> = generation
        .nodes
        .iter()
        .filter(|node| is_signature_candidate(node))
        .filter(|node| {
            options.path_prefix.as_ref().map_or(true, |prefix| {
                node.get("path").and_then(Value::as_str).unwrap_or("").starts_with(prefix.as_str())
            })
        })
        .filter(|node| {
            let Some(allowed) = &allowed_kinds else { return true };
            let mut candidates: Vec<String> =
                vec![node.get("kind").and_then(Value::as_str).unwrap_or("").to_lowercase()];
            candidates.extend(labels_of(node).iter().map(|l| l.to_lowercase()));
            candidates.iter().any(|c| allowed.contains(c))
        })
        .map(|node| {
            json!({
                "id": node.get("id").cloned().unwrap_or(Value::Null),
                "portableId": node.get("portableId").cloned().unwrap_or(Value::Null),
                "kind": node.get("kind").cloned().unwrap_or(Value::Null),
                "labels": node.get("labels").cloned().unwrap_or_else(|| json!([])),
                "name": node.get("name").cloned().unwrap_or(Value::Null),
                "qualifiedName": node.get("qualifiedName").cloned().unwrap_or(Value::Null),
                "path": node.get("path").cloned().unwrap_or(Value::Null),
                "line": node.get("evidence").and_then(Value::as_array).and_then(|e| e.first()).and_then(|e| e.get("startLine")).cloned().unwrap_or(Value::Null),
                "signature": signature_text(node),
                "evidence": node.get("evidence").cloned().unwrap_or_else(|| json!([])),
            })
        })
        .collect();

    rows.sort_by(|a, b| {
        // `localeCompare(String(value))` is the legacy ordering contract.
        // The final id tie-break makes equal path/name rows deterministic
        // even when provider node order changes.
        let a_path = js_string(a.get("path").unwrap_or(&Value::Null));
        let b_path = js_string(b.get("path").unwrap_or(&Value::Null));
        a_path.cmp(&b_path).then_with(|| {
            let a_name = a.get("qualifiedName").filter(|v| !v.is_null())
                .or_else(|| a.get("name").filter(|v| !v.is_null()))
                .or_else(|| a.get("id")).map(js_string).unwrap_or_else(|| "undefined".to_string());
            let b_name = b.get("qualifiedName").filter(|v| !v.is_null())
                .or_else(|| b.get("name").filter(|v| !v.is_null()))
                .or_else(|| b.get("id")).map(js_string).unwrap_or_else(|| "undefined".to_string());
            a_name.cmp(&b_name)
        }).then_with(|| {
            js_string(a.get("id").unwrap_or(&Value::Null)).cmp(&js_string(b.get("id").unwrap_or(&Value::Null)))
        })
    });

    let total = rows.len();
    let truncated = total > cap;
    rows.truncate(cap);

    json!({
        "schemaVersion": 1,
        "kind": "symbol-signatures",
        "generationId": generation_id,
        "signatures": rows,
        "truncated": truncated,
        "omissions": if truncated { json!([{ "reason": "signature_limit", "limit": cap }]) } else { json!([]) },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation_from(nodes: Vec<Value>) -> Generation {
        Generation { manifest: Some(json!({ "generationId": "gen-1" })), nodes, ..Default::default() }
    }

    #[test]
    fn projects_function_signatures_sorted_by_path_then_name() {
        let generation = generation_from(vec![
            json!({ "id": "b", "kind": "symbol", "labels": ["Function"], "name": "beta", "path": "b.rs", "evidence": [{"startLine": 3}] }),
            json!({ "id": "a", "kind": "symbol", "labels": ["Function"], "name": "alpha", "path": "a.rs", "evidence": [] }),
        ]);
        let out = project_symbol_signatures(&generation, &SignatureProjectionOptions::default());
        let sigs = out["signatures"].as_array().unwrap();
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[0]["id"], json!("a"));
        assert_eq!(sigs[1]["id"], json!("b"));
        assert!(sigs[1]["signature"].as_str().unwrap().contains("beta"));
    }

    #[test]
    fn filters_by_path_prefix_and_kind() {
        let generation = generation_from(vec![
            json!({ "id": "a", "kind": "symbol", "labels": ["Function"], "name": "a", "path": "src/a.rs" }),
            json!({ "id": "b", "kind": "symbol", "labels": ["Class"], "name": "b", "path": "test/b.rs" }),
        ]);
        let out = project_symbol_signatures(
            &generation,
            &SignatureProjectionOptions { limit: None, path_prefix: Some("src/".to_string()), kinds: Some(vec!["function".to_string()]) },
        );
        let sigs = out["signatures"].as_array().unwrap();
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0]["id"], json!("a"));
    }

    #[test]
    fn truncates_at_limit_and_reports_omission() {
        let nodes: Vec<Value> = (0..5)
            .map(|i| json!({ "id": format!("n{i}"), "kind": "symbol", "labels": ["Function"], "name": format!("n{i}"), "path": "a.rs" }))
            .collect();
        let generation = generation_from(nodes);
        let out = project_symbol_signatures(&generation, &SignatureProjectionOptions { limit: Some(2), ..Default::default() });
        assert_eq!(out["signatures"].as_array().unwrap().len(), 2);
        assert_eq!(out["truncated"], json!(true));
        assert_eq!(out["omissions"][0]["reason"], json!("signature_limit"));
    }
}
