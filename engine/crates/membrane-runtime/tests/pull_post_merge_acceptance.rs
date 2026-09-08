//! Post-merge Pull acceptance through the public runtime owner paths.
//!
//! These checks intentionally exercise delivery state, semantic placement,
//! cache-prefix diagnostics, and the planner envelope rather than copying
//! private implementation logic into fixtures.

use membrane_runtime::cache_prefix::diagnose_cache_prefix;
use membrane_runtime::pull::delivery_state::{record_selected_packet, suppress_packet};
use membrane_runtime::pull::federation::{envelope_from_ccs, EnvelopeInput};
use membrane_runtime::pull::placement::place;
use membrane_protocol::{
    FederationRequestV1, FederationResponseV1, FederationStatus, FreshnessSnapshotV1,
    ProviderId, ProviderOmissionV1, ReasonCode,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn block(id: &str, provider: &str, source_kind: &str, hash: &str) -> Value {
    json!({
        "id": id, "layer": 3, "provider": provider, "sourceKind": source_kind,
        "sourceRef": format!("repo:{id}"), "sourceHash": hash,
        "trustClass": "workspace_tracked", "instructionPolicy": "data_only",
        "priority": 0, "estimatedTokens": 4, "protected": false,
        "recoverable": true, "resolver": "", "text": format!("evidence {id}"),
    })
}

fn packet(blocks: Vec<Value>, trace: &str) -> Value {
    json!({
        "schemaVersion": 1, "traceId": trace, "task": "post merge",
        "mode": "normal", "budget": {"maxTokens": 100, "admittedTokens": 8},
        "allocations": {}, "providerAccounting": {}, "blocks": blocks,
        "omissions": []
    })
}

#[test]
fn same_session_suppression_requires_retention_and_refresh_restores_evidence() {
    let repository = "post-merge-suppression-repo";
    let session = "post-merge-suppression-session";
    let source_hash = format!("sha256:{}", "1".repeat(64));
    let selected = packet(vec![block("unchanged", "blueprint", "repo_code", &source_hash)], "first");
    record_selected_packet(&selected, repository, session);

    let mut retained = serde_json::from_value(selected.clone()).unwrap();
    let receipts = suppress_packet(&mut retained, repository, session, true, false);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].reason, "unchanged_in_horizon");
    assert!(retained.blocks.is_empty());

    let mut refreshed = serde_json::from_value(selected).unwrap();
    assert!(suppress_packet(&mut refreshed, repository, session, true, true).is_empty());
    assert_eq!(refreshed.blocks.len(), 1);

    let mut unknown = refreshed.clone();
    assert!(suppress_packet(&mut unknown, repository, session, false, false).is_empty());
    assert_eq!(unknown.blocks.len(), 1);
}

#[test]
fn semantic_placement_reorders_only_by_class_and_reports_each_original_row() {
    let hash = format!("sha256:{}", "2".repeat(64));
    let mut value = packet(
        vec![
            block("doc", "ledger", "doc", &hash),
            block("repo", "blueprint", "repo_code", &hash),
            block("policy", "rules", "rules", &hash),
        ],
        "placement",
    );
    let before = value["blocks"].clone();
    let mut parsed = serde_json::from_value(value.clone()).unwrap();
    let receipt = place(&mut parsed);
    value["blocks"] = serde_json::to_value(&parsed.blocks).unwrap();

    let ids: Vec<_> = parsed.blocks.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(ids, ["policy", "repo", "doc"]);
    assert_eq!(before.as_array().unwrap().len(), parsed.blocks.len());
    assert_eq!(receipt.policy, "pull-semantic-placement-v1");
    assert_eq!(receipt.rows.len(), 3);
    for row in &receipt.rows {
        let placed = &parsed.blocks[row.placed_index];
        assert_eq!(row.id, placed.id);
        assert_eq!(row.id, before[row.original_index]["id"]);
        assert_eq!(placed.source_hash, hash);
    }
}

