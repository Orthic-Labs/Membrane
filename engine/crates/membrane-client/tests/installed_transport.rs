//! LC-01/LC-05 transport-boundary regression for `InstalledResidentController`:
//! same-client deadline and stalled-transport behavior, receiver-gone/EOF
//! transport failures, and per-call bounded budgets. Each negative test
//! asserts the specific injected fault, not merely "some error".

use membrane_client::{
    AuthenticatedHolder, CallOptions, CancellationToken, ClientError, CompatibilityRequirement,
    HolderKind, InstalledResidentController, KnownCandidate, ResidentControllerBinding,
    ResidentHolderResponseV1, ResidentHolderStatusV1, ServiceIdentity, RESIDENT_HOLDER_SCHEMA_VERSION,
    bind_candidate,
};
use membrane_protocol::ResidentHolderOperationV1;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
        bind_candidate(candidate, identity, &CompatibilityRequirement::default()).unwrap(),
    )
    .unwrap()
}

fn holder() -> AuthenticatedHolder {
    AuthenticatedHolder { kind: HolderKind::CodeRightDaemon, holder_id: "coderight".into(), credential_id: "cred".into() }
}

fn ok_response(request: &membrane_protocol::ResidentHolderRequestV1) -> ResidentHolderResponseV1 {
    ResidentHolderResponseV1 {
        schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
        operation: request.operation,
        controller: request.controller.clone(),
        status: ResidentHolderStatusV1 {
            controller_active: true, services_ready: true, services_unavailable_reason: None,
            hub_holders: 0, coderight_daemon_holders: 1,
        },
        loss: None,
    }
}

/// A transport that has already passed its deadline before it ever returns
/// must be fenced by the post-call `options.check()`: the caller sees a
/// typed timeout, not a stale success treated as valid.
#[test]
fn same_client_deadline_fences_a_stalled_transport_response() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = attempts.clone();
    let client = InstalledResidentController::new(binding(), Arc::new(move |request: membrane_protocol::ResidentHolderRequestV1| {
        seen.fetch_add(1, Ordering::SeqCst);
        // Simulate a transport that stalls past the caller's own deadline
        // before finally returning a well-formed response.
        std::thread::sleep(Duration::from_millis(20));
        Ok::<_, ClientError>(ok_response(&request))
    }))
    .unwrap();

    let short_deadline = CallOptions::at(Instant::now() + Duration::from_millis(1), CancellationToken::new());
    let result = client.with_call_options(short_deadline).status(1);
    assert!(matches!(result, Err(ClientError::Timeout { .. })), "stalled transport past deadline must fail as Timeout, got {result:?}");
    assert_eq!(attempts.load(Ordering::SeqCst), 1, "the stalled call still happened exactly once; this is a post-hoc fence, not a retry");
}

/// The same client reused for two separate calls must apply each call's
/// own bounded budget independently — a long-elapsed first call must not
/// shrink or poison a fresh, generously-bounded second call.
#[test]
fn same_client_reused_across_calls_gets_independent_bounded_budgets() {
    let client = InstalledResidentController::new(binding(), Arc::new(|request: membrane_protocol::ResidentHolderRequestV1| {
        Ok::<_, ClientError>(ok_response(&request))
    }))
    .unwrap();

    let tight = CallOptions::at(Instant::now() + Duration::from_millis(1), CancellationToken::new());
    std::thread::sleep(Duration::from_millis(5));
    assert!(matches!(client.with_call_options(tight).status(1), Err(ClientError::Timeout { .. })));

    // A fresh call with its own generous deadline must succeed independently.
    let generous = CallOptions::after(Duration::from_secs(30));
    assert!(client.with_call_options(generous).status(2).is_ok());
}

/// "Receiver gone" (the peer end of the transport's channel has already
/// been dropped) must surface as the specific error the transport reports,
/// not be reinterpreted into a different typed error or silently ignored.
#[test]
fn receiver_gone_surfaces_as_the_specific_transport_error() {
    let client = InstalledResidentController::new(binding(), Arc::new(|_: membrane_protocol::ResidentHolderRequestV1| {
        Err::<ResidentHolderResponseV1, _>(ClientError::Internal { message: "receiver gone: channel closed".into() })
    }))
    .unwrap();
    let error = client.status(1).unwrap_err();
    match error {
        ClientError::Internal { message } => assert!(message.contains("receiver gone"), "expected the receiver-gone fault to propagate verbatim, got: {message}"),
        other => panic!("expected ClientError::Internal for receiver-gone, got {other:?}"),
    }
}

