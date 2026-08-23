//! M8 acceptance for service-backed memory backend compatibility.

use membrane_client::{
    handshake::CompatibilityRequirement, CallOptions, CancellationToken, ClientError,
    MemoryBackendClient, MemoryTier,
};
use cortex_core::MemoryTier as EmbeddedTier;
use membrane_runtime::MemoryStore;
use serde_json::{json, Map, Value};
use std::time::Duration;
use std::sync::{Arc, Mutex};

fn row() -> Value {
    json!({"id":"m1","tier":"Working","content":"body","keywords":["k"],"score":0.5,"created_at":"now","access_count":1,"scope_id":"workspace","chars":4,"access":1,"inject":0})
}

fn response(operation: &str) -> Value {
    match operation {
        "/health" => json!({"serviceId":"membrane-acceptance","releaseGeneration":"r1","protocolVersion":1,"schemaVersion":1,"capabilities":[],"embedderDim":256}),
        "/activity" | "/metrics" => json!({"ok":true}),
        "/list" | "/search" => json!([row()]),
        "/scopes" => json!(["workspace"]),
        "/recall" => json!([{"entry":row(),"score":0.9}]),
        "/get" => json!({"id":"m1","content":"body","access_count":1}),
        "/put" => json!({"put":"m1"}),
        "/remember" => row(),
        "/remember_consolidated" => json!({"id":"m1"}),
        "/delete" => json!({"deleted":true}),
        "/use" => json!({"ok":true}),
        _ => json!({"error":"unsupported"}),
    }
}

fn client() -> MemoryBackendClient {
    MemoryBackendClient::new(Box::new(|operation: &str, _request: &Map<String, Value>| {
        Ok(response(operation))
    }) as Box<membrane_client::MemoryTransport>)
}

fn embedded_tier_name(tier: EmbeddedTier) -> &'static str {
    match tier {
        EmbeddedTier::Working => "Working",
        EmbeddedTier::Episodic => "Episodic",
        EmbeddedTier::Semantic => "Semantic",
    }
}

#[test]
fn handshake_and_all_backend_operations_share_typed_contract() {
    let client = client().bind(&CompatibilityRequirement::default()).unwrap();
    assert_eq!(client.identity().unwrap().service_id, "membrane-acceptance");
    let scope = vec!["workspace".to_owned()];
    client.activity_json(10).unwrap();
    client.metrics_json().unwrap();
    assert_eq!(client.embedder_dim().unwrap(), 256);
    client.delete("m1").unwrap();
    client.entries(10).unwrap();
    client.list(Some("workspace")).unwrap();
    client.search("body", 10).unwrap();
    client.scopes().unwrap();
    client.get_full("m1").unwrap();
    client.recall_scored("body", 10, &scope).unwrap();
    client.record_injections(&["m1".to_owned()]).unwrap();
    client.put("name", "body", "workspace", MemoryTier::Working).unwrap();
    client.try_put("name", "body", "workspace", MemoryTier::Working).unwrap();
    client.remember("body", vec!["k".into()]).unwrap();
    client
        .remember_consolidated("name", "body", vec!["k".into()], 0.5)
        .unwrap();
}

#[test]
fn incompatibility_failure_timeout_and_cancellation_are_explicit() {
    let base_client = client();
    let incompatible = MemoryBackendClient::new(Box::new(
        |operation: &str, _request: &Map<String, Value>| {
            Ok(if operation == "/health" {
                json!({"serviceId":"x","protocolVersion":2,"schemaVersion":1})
            } else {
                json!({"ok":true})
            })
        },
    ));
    assert!(matches!(
        incompatible.bind(&CompatibilityRequirement::default()),
        Err(ClientError::Incompatible { .. })
    ));
    let expired = client()
        .bind(&CompatibilityRequirement::default())
        .unwrap()
        .with_options(CallOptions::after(Duration::ZERO));
    assert!(matches!(expired.metrics_json(), Err(ClientError::Timeout { .. })));
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let cancelled = client()
        .bind(&CompatibilityRequirement::default())
        .unwrap()
        .with_options(CallOptions { deadline: std::time::Instant::now() + Duration::from_secs(1), cancellation });
    assert!(matches!(cancelled.metrics_json(), Err(ClientError::Cancelled)));
    let credentialed = base_client.with_bearer_token("secret");
    assert!(credentialed.has_bearer_token());
}