#[test]
fn equivalent_packet_prefix_ignores_trace_but_attributes_changed_block() {
    let hash = format!("sha256:{}", "3".repeat(64));
    let first = packet(vec![block("stable", "blueprint", "repo_code", &hash)], "turn-a");
    let next = packet(vec![block("stable", "blueprint", "repo_code", &hash)], "turn-b");
    let stable = diagnose_cache_prefix(&next, Some(&first));
    assert_eq!(stable.cache_break, None);
    assert_eq!(stable.volatility_source, "traceId_excluded");
    assert_eq!(stable.prefix_digest, diagnose_cache_prefix(&first, None).prefix_digest);

    let changed = packet(vec![block("changed", "blueprint", "repo_code", &hash)], "turn-c");
    let diagnostic = diagnose_cache_prefix(&changed, Some(&first));
    assert_eq!(diagnostic.cache_break.as_ref().unwrap().kind, "block");
    assert_eq!(diagnostic.cache_break.as_ref().unwrap().block_id.as_deref(), Some("changed"));
}

fn candidate(id: &str, resolver: &str, text: &str, source_ref: &str) -> Value {
    json!({
        "id": id, "layer": 3, "provider": "blueprint", "sourceKind": "repo_code",
        "sourceRef": source_ref, "sourceHash": format!("sha256:{}", id.chars().next().unwrap().to_string().repeat(64)),
        "trustClass": "workspace_tracked", "instructionPolicy": "data_only",
        "providerScore": 0.9, "scoreComponents": {}, "estimatedTokens": 4,
        "protected": false, "exact": true, "recoverable": true,
        "resolver": resolver, "text": text
    })
}

fn ccs(candidates: Vec<Value>) -> Value {
    ccs_with_omissions(candidates, Vec::new())
}

fn ccs_with_omissions(candidates: Vec<Value>, omissions: Vec<Value>) -> Value {
    json!({
        "schemaVersion": 1, "traceId": "resolver-acceptance", "indexedAt": "2026-09-08T00:00:00Z",
        "task": "resolver acceptance", "mode": "normal", "provider": "blueprint",
        "freshness": {"revision": "r1", "indexedAt": "2026-09-08T00:00:00Z", "stale": false},
        "providerCeiling": {"maxCandidates": 10, "maxEstimatedTokens": 100},
        "candidates": candidates, "omissions": omissions
    })
}

#[test]
fn native_ccs_omission_metadata_survives_planner_envelope_without_content() {
    let populated = json!({
        "id": "blueprint:0", "layer": null, "reason": "provider_unavailable",
        "detailId": "blueprint_timeout", "stage": "provider_fanout"
    });
    let empty = json!({
        "id": "blueprint:1", "layer": null, "reason": "provider_timeout",
        "detailId": "", "stage": ""
    });
    let legacy = json!({
        "id": "blueprint:2", "layer": null, "reason": "provider_disabled"
    });
    let payload = envelope_from_ccs(
        &ccs_with_omissions(Vec::new(), vec![populated, empty, legacy]).to_string(),
        EnvelopeInput { max_tokens: 100, packet_char_budget_override: None, packet_char_budget_model: None,
            accepted_receipt_versions: vec![2], scope_grant_present: false,
            consumer_resolvers: vec![], scope_grant_fence: None, gateway_process_ms: 0.0 },
    ).unwrap();
    let omissions = payload["packet"]["omissions"].as_array().unwrap();
    let omission = |id: &str| omissions.iter().find(|entry| entry["id"] == id).unwrap();
    let populated = omission("blueprint:0");
    assert_eq!(populated["detailId"], "blueprint_timeout");
    assert_eq!(populated["stage"], "provider_fanout");
    assert!(populated.get("text").is_none());
    let empty = omission("blueprint:1");
    assert_eq!(empty["detailId"], "");
    assert_eq!(empty["stage"], "");
    let legacy = omission("blueprint:2");
    assert!(legacy.get("detailId").is_none());
    assert!(legacy.get("stage").is_none());
}

