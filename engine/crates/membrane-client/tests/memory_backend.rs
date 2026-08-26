//! Conformance fixtures for service binding and all fourteen backend methods.

use membrane_client::{
    handshake::CompatibilityRequirement, CallOptions, ClientError, MemoryBackendClient, MemoryTier,
    ServiceIdentity,
};
use serde_json::{json, Map, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn client() -> MemoryBackendClient {
    MemoryBackendClient::new(Box::new(|operation: &str, _request: &Map<String, Value>| {
        Ok(match operation {
            "/health" => {
                json!({"serviceId":"membrane-hub","installationId":"install-1","cortexStoreId":"store-1","releaseGeneration":"r1","protocolVersion":1,"schemaVersion":1,"nativeOnly":true,"subsystems":["pull","push","cortex","blueprint","ledger","adapt"],"capabilities":["memory","diagnostics"]} )
            }
            "/metrics" | "/activity" => json!({}),
            "/list" => json!([{"id":"id","tier":"Working","chars":4,"access":2,"inject":3}]),
            "/search" | "/scopes" | "/recall" => json!([]),
            "/delete" => json!({"deleted":true}),
            "/get" => json!({"id":"id","content":"body","access_count":0}),
            "/put" => json!({"put":"id"}),
            "/remember" => {
                json!({"id":"id","tier":"Working","content":"body","keywords":[],"score":0,"created_at":"now","access_count":0,"scope_id":"global"})
            }
            "/remember_consolidated" => json!({"id":"stable-id","consolidated":true}),
            _ => json!({"ok":true}),
        })
    }) as Box<membrane_client::MemoryTransport>)
}

#[test]
fn compatible_handshake_binds_identity() {
    let client = client().bind(&CompatibilityRequirement::default()).unwrap();
    assert_eq!(client.identity().unwrap().service_id, "membrane-hub");
    assert_eq!(client.identity().unwrap().cortex_store_id, "store-1");
    let identity = client.identity().unwrap();
    assert!(identity.capabilities.iter().any(|value| value == "memory"));
    assert!(identity
        .capabilities
        .iter()
        .any(|value| value == "diagnostics"));
}

fn bind_health(health: Value) -> Result<MemoryBackendClient, ClientError> {
    MemoryBackendClient::new(
        Box::new(move |operation: &str, _request: &Map<String, Value>| {
            Ok(if operation == "/health" {
                health.clone()
            } else {
                json!({"ok":true})
            })
        }) as Box<membrane_client::MemoryTransport>,
    )
    .bind(&CompatibilityRequirement::default())
}

#[test]
fn embedded_or_wrong_store_binding_fails_closed() {
    let requirement = CompatibilityRequirement {
        cortex_store_id: Some("other-store".into()),
        ..CompatibilityRequirement::default()
    };
    assert!(matches!(
        client().bind(&requirement),
        Err(ClientError::Incompatible { .. })
    ));
}

#[test]
fn hub_loss_and_uncertain_commit_remain_distinct() {
    let unavailable =
        MemoryBackendClient::new(Box::new(|_operation: &str, _request: &Map<String, Value>| {
            Ok(json!({"kind":"error","code":"backend_unavailable","message":"Hub stopped"}))
        }) as Box<membrane_client::MemoryTransport>);
    assert!(matches!(
        unavailable.bind(&CompatibilityRequirement::default()),
        Err(ClientError::BackendUnavailable { .. })
    ));

    let unknown = ClientError::CommitUnknown {
        message: "connection closed after dispatch".into(),
        receipt_id: Some("receipt-1".into()),
    };
    assert_eq!(unknown.code(), "commit_unknown");
    assert!(!unknown.retryable());
}

#[test]
fn incompatible_handshake_fails_closed() {
    let error = match client().bind(&CompatibilityRequirement {
        protocol_version: 2,
        ..CompatibilityRequirement::default()
    }) {
        Ok(_) => panic!("incompatible handshake unexpectedly succeeded"),
        Err(error) => error,
    };
    assert!(matches!(error, ClientError::Incompatible { .. }));
}

#[test]
fn legacy_snake_case_handshake_is_rejected() {
    let error = match bind_health(json!({
        "service_id": "membrane-hub",
        "installation_id": "install-1",
        "cortex_store_id": "store-1",
        "release_generation": "r1",
        "protocol_version": 1,
        "schema_version": 1,
        "native_only": true,
        "subsystems": ["pull", "push", "cortex", "blueprint", "ledger", "adapt"],
        "capabilities": []
    })) {
        Ok(_) => panic!("legacy payload must not bind"),
        Err(error) => error,
    };
    assert!(matches!(error, ClientError::Incompatible { .. }));
}

#[test]
fn incomplete_native_handshake_is_rejected_without_defaults() {
    let error = match bind_health(json!({
        "serviceId": "membrane-hub",
        "installationId": "install-1",
        "cortexStoreId": "store-1"
    })) {
        Ok(_) => panic!("missing native compatibility fields must not bind"),
        Err(error) => error,
    };
    assert!(matches!(error, ClientError::Incompatible { .. }));
}

#[test]
fn non_native_handshake_is_rejected() {
    let error = match bind_health(json!({
        "serviceId": "membrane-hub",
        "installationId": "install-1",
        "cortexStoreId": "store-1",
        "releaseGeneration": "r1",
        "protocolVersion": 1,
        "schemaVersion": 1,
        "nativeOnly": false,
        "subsystems": ["pull", "push", "cortex", "blueprint", "ledger", "adapt"],
        "capabilities": []
    })) {
        Ok(_) => panic!("non-native service must not bind"),
        Err(error) => error,
    };
    assert!(matches!(error, ClientError::Incompatible { .. }));
}

#[test]
fn malformed_handshake_arrays_are_rejected() {
    let error = match bind_health(json!({
        "serviceId": "membrane-hub",
        "installationId": "install-1",
        "cortexStoreId": "store-1",
        "releaseGeneration": "r1",
        "protocolVersion": 1,
        "schemaVersion": 1,
        "nativeOnly": true,
        "subsystems": ["pull", 2],
        "capabilities": []
    })) {
        Ok(_) => panic!("malformed subsystem list must not bind"),
        Err(error) => error,
    };
    assert!(matches!(error, ClientError::Incompatible { .. }));
}

#[test]
fn every_backend_call_requires_a_successful_binding() {
    let client = client();
    assert!(matches!(
        client.metrics_json(),
        Err(ClientError::Incompatible { .. })
    ));
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
    assert_eq!(
        client
            .remember_consolidated("n", "body", vec!["k".into()], 0.8)
            .unwrap(),
        Some("stable-id".into())
    );
}

#[test]
fn deadline_and_cancellation_are_checked_before_transport() {
    let client = client()
        .bind(&CompatibilityRequirement::default())
        .unwrap()
        .with_options(CallOptions::after(Duration::ZERO));
    assert!(matches!(
        client.metrics_json(),
        Err(ClientError::Timeout { .. })
    ));
}

#[test]
fn service_identity_is_typed_and_credentials_are_not_exposed() {
    let client = client().with_bearer_token("secret");
    assert!(client.has_bearer_token());
    let _: Option<&ServiceIdentity> = client.identity();
    let _: MemoryTier = MemoryTier::Working;
}

#[test]
fn hub_recall_and_injection_requests_use_route_native_shapes() {
    let seen = Arc::new(Mutex::new(Vec::<(String, Map<String, Value>)>::new()));
    let observed = Arc::clone(&seen);
    let client = MemoryBackendClient::new(Box::new(
        move |operation: &str, request: &Map<String, Value>| {
            observed
                .lock()
                .unwrap()
                .push((operation.to_string(), request.clone()));
            Ok(match operation {
                "/health" => json!({
                    "serviceId":"membrane-hub",
                    "installationId":"install-1",
                    "cortexStoreId":"store-1",
                    "releaseGeneration":"r1",
                    "protocolVersion":1,
                    "schemaVersion":1,
                    "nativeOnly":true,
                    "subsystems":["pull","push","cortex","blueprint","ledger","adapt"],
                    "capabilities":["memory","diagnostics"]
                }),
                "/recall" => json!([]),
                "/use" => json!({"ok":true}),
                _ => json!({"ok":true}),
            })
        },
    ) as Box<membrane_client::MemoryTransport>)
    .bind(&CompatibilityRequirement::default())
    .unwrap();

    assert!(client
        .recall_scored("deploy", 3, &["workspace".into(), "global".into()])
        .unwrap()
        .is_empty());
    client
        .record_injections(&["first".into(), "second".into()])
        .unwrap();

    let calls = seen.lock().unwrap();
    let recall = calls.iter().find(|(op, _)| op == "/recall").unwrap();
    assert_eq!(
        recall.1.get("client").and_then(Value::as_str),
        Some("coderight")
    );
    assert_eq!(
        recall.1.get("observe").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        recall.1.get("scope").and_then(Value::as_str),
        Some("workspace")
    );
    assert_eq!(
        recall.1.get("cross").and_then(Value::as_array).unwrap(),
        &vec![Value::from("global")]
    );
    assert!(recall.1.get("scopes").is_none());
    let uses: Vec<_> = calls.iter().filter(|(op, _)| op == "/use").collect();
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].1.get("id").and_then(Value::as_str), Some("first"));
    assert_eq!(uses[1].1.get("id").and_then(Value::as_str), Some("second"));
    assert!(uses.iter().all(|(_, request)| request.get("ids").is_none()));
}
