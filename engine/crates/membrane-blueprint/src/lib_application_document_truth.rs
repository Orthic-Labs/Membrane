//! Native port of `blueprint/src/lib/application/service.mjs`'s
//! `documentTruth` operation and `blueprint/src/graph/doc-truth-projection.mjs`.
//!
//! Ported by lane LIB5 (r5 windows closure). This is a pure, generation-bound
//! projection over claims, claim-code edges, and document-supersession rows
//! that the native store already persists (`Generation::claims`,
//! `Generation::claim_code_edges`, `Generation::document_supersession`) but
//! that the query dispatch path (`GraphGeneration`) drops on load. It reads
//! only from an already-loaded `store::Generation` and performs no I/O of its
//! own, matching `projectDocumentTruth` in the legacy module exactly:
//! grounding state derivation (direct/indirect/unsupported/contradicted/
//! ambiguous/stale), citation dedup, and per-grounding-state counts.

use crate::api::{BlueprintError, BlueprintRequest};
use crate::store::Generation;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

const GROUNDING_STATES: [&str; 6] = ["direct", "indirect", "unsupported", "contradicted", "ambiguous", "stale"];
const DETERMINISTIC_CLASSES: [&str; 3] = ["EXTRACTED", "DETERMINISTIC_EXTRACTION", "AUTHORITATIVE_SEMANTIC"];

/// Execute the `documentTruth` operation against an already-loaded
/// generation. Mirrors `service.documentTruth` minus the freshness-session
/// wrapping, which the native one-shot dispatch (`engine.rs`) already
/// provides uniformly for every query-shaped operation.
pub fn execute_document_truth(generation: &Generation, request: &BlueprintRequest) -> Result<Value, BlueprintError> {
    let generation_id = generation.generation_id().unwrap_or("").to_owned();
    let freshness = request
        .input
        .get("freshness")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let limit = request
        .input
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(200) as usize;
    let claim_id_filter = request.input.get("claimId").and_then(Value::as_str);
    let kind_filter = request.input.get("kind").and_then(Value::as_str);

    let claims = generation.claims.clone().unwrap_or_default();
    let edges = generation.claim_code_edges.clone().unwrap_or_default();
    let supersedes = generation.document_supersession.clone().unwrap_or_default();

    // Join claim_code_edges onto their claim by claimId, matching the SQL
    // join `listClaimSlice` performs in the legacy store layer.
    let mut edges_by_claim: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for edge in &edges {
        if let Some(claim_id) = edge.get("claimId").and_then(Value::as_str) {
            if let Some(kind) = kind_filter {
                if edge.get("kind").and_then(Value::as_str) != Some(kind) { continue; }
            }
            edges_by_claim.entry(claim_id.to_owned()).or_default().push(edge);
        }
    }

    let mut grounded = Vec::new();
    let mut counts: BTreeMap<&str, u64> = GROUNDING_STATES.iter().map(|state| (*state, 0)).collect();
    for claim in claims.iter().take(limit) {
        let claim_id = claim.get("id").and_then(Value::as_str).unwrap_or("");
        if let Some(want) = claim_id_filter {
            if claim_id != want { continue; }
        }
        let claim_edges = edges_by_claim.get(claim_id).cloned().unwrap_or_default();
        let state = grounding_state(&claim_edges, &freshness);
        *counts.entry(state).or_insert(0) += 1;

        let mut citations = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut observed = Vec::new();
        for edge in &claim_edges {
            let edge_citations = citations_for(edge);
            for citation in &edge_citations {
                let key = citation.to_string();
                if seen.insert(key) { citations.push(citation.clone()); }
            }
            observed.push(json!({
                "kind": edge.get("kind").cloned().unwrap_or(Value::Null),
                "source": edge.get("source").cloned().unwrap_or(Value::Null),
                "target": edge.get("target").cloned().unwrap_or(Value::Null),
                "reason": edge.get("reason").cloned().unwrap_or(Value::Null),
                "provenance": edge.get("confidenceClass").cloned().unwrap_or(Value::Null),
                "confidence": public_confidence(edge),
                "evidence": edge_citations,
            }));
        }
        let all_null = !observed.is_empty()
            && observed.iter().all(|row| row.get("confidence").map(Value::is_null).unwrap_or(true));
        let confidence = if all_null {
            Value::Null
        } else {
            Value::Array(
                observed
                    .iter()
                    .filter_map(|row| row.get("confidence").cloned())
                    .filter(|value| !value.is_null())
                    .collect(),
            )
        };
        let mismatch = match state {
            "contradicted" => json!({"present": true, "reason": "declared_intent_conflicts_with_observed_code"}),
            "ambiguous" => json!({"present": true, "reason": "conflicting_grounding_evidence"}),
            _ => json!({"present": false, "reason": Value::Null}),
        };

        grounded.push(json!({
            "claimId": claim.get("id").cloned().unwrap_or(Value::Null),
            "declared": {
                "documentId": claim.get("documentId").cloned().unwrap_or(Value::Null),
                "source": claim.get("source").cloned().unwrap_or(Value::Null),
                "line": claim.get("line").cloned().unwrap_or(Value::Null),
                "status": claim.get("status").cloned().unwrap_or(Value::String("unknown".into())),
                "sourceHash": claim.get("sha1").cloned().unwrap_or(Value::Null),
            },
            "grounding": state,
            "observed": observed,
            "mismatch": mismatch,
            "citations": citations,
            "confidence": confidence,
            "invalidation": {
                "generationId": generation_id,
                "freshness": freshness,
                "stale": state == "stale",
            },
        }));
    }

    let counts_value: Map<String, Value> = GROUNDING_STATES
        .iter()
        .map(|state| (state.to_string(), Value::from(*counts.get(state).unwrap_or(&0))))
        .collect();

    Ok(json!({
        "schemaVersion": 1,
        "generationId": generation_id,
        "claims": claims.into_iter().take(limit).collect::<Vec<_>>(),
        "grounding": grounded,
        "groundingCounts": Value::Object(counts_value),
        "supersedes": supersedes,
        "omissions": [],
        "truncated": false,
    }))
}