#[test]
fn native_ccs_conversion_preserves_optional_omission_shape() {
    let response = FederationResponseV1 {
        schema_version: 1,
        request_id: "request-omissions".into(),
        trace_id: "trace-omissions".into(),
        status: FederationStatus::Complete,
        providers: Vec::new(),
        candidates: Vec::new(),
        warnings: Vec::new(),
        omissions: vec![
            ProviderOmissionV1 {
                provider: ProviderId::Blueprint,
                reason: ReasonCode::ProviderUnavailable,
                candidate_id: Some("populated".into()),
                detail_id: Some("blueprint_timeout".into()),
                stage: Some("provider_fanout".into()),
            },
            ProviderOmissionV1 {
                provider: ProviderId::Cortex,
                reason: ReasonCode::ProviderTimeout,
                candidate_id: Some("empty".into()),
                detail_id: Some(String::new()),
                stage: Some(String::new()),
            },
            ProviderOmissionV1 {
                provider: ProviderId::Ledger,
                reason: ReasonCode::ProviderUnavailable,
                candidate_id: Some("legacy".into()),
                detail_id: None,
                stage: None,
            },
        ],
        diagnostics: None,
        error: None,
        extensions: BTreeMap::new(),
    };
    let request = FederationRequestV1 {
        schema_version: 1,
        request_id: "request-omissions".into(),
        trace_id: "trace-omissions".into(),
        task: "omission conversion".into(),
        repository_root: std::env::current_dir().unwrap().to_string_lossy().into_owned(),
        client: "test".into(),
        session_id: "session".into(),
        deadline_ms: 1_000,
        max_tokens: 100,
        anchors: Vec::new(),
        scope_grant_id: None,
        manifest_digest: None,
        release_generation: Some("release".into()),
        blueprint_generation: None,
        skills_generation: None,
        extensions: BTreeMap::new(),
    };
    let freshness = FreshnessSnapshotV1 {
        graph_state: "clean".into(),
        generation: Some("generation".into()),
        snapshot_id: Some("2026-09-08T00:00:00Z".into()),
        base_commit: None,
        overlay_digest: None,
        stale: false,
    };

    let ccs = membrane_runtime::pull::federation::native_response_to_ccs(
        &response,
        &request,
        &freshness,
    );
    let omission = |id: &str| {
        ccs["omissions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == id)
            .unwrap()
    };
    assert_eq!(omission("populated")["detailId"], "blueprint_timeout");
    assert_eq!(omission("populated")["stage"], "provider_fanout");
    assert_eq!(omission("empty")["detailId"], "");
    assert_eq!(omission("empty")["stage"], "");
    assert!(omission("legacy").get("detailId").is_none());
    assert!(omission("legacy").get("stage").is_none());
}

#[test]
fn resolver_only_evidence_requires_runtime_owned_negotiation_while_inline_fallback_survives() {
    let resolver_only = candidate("handle", "membrane_source_read:repo/file", "", "repo-a/file");
    let inline = candidate("inline", "membrane_source_read:repo/file", "faithful inline", "repo-b/file");
    let without = envelope_from_ccs(
        &ccs(vec![resolver_only.clone(), inline.clone()]).to_string(),
        EnvelopeInput { max_tokens: 100, packet_char_budget_override: None, packet_char_budget_model: None,
            accepted_receipt_versions: vec![2], scope_grant_present: false,
            consumer_resolvers: vec![], scope_grant_fence: None, gateway_process_ms: 0.0 },
    ).unwrap();
    let without_blocks = without["packet"]["blocks"].as_array().unwrap();
    assert_eq!(without_blocks.len(), 1);
    assert!(without_blocks.iter().all(|entry| entry["id"] != "handle"));
    assert!(without_blocks[0]["text"] == "faithful inline");
    let without_receipts = without["receipts"].as_array().unwrap();
    assert!(without_receipts.iter().any(|receipt| receipt["id"] == "handle" && receipt["reason"] == "consumer_resolver_unavailable"));

    let with = envelope_from_ccs(
        &ccs(vec![resolver_only, inline]).to_string(),
        EnvelopeInput { max_tokens: 100, packet_char_budget_override: None, packet_char_budget_model: None,
            accepted_receipt_versions: vec![2], scope_grant_present: false,
            consumer_resolvers: vec!["membrane_source_read".into()], scope_grant_fence: None, gateway_process_ms: 0.0 },
    ).unwrap();
    let blocks = with["packet"]["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 2);
    assert!(blocks.iter().any(|entry| entry["id"] == "handle"
        && entry["deliveryClass"] == "resolver_backed"
        && entry["sourceRef"] == "repo-a/file"));
    assert!(blocks.iter().any(|entry| entry["id"] == "inline"
        && entry["deliveryClass"] == "resolver_backed"
        && entry["sourceRef"] == "repo-b/file"));
}