#[test]
fn embedded_and_service_share_one_typed_conformance_matrix() {
    let service = client().bind(&CompatibilityRequirement::default()).unwrap();
    let embedded = MemoryStore::new();
    let embedded_id = embedded
        .try_put("name", "body", "workspace", EmbeddedTier::Working)
        .unwrap();
    let embedded_rows = embedded.search("body", 10);
    let service_rows = service.search("body", 10).unwrap();
    assert_eq!(embedded_rows[0].content, service_rows[0].content);
    assert_eq!(embedded_rows[0].scope_id, service_rows[0].scope_id);
    assert_eq!(embedded_tier_name(embedded_rows[0].tier), service_rows[0].tier.as_str());

    let embedded_entries = embedded.entries(10);
    let service_entries = service.entries(10).unwrap();
    assert_eq!(embedded_entries[0].content, service_entries[0].content);
    assert_eq!(embedded.scopes(), service.scopes().unwrap());
    let embedded_list = embedded.list(Some("workspace"));
    let service_list = service.list(Some("workspace")).unwrap();
    assert_eq!(embedded_list.len(), service_list.len());
    assert_eq!(embedded_list[0].1, service_list[0].tier.as_str());
    assert_eq!(embedded_list[0].2, service_list[0].chars as i64);

    let embedded_recall = embedded.recall_scored("body", 10, &["workspace".into()]);
    let service_recall = service.recall_scored("body", 10, &["workspace".into()]).unwrap();
    assert_eq!(embedded_recall[0].0.content, service_recall[0].0.content);
    assert_eq!(embedded_recall[0].0.scope_id, service_recall[0].0.scope_id);
    assert!(embedded.delete(&embedded_id));
    assert!(service.delete("m1").unwrap());
}

#[test]
fn request_output_conformance_and_transport_failures_are_typed() {
    let seen = Arc::new(Mutex::new(Vec::<(String, Map<String, Value>)>::new()));
    let record = Arc::clone(&seen);
    let transport = Box::new(move |operation: &str, request: &Map<String, Value>| {
        record
            .lock()
            .unwrap()
            .push((operation.to_string(), request.clone()));
        Ok(response(operation))
    });
    let conformance = MemoryBackendClient::new(transport)
        .bind(&CompatibilityRequirement::default())
        .unwrap();
    conformance.search("body", 7).unwrap();
    conformance.put("name", "body", "workspace", MemoryTier::Working).unwrap();
    conformance.recall_scored("body", 3, &["workspace".into()]).unwrap();
    let calls = seen.lock().unwrap();
    let search = calls.iter().find(|(operation, _)| operation == "/search").unwrap();
    assert_eq!(search.1.get("query").and_then(Value::as_str), Some("body"));
    assert_eq!(search.1.get("limit").and_then(Value::as_u64), Some(7));
    let put = calls.iter().find(|(operation, _)| operation == "/put").unwrap();
    assert_eq!(put.1.get("tier").and_then(Value::as_str), Some("Working"));
    let failing = MemoryBackendClient::new(Box::new(|operation: &str, _request: &Map<String, Value>| {
        if operation == "/health" {
            Ok(response(operation))
        } else if operation == "/search" {
            Err(ClientError::protocol("transport_down", "fixture transport failed"))
        } else {
            Ok(json!("malformed"))
        }
    }))
    .bind(&CompatibilityRequirement::default())
    .unwrap();
    assert!(matches!(failing.search("body", 1), Err(ClientError::Protocol { code, .. }) if code == "transport_down"));
    assert!(matches!(failing.entries(1), Err(ClientError::Protocol { code, .. }) if code == "response_malformed"));
}