fn public_confidence(edge: &Value) -> Value {
    let class = edge.get("confidenceClass").and_then(Value::as_str).unwrap_or("");
    if DETERMINISTIC_CLASSES.contains(&class) {
        Value::Null
    } else {
        edge.get("confidence").cloned().unwrap_or(Value::Null)
    }
}

fn citations_for(edge: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    if let Some(path) = edge.get("evidenceDocPath").and_then(Value::as_str) {
        out.push(json!({
            "kind": "document",
            "path": path,
            "line": edge.get("evidenceDocLine").cloned().unwrap_or(Value::Null),
            "contentHash": edge.get("evidenceDocSha1").cloned().unwrap_or(Value::Null),
        }));
    }
    if let Some(path) = edge.get("evidenceCodePath").and_then(Value::as_str) {
        out.push(json!({
            "kind": "code",
            "path": path,
            "nodeId": edge.get("evidenceCodeNodeId").cloned().unwrap_or(Value::Null),
            "contentHash": edge.get("evidenceCodeContentHash").cloned().unwrap_or(Value::Null),
        }));
    }
    out
}

fn grounding_state(edges: &[&Value], freshness: &str) -> &'static str {
    if freshness != "unknown" && freshness != "fresh" { return "stale"; }
    let mut has_supports = false;
    let mut has_contradicts = false;
    let mut has_supersedes = false;
    for edge in edges {
        match edge.get("kind").and_then(Value::as_str) {
            Some("supports") => has_supports = true,
            Some("contradicts") => has_contradicts = true,
            Some("supersedes") => has_supersedes = true,
            _ => {}
        }
    }
    if has_supports && has_contradicts { return "ambiguous"; }
    if has_contradicts { return "contradicted"; }
    if has_supports { return "direct"; }
    if has_supersedes { return "indirect"; }
    "unsupported"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::BlueprintRequest;
    use crate::model::Operation;

    fn generation_with_claims() -> Generation {
        Generation {
            manifest: Some(json!({"generationId": "gen-1"})),
            claims: Some(vec![
                json!({"id": "claim-1", "documentId": "doc-1", "source": "README.md", "line": 3, "text": "x", "status": "declared", "sha1": "abc"}),
                json!({"id": "claim-2", "documentId": "doc-1", "source": "README.md", "line": 9, "text": "y", "status": "declared", "sha1": "def"}),
            ]),
            claim_code_edges: Some(vec![
                json!({"id": "edge-1", "claimId": "claim-1", "kind": "supports", "source": "claim-1", "target": "node-1", "confidence": 0.9, "confidenceClass": "HEURISTIC", "evidenceCodePath": "src/x.rs", "evidenceCodeNodeId": "node-1"}),
            ]),
            document_supersession: Some(vec![]),
            ..Default::default()
        }
    }

    fn request(input: Value) -> BlueprintRequest {
        let mut request = BlueprintRequest::new("req-1", Operation::DocumentTruth, "C:/tmp");
        request.input = input;
        request
    }

    #[test]
    fn direct_claim_is_grounded_and_counted() {
        let generation = generation_with_claims();
        let result = execute_document_truth(&generation, &request(json!({}))).unwrap();
        assert_eq!(result["generationId"], "gen-1");
        let grounding = result["grounding"].as_array().unwrap();
        assert_eq!(grounding.len(), 2);
        assert_eq!(grounding[0]["grounding"], "direct");
        assert_eq!(grounding[1]["grounding"], "unsupported");
        assert_eq!(result["groundingCounts"]["direct"], 1);
        assert_eq!(result["groundingCounts"]["unsupported"], 1);
    }

    #[test]
    fn deterministic_confidence_class_is_redacted_to_null() {
        let mut generation = generation_with_claims();
        generation.claim_code_edges = Some(vec![json!({
            "id": "edge-1", "claimId": "claim-1", "kind": "supports",
            "confidence": 0.5, "confidenceClass": "EXTRACTED",
        })]);
        let result = execute_document_truth(&generation, &request(json!({}))).unwrap();
        let observed = result["grounding"][0]["observed"][0].clone();
        assert!(observed["confidence"].is_null());
    }

    #[test]
    fn claim_id_filter_narrows_to_one_claim() {
        let generation = generation_with_claims();
        let result = execute_document_truth(&generation, &request(json!({"claimId": "claim-2"}))).unwrap();
        let grounding = result["grounding"].as_array().unwrap();
        assert_eq!(grounding.len(), 1);
        assert_eq!(grounding[0]["claimId"], "claim-2");
    }

    #[test]
    fn stale_freshness_overrides_grounding_state() {
        let generation = generation_with_claims();
        let result = execute_document_truth(&generation, &request(json!({"freshness": "changed_since_generation"}))).unwrap();
        let grounding = result["grounding"].as_array().unwrap();
        assert!(grounding.iter().all(|row| row["grounding"] == "stale"));
        assert_eq!(result["groundingCounts"]["stale"], 2);
    }

    #[test]
    fn contradicts_edge_produces_mismatch() {
        let mut generation = generation_with_claims();
        generation.claim_code_edges = Some(vec![json!({
            "id": "edge-1", "claimId": "claim-1", "kind": "contradicts",
        })]);
        let result = execute_document_truth(&generation, &request(json!({}))).unwrap();
        let row = &result["grounding"][0];
        assert_eq!(row["grounding"], "contradicted");
        assert_eq!(row["mismatch"]["present"], true);
    }
}