/// An EOF (unexpected end of stream) before any bytes of a response frame
/// arrive is a distinct fault from receiver-gone and must be distinguishable
/// as its own typed error, not coerced into a generic timeout or success.
#[test]
fn eof_before_any_response_frame_is_typed_and_not_treated_as_success() {
    let client = InstalledResidentController::new(binding(), Arc::new(|_: membrane_protocol::ResidentHolderRequestV1| {
        Err::<ResidentHolderResponseV1, _>(ClientError::Incompatible { message: "eof: transport closed before response frame".into() })
    }))
    .unwrap();
    let error = client.status(1).unwrap_err();
    match error {
        ClientError::Incompatible { message } => assert!(message.starts_with("eof:"), "expected the EOF fault to be preserved, got: {message}"),
        other => panic!("expected ClientError::Incompatible for EOF, got {other:?}"),
    }
}

/// A cancelled token must prevent transport entry entirely (this is the
/// "receiver gone before dispatch" boundary case): the specific fault is
/// that dispatch never happens, distinguishable from a transport-reported
/// receiver-gone that happens *after* dispatch.
#[test]
fn cancellation_before_dispatch_is_distinct_from_a_post_dispatch_transport_fault() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = attempts.clone();
    let client = InstalledResidentController::new(binding(), Arc::new(move |request: membrane_protocol::ResidentHolderRequestV1| {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok::<_, ClientError>(ok_response(&request))
    }))
    .unwrap();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let result = client.with_call_options(CallOptions::at(Instant::now() + Duration::from_secs(30), cancellation)).status(1);
    assert!(matches!(result, Err(ClientError::Cancelled)), "pre-dispatch cancellation must be Cancelled, not any other error, got {result:?}");
    assert_eq!(attempts.load(Ordering::SeqCst), 0, "cancellation before dispatch must never reach the transport");
}

/// Every acquire/renew/release route must reject an invalid expiry before
/// ever reaching the transport, and status/subscribe_loss must never
/// require one — this is the boundary the exact-vector HMAC/replay work
/// (owned by membrane-protocol operations.rs / membrane-runtime serve.rs,
/// outside this crate's slice) ultimately composes with.
#[test]
fn every_route_enforces_its_own_expiry_requirement_and_only_its_own() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = attempts.clone();
    let client = InstalledResidentController::new(binding(), Arc::new(move |request: membrane_protocol::ResidentHolderRequestV1| {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok::<_, ClientError>(ok_response(&request))
    }))
    .unwrap();

    assert!(matches!(client.acquire(holder(), 10, 10), Err(ClientError::InvalidRequest { .. })));
    assert!(matches!(client.renew(holder(), 10, 5), Err(ClientError::InvalidRequest { .. })));
    assert_eq!(attempts.load(Ordering::SeqCst), 0, "invalid-expiry routes must never dispatch");

    assert!(client.release(holder(), 10).is_ok());
    assert!(client.status(10).is_ok());
    assert_eq!(attempts.load(Ordering::SeqCst), 2, "release/status have no expiry requirement and must dispatch normally");
}

/// Route coverage sanity: every operation this crate's transport trait can
/// carry must round-trip the exact operation tag end to end, so a future
/// route addition cannot silently bypass the identity fence exercised in
/// `tests/residency.rs`.
#[test]
fn every_resident_holder_operation_round_trips_its_own_tag() {
    for operation in [
        ResidentHolderOperationV1::Acquire,
        ResidentHolderOperationV1::Renew,
        ResidentHolderOperationV1::Release,
        ResidentHolderOperationV1::Status,
        ResidentHolderOperationV1::SubscribeLoss,
    ] {
        let client = InstalledResidentController::new(binding(), Arc::new(move |request: membrane_protocol::ResidentHolderRequestV1| {
            assert_eq!(request.operation, operation);
            Ok::<_, ClientError>(ok_response(&request))
        }))
        .unwrap();
        let result = match operation {
            ResidentHolderOperationV1::Acquire => client.acquire(holder(), 1, 2).map(|_| ()),
            ResidentHolderOperationV1::Renew => client.renew(holder(), 1, 2).map(|_| ()),
            ResidentHolderOperationV1::Release => client.release(holder(), 1).map(|_| ()),
            ResidentHolderOperationV1::Status => client.status(1).map(|_| ()),
            ResidentHolderOperationV1::SubscribeLoss => client.subscribe_loss(1, 0).map(|_| ()),
        };
        assert!(result.is_ok(), "{operation:?} must round-trip cleanly, got {result:?}");
    }
}
