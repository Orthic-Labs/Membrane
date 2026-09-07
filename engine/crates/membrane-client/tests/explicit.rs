use membrane_client::{CallOptions, CancellationToken, ClientError, CompatibilityRequirement,
    KnownCandidate, MemoryBackendClient, MemoryTransport, ResidentEndpointV1};
use membrane_client::explicit::*;
use membrane_protocol::explicit::{ExplicitRequestV1, ExplicitResponseV1};
use serde_json::{json, Map, Value};
use std::{sync::{Arc, Mutex}, time::{Duration, Instant}};

fn binding() -> ExplicitOwnerBindingV1 {
    ExplicitOwnerBindingV1 { schema_version: 1, mode: ExplicitOwnerMode::BoundedExplicit,
        installation_id: "installation".into(), cortex_store_id: "store".into(), release_generation: "release".into(),
        startup_generation: 7, stable_install_root: "C:/installed/current".into(), protocol_version: 1,
        native_only: true, subsystems: ["pull", "push", "cortex", "blueprint", "ledger", "adapt"].map(str::to_owned).to_vec(),
        capabilities: vec!["memory".into(), "diagnostics".into(), "explicit-call".into()], embedder_dim: 384 }
}
fn candidate() -> KnownCandidate {
    KnownCandidate { stable_install_root: binding().stable_install_root,
        endpoint: ResidentEndpointV1 { host: "127.0.0.1".into(), port: 47851 },
        expected_installation_id: Some("installation".into()), expected_startup_generation: Some(7) }
}
fn response(binding: ExplicitOwnerBindingV1, status: u16, data: Value) -> Vec<u8> {
    serde_json::to_vec(&ExplicitResponseV1 { schema_version: 1, binding, status, data }).unwrap()
}
fn connect(transport: Arc<ExplicitOwnerTransport>) -> InstalledExplicitClient {
    InstalledExplicitClient::connect(candidate(), &CompatibilityRequirement::default(), transport,
        &CallOptions::after(Duration::from_secs(3))).unwrap()
}

#[test]
fn authentic_explicit_binding_has_no_resident_health_or_service_identity() {
    let wire = serde_json::to_value(binding()).unwrap();
    assert_eq!(wire["mode"], "bounded_explicit");
    assert!(wire.get("serviceId").is_none());
    assert!(wire.get("serviceGeneration").is_none());
    for field in ["installationId", "cortexStoreId", "releaseGeneration", "stableInstallRoot"] {
        let mut bad = wire.clone(); bad[field] = json!("");
        assert!(verify_explicit_binding(&candidate(), &serde_json::from_value(bad).unwrap(), &CompatibilityRequirement::default()).is_err());
    }
    let mut wrong = binding(); wrong.startup_generation += 1;
    assert!(verify_explicit_binding(&candidate(), &wrong, &CompatibilityRequirement::default()).is_err());
}

#[test]
fn typed_memory_methods_use_explicit_owner_and_request_options() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let output = seen.clone();
    let deadline = Instant::now() + Duration::from_secs(3);
    let client = connect(Arc::new(move |invocation, options| {
        assert_eq!(invocation.arguments, ["cli", "explicit-call"]);
        assert!(invocation.executable.ends_with(if cfg!(windows) { "membrane.exe" } else { "membrane" }));
        let request: ExplicitRequestV1 = serde_json::from_slice(&invocation.input).unwrap();
        if request.operation != ExplicitOperation::Binding {
            assert_eq!(options.deadline, deadline);
            assert_eq!(request.expected_binding, Some(binding()));
        }
        output.lock().unwrap().push(request.operation);
        Ok(response(binding(), 200, if request.operation == ExplicitOperation::ActivityRead { json!([]) } else { json!({}) }))
    }));
    let client = MemoryBackendClient::<MemoryTransport>::from_explicit(Arc::new(client));
    assert!(client.identity().is_none());
    assert_eq!(client.explicit_binding(), Some(&binding()));
    let scoped = client.with_call_options(CallOptions::at(deadline, CancellationToken::new()));
    assert_eq!(scoped.embedder_dim().unwrap(), 384);
    scoped.metrics_json().unwrap();
    client.with_options(CallOptions::at(deadline, CancellationToken::new())).activity_json(9).unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![ExplicitOperation::Binding, ExplicitOperation::Metrics, ExplicitOperation::ActivityRead]);
}

