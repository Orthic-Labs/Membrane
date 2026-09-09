use membrane_client::{
    AuthenticatedHolder, CallOptions, ClientError, HolderKind,
    bind_candidate, InstalledResidentController, ResidentControllerBinding, ResidentHolderResponseV1,
    CompatibilityRequirement, KnownCandidate, ServiceIdentity,
    ResidentHolderStatusV1, RESIDENT_HOLDER_SCHEMA_VERSION,
};
use membrane_protocol::ResidentHolderOperationV1;
use std::sync::Arc;

fn binding() -> ResidentControllerBinding {
    let candidate = KnownCandidate {
        stable_install_root: r"C:\Membrane\current".into(),
        endpoint: membrane_protocol::ResidentEndpointV1 { host: "127.0.0.1".into(), port: 47851 },
        expected_installation_id: Some("install-1".into()),
        expected_startup_generation: Some(1),
    };
    let identity = ServiceIdentity {
        service_id: "service-1".into(), installation_id: "install-1".into(),
        cortex_store_id: "store-1".into(), release_generation: "release-1".into(),
        startup_generation: 1, runtime_origin: "installed".into(),
        stable_install_root: Some(r"C:\Membrane\current".into()), protocol_version: 1,
        schema_version: 1, native_only: true, subsystems: vec![], capabilities: vec![],
    };
    ResidentControllerBinding::from_canonical(
        bind_candidate(candidate, identity, &CompatibilityRequirement::default()).unwrap()
    ).unwrap()
}

fn holder() -> AuthenticatedHolder {
    AuthenticatedHolder {
        kind: HolderKind::CodeRightDaemon,
        holder_id: "coderight".into(),
        credential_id: "authenticated-holder".into(),
    }
}

fn response(request: &membrane_protocol::ResidentHolderRequestV1) -> ResidentHolderResponseV1 {
    ResidentHolderResponseV1 {
        schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
        operation: request.operation,
        controller: request.controller.clone(),
        status: ResidentHolderStatusV1 { controller_active: true, services_ready: true,
            services_unavailable_reason: None, hub_holders: 0, coderight_daemon_holders: 1 },
        loss: None,
    }
}

#[test]
fn injected_transport_preserves_installed_identity_fence() {
    let client = InstalledResidentController::new(binding(), Arc::new(|request: membrane_protocol::ResidentHolderRequestV1| {
        Ok::<_, ClientError>(ResidentHolderResponseV1 {
            schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
            operation: request.operation,
            controller: request.controller,
            status: ResidentHolderStatusV1 {
                controller_active: true,
                services_ready: true,
                services_unavailable_reason: None,
                hub_holders: 0,
                coderight_daemon_holders: 1,
            },
            loss: None,
        })
    }))
    .unwrap();
    let response = client.acquire(holder(), 1, 2).unwrap();
    assert_eq!(response.operation, ResidentHolderOperationV1::Acquire);
    assert!(response.status.services_ready);
}

#[test]
fn cancelled_call_never_enters_transport() {
    let client = InstalledResidentController::new(binding(), Arc::new(|_: membrane_protocol::ResidentHolderRequestV1| {
        Err::<ResidentHolderResponseV1, _>(ClientError::Internal { message: "transport called".into() })
    }))
    .unwrap();
    let cancellation = membrane_client::CancellationToken::new();
    cancellation.cancel();
    assert!(matches!(
        client.with_call_options(CallOptions::at(std::time::Instant::now(), cancellation)).status(1),
        Err(ClientError::Cancelled)
    ));
}

#[test]
fn response_schema_operation_and_controller_are_fenced() {
    for kind in 0..3 {
        let client = InstalledResidentController::new(binding(), Arc::new(move |request: membrane_protocol::ResidentHolderRequestV1| {
            let mut result = response(&request);
            if kind == 0 { result.schema_version += 1; }
            if kind == 1 { result.operation = ResidentHolderOperationV1::Status; }
            if kind == 2 { result.controller.installation_id = "other".into(); }
            Ok::<_, ClientError>(result)
        })).unwrap();
        assert!(matches!(client.status(1), Err(ClientError::Incompatible { .. })));
    }
}

#[test]
fn loss_payload_must_match_subscribe_shape_and_controller() {
    let client = InstalledResidentController::new(binding(), Arc::new(|request: membrane_protocol::ResidentHolderRequestV1| {
        let mut result = response(&request);
        result.loss = Some(membrane_protocol::ResidentHolderLossV1 {
            sequence: 1, kind: membrane_protocol::ResidentHolderLossKindV1::FinalRelease,
            controller: { let mut c = request.controller; c.cortex_store_id = "other".into(); c },
        });
        Ok::<_, ClientError>(result)
    })).unwrap();
    assert!(matches!(client.subscribe_loss(1, 0), Err(ClientError::Incompatible { .. })));

    let client = InstalledResidentController::new(binding(), Arc::new(|request: membrane_protocol::ResidentHolderRequestV1| {
        let mut result = response(&request);
        result.loss = Some(membrane_protocol::ResidentHolderLossV1 {
            sequence: 1, kind: membrane_protocol::ResidentHolderLossKindV1::FinalRelease,
            controller: request.controller,
        });
        Ok::<_, ClientError>(result)
    })).unwrap();
    assert!(matches!(client.status(1), Err(ClientError::Incompatible { .. })));
}

#[test]
fn expiry_and_holder_fields_are_validated_before_transport() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = calls.clone();
    let client = InstalledResidentController::new(binding(), Arc::new(move |request: membrane_protocol::ResidentHolderRequestV1| {
        observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok::<_, ClientError>(response(&request))
    })).unwrap();
    assert!(matches!(client.acquire(holder(), 10, 10), Err(ClientError::InvalidRequest { .. })));
    let mut invalid = holder(); invalid.holder_id.clear();
    assert!(matches!(client.acquire(invalid, 1, 2), Err(ClientError::InvalidRequest { .. })));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}
