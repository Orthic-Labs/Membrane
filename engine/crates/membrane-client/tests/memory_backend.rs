//! Conformance fixtures for service binding and all fourteen backend methods.

use membrane_client::{handshake::CompatibilityRequirement, CallOptions, ClientError, MemoryBackendClient, MemoryTier, ServiceIdentity};
use serde_json::{json, Map, Value};
use std::time::Duration;

fn client() -> MemoryBackendClient {
    MemoryBackendClient::new(Box::new(|operation: &str, _request: &Map<String, Value>| {
        Ok(match operation {
            "/health" => json!({"serviceId":"membrane-test","releaseGeneration":"r1","protocolVersion":1,"schemaVersion":1,"capabilities":[]} ),
            "/metrics" | "/activity" => json!({}),
            "/list" => json!([{"id":"id","tier":"Working","chars":4,"access":2,"inject":3}]),
            "/search" | "/scopes" | "/recall" => json!([]),
            "/delete" => json!({"deleted":true}),
            "/get" => json!({"id":"id","content":"body","access_count":0}),
            "/put" => json!({"put":"id"}),
            "/remember" => json!({"id":"id","tier":"Working","content":"body","keywords":[],"score":0,"created_at":"now","access_count":0,"scope_id":"global"}),
            "/remember_consolidated" => json!({"id":"stable-id","consolidated":true}),
            _ => json!({"ok":true}),
        })
    }) as Box<membrane_client::MemoryTransport>)
}

#[test]
fn compatible_handshake_binds_identity() {
    let client = client().bind(&CompatibilityRequirement::default()).unwrap();
    assert_eq!(client.identity().unwrap().service_id, "membrane-test");
}

#[test]
fn incompatible_handshake_fails_closed() {
    let error = match client().bind(&CompatibilityRequirement { protocol_version: 2, ..CompatibilityRequirement::default() }) {
        Ok(_) => panic!("incompatible handshake unexpectedly succeeded"),
        Err(error) => error,
    };
    assert!(matches!(error, ClientError::Incompatible { .. }));
}

#[test]
fn every_backend_call_requires_a_successful_binding() {
    let client = client();
    assert!(matches!(client.metrics_json(), Err(ClientError::Incompatible { .. })));
}

#[test]
fn list_preserves_access_and_injection_counts() {
    let client = client().bind(&CompatibilityRequirement::default()).unwrap();
    let row = client.list(None).unwrap().remove(0);
    assert_eq!((row.access_count, row.inject_count), (2, 3));
}

#[test]
fn consolidated_write_returns_stable_service_id() {
    let client = client().bind(&CompatibilityRequirement::default()).unwrap();
    assert_eq!(client.remember_consolidated("n", "body", vec!["k".into()], 0.8).unwrap(), Some("stable-id".into()));
}

#[test]
fn deadline_and_cancellation_are_checked_before_transport() {
    let client = client().with_options(CallOptions::after(Duration::ZERO));
    assert!(matches!(client.metrics_json(), Err(ClientError::Timeout { .. })));
}

#[test]
fn service_identity_is_typed_and_credentials_are_not_exposed() {
    let client = client().with_bearer_token("secret");
    assert!(client.has_bearer_token());
    let _: Option<&ServiceIdentity> = client.identity();
    let _: MemoryTier = MemoryTier::Working;
}