#[test]
fn unknown_dispatch_is_never_replayed_including_activity_and_accounting_reads() {
    for operation in [ExplicitOperation::Put, ExplicitOperation::Activity, ExplicitOperation::Get,
        ExplicitOperation::Search, ExplicitOperation::Recall, ExplicitOperation::Federate, ExplicitOperation::Diagnostic] {
        let attempts = Arc::new(Mutex::new(0)); let count = attempts.clone();
        let client = connect(Arc::new(move |invocation, _| {
            let request: ExplicitRequestV1 = serde_json::from_slice(&invocation.input).unwrap();
            if request.operation == ExplicitOperation::Binding { return Ok(response(binding(), 200, json!({}))); }
            *count.lock().unwrap() += 1;
            Err(ExplicitTransportFailure { error: ClientError::Cancelled, action_dispatched: true })
        }));
        let failure = client.call(operation, Map::new(), &CallOptions::after(Duration::from_secs(3))).unwrap_err();
        assert!(matches!(failure, ClientError::CommitUnknown { .. }), "{operation:?}: {failure}");
        assert!(!failure.retryable()); assert_eq!(*attempts.lock().unwrap(), 1);
    }
}

#[test]
fn pre_dispatch_cancellation_and_read_timeout_remain_typed() {
    let client = connect(Arc::new(|invocation, _| {
        let request: ExplicitRequestV1 = serde_json::from_slice(&invocation.input).unwrap();
        if request.operation == ExplicitOperation::Binding { return Ok(response(binding(), 200, json!({}))); }
        Err(ExplicitTransportFailure { error: ClientError::Timeout { message: "test deadline".into() }, action_dispatched: true })
    }));
    let cancelled = CancellationToken::new(); cancelled.cancel();
    assert!(matches!(client.call(ExplicitOperation::Put, Map::new(), &CallOptions::at(Instant::now() + Duration::from_secs(3), cancelled)), Err(ClientError::Cancelled)));
    assert!(matches!(client.call(ExplicitOperation::Metrics, Map::new(), &CallOptions::after(Duration::from_secs(3))), Err(ClientError::Timeout { .. })));
}

#[test]
fn changed_owner_refuses_before_mutation_and_malformed_success_is_unknown() {
    for malformed in [false, true] {
        let client = connect(Arc::new(move |invocation, _| {
            let request: ExplicitRequestV1 = serde_json::from_slice(&invocation.input).unwrap();
            if request.operation == ExplicitOperation::Binding { return Ok(response(binding(), 200, json!({}))); }
            if malformed { return Ok(b"invalid".to_vec()); }
            let mut rotated = binding(); rotated.startup_generation += 1;
            Ok(response(rotated, 409, json!({"code":"corrupt_or_rotation","error":"changed before dispatch"})))
        }));
        let result = client.call(ExplicitOperation::Put, Map::new(), &CallOptions::after(Duration::from_secs(3)));
        if malformed { assert!(matches!(result, Err(ClientError::CommitUnknown { .. }))); }
        else { assert!(matches!(result, Err(ClientError::CorruptOrRotation { .. }))); }
    }
}

#[test]
fn arbitrary_routes_and_resident_health_cannot_enter_explicit_dispatch() {
    for operation in ["/health", "/put", "activate", "../put"] {
        assert!(serde_json::from_value::<ExplicitRequestV1>(json!({"schemaVersion":1,"operation":operation,"expectedBinding":null,"request":{}})).is_err());
    }
}
