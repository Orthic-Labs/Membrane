use membrane_federation::shadow::{
    execute_shadow, execute_shadow_with_cancellation, DifferenceClassification, ShadowAdapter,
    ShadowCancellation, ShadowComparisonPolicy, ShadowDeadline, ShadowEffects, ShadowEngineOutput,
    ShadowError, ShadowExecutionContext, ShadowFixture, ShadowFixtureManifest, ShadowFuture,
    ShadowReceiptSink, ShadowReport, ShadowSnapshot, ShadowSourceSnapshot,
};
use membrane_protocol::{
    FederationRequestV1, FederationResponseV1, FederationStatus, FEDERATION_REQUEST_SCHEMA_VERSION,
    FEDERATION_RESPONSE_SCHEMA_VERSION,
};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::time::Duration;
fn request() -> FederationRequestV1 {
    FederationRequestV1 {
        schema_version: FEDERATION_REQUEST_SCHEMA_VERSION,
        request_id: "legacy-request".into(),
        trace_id: "legacy-trace".into(),
        task: "shadow fixture".into(),
        repository_root: "/fixture/repository".into(),
        client: "test".into(),
        session_id: "session".into(),
        deadline_ms: 500,
        max_tokens: 100,
        anchors: Vec::new(),
        scope_grant_id: None,
        manifest_digest: None,
        release_generation: None,
        blueprint_generation: None,
        skills_generation: None,
        extensions: BTreeMap::new(),
    }
}
fn response(status: FederationStatus, trace: &str) -> FederationResponseV1 {
    FederationResponseV1 {
        schema_version: FEDERATION_RESPONSE_SCHEMA_VERSION,
        request_id: trace.into(),
        trace_id: trace.into(),
        status,
        providers: Vec::new(),
        candidates: Vec::new(),
        warnings: Vec::new(),
        omissions: Vec::new(),
        diagnostics: None,
        error: None,
        extensions: BTreeMap::new(),
    }
}
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<ShadowReport>>>);

impl ShadowReceiptSink for Sink {
    fn store(&self, report: &ShadowReport) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "poisoned".to_owned())?
            .push(report.clone());
        Ok(())
    }
}
fn adapter(response: FederationResponseV1) -> impl ShadowAdapter {
    move |_context: ShadowExecutionContext| -> ShadowFuture {
        let response = response.clone();
        Box::pin(async move {
            Ok(ShadowEngineOutput {
                response,
                effects: ShadowEffects::default(),
            })
        })
    }
}
fn block_on<F: Future>(mut future: F) -> F::Output {
    unsafe fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut context = Context::from_waker(&waker);
    let mut future = unsafe { Pin::new_unchecked(&mut future) };
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
#[test]
fn shadow_returns_legacy_and_records_redacted_semantic_difference() {
    let snapshot = ShadowSnapshot::new(
        request(),
        vec![ShadowSourceSnapshot::new(
            "fixture-cortex",
            "memory:fixture",
            "sha256:source",
            Some("sha256:generation".into()),
        )],
    )
    .unwrap();
    let sink = Sink::default();
    let result = block_on(execute_shadow(
        &adapter(response(FederationStatus::Complete, "legacy")),
        &adapter(response(FederationStatus::Partial, "native")),
        snapshot,
        ShadowDeadline::after(Duration::from_secs(1)),
        ShadowComparisonPolicy::default().expected(
            "$.status",
            DifferenceClassification::IntentionalVersionChange,
        ),
        &sink,
    ))
    .unwrap();
    assert_eq!(result.legacy_response.status, FederationStatus::Complete);
    assert_eq!(result.report.status, "differed");
    assert_eq!(
        result.report.differences[0].classification,
        DifferenceClassification::IntentionalVersionChange
    );
    assert_eq!(sink.0.lock().unwrap().len(), 1);
    assert!(result.report.legacy_output_digest.starts_with("sha256:"));
}
#[test]
fn shadow_normalizes_transport_identity_and_never_allows_effects() {
    let snapshot = ShadowSnapshot::new(request(), Vec::new()).unwrap();
    let sink = Sink::default();
    let result = block_on(execute_shadow(
        &adapter(response(FederationStatus::Complete, "legacy")),
        &adapter(response(FederationStatus::Complete, "native")),
        snapshot,
        ShadowDeadline::after(Duration::from_secs(1)),
        ShadowComparisonPolicy::default(),
        &sink,
    ))
    .unwrap();
    assert!(result.report.is_match());
    assert_eq!(result.report.legacy_effects.persistent_writes, 0);
    assert_eq!(result.report.native_effects.duplicate_effects, 0);
}
#[test]
fn shadow_rejects_persistent_or_duplicate_effects() {
    let snapshot = ShadowSnapshot::new(request(), Vec::new()).unwrap();
    let sink = Sink::default();
    let effectful = |_context: ShadowExecutionContext| -> ShadowFuture {
        Box::pin(async {
            Ok(ShadowEngineOutput {
                response: response(FederationStatus::Complete, "native"),
                effects: ShadowEffects {
                    persistent_writes: 1,
                    ..ShadowEffects::default()
                },
            })
        })
    };
    let result = block_on(execute_shadow(
        &adapter(response(FederationStatus::Complete, "legacy")),
        &effectful,
        snapshot,
        ShadowDeadline::after(Duration::from_secs(1)),
        ShadowComparisonPolicy::default(),
        &sink,
    ));
    assert!(result.is_err());
    assert!(sink.0.lock().unwrap().is_empty());
}
#[test]
fn shadow_capability_rejects_persistent_duplicate_and_native_effects() {
    let snapshot = ShadowSnapshot::new(request(), Vec::new()).unwrap();
    let sink = Sink::default();
    let effectful = |context: ShadowExecutionContext| -> ShadowFuture {
        let effects = context.effects();
        Box::pin(async move {
            effects.simulated_write("fixture:one").unwrap();
            assert_eq!(
                effects.simulated_write("fixture:one"),
                Err(ShadowError::DuplicateEffect)
            );
            assert_eq!(
                effects.persistent_write("fixture:one"),
                Err(ShadowError::PersistentWriteForbidden)
            );
            assert_eq!(
                effects.native_output(),
                Err(ShadowError::NativeOutputForbidden)
            );
            Ok(ShadowEngineOutput {
                response: response(FederationStatus::Complete, "legacy"),
                effects: ShadowEffects {
                    simulated_writes: 1,
                    ..ShadowEffects::default()
                },
            })
        })
    };
    let result = block_on(execute_shadow(
        &effectful,
        &adapter(response(FederationStatus::Complete, "native")),
        snapshot,
        ShadowDeadline::after(Duration::from_secs(1)),
        ShadowComparisonPolicy::default(),
        &sink,
    ))
    .unwrap();
    assert_eq!(result.legacy_response.request_id, "legacy");
    assert_eq!(sink.0.lock().unwrap().len(), 1);
}
#[test]
fn shadow_native_output_is_never_authoritative() {
    let snapshot = ShadowSnapshot::new(request(), Vec::new()).unwrap();
    let sink = Sink::default();
    let result = block_on(execute_shadow(
        &adapter(response(FederationStatus::Complete, "legacy")),
        &adapter(response(FederationStatus::Partial, "native")),
        snapshot,
        ShadowDeadline::after(Duration::from_secs(1)),
        ShadowComparisonPolicy::default(),
        &sink,
    ))
    .unwrap();
    assert_eq!(result.legacy_response.request_id, "legacy");
    assert_ne!(
        result.report.legacy_output_digest,
        result.report.native_output_digest
    );
}
#[test]
fn shadow_common_cancellation_reaches_both_adapter_contexts() {
    let snapshot = ShadowSnapshot::new(request(), Vec::new()).unwrap();
    let cancellation = ShadowCancellation::new();
    cancellation.cancel();
    let sink = Sink::default();
    let called = Arc::new(Mutex::new(false));
    let called_ref = Arc::clone(&called);
    let adapter = move |context: ShadowExecutionContext| -> ShadowFuture {
        *called_ref.lock().unwrap() = true;
        assert!(context.cancellation().is_cancelled());
        Box::pin(async {
            Ok(ShadowEngineOutput {
                response: response(FederationStatus::Complete, "cancelled"),
                effects: ShadowEffects::default(),
            })
        })
    };
    let result = block_on(execute_shadow_with_cancellation(
        &adapter,
        &adapter,
        snapshot,
        ShadowDeadline::after(Duration::from_secs(1)),
        cancellation,
        ShadowComparisonPolicy::default(),
        &sink,
    ));
    assert!(matches!(result, Err(ShadowError::Cancelled)));
    assert!(!*called.lock().unwrap());
}
#[test]
fn shadow_deadline_is_enforced_around_pending_adapter_future() {
    let snapshot = ShadowSnapshot::new(request(), Vec::new()).unwrap();
    let sink = Sink::default();
    let pending =
        |_context: ShadowExecutionContext| -> ShadowFuture { Box::pin(std::future::pending()) };
    let result = block_on(execute_shadow(
        &pending,
        &adapter(response(FederationStatus::Complete, "native")),
        snapshot,
        ShadowDeadline::after(Duration::from_millis(1)),
        ShadowComparisonPolicy::default(),
        &sink,
    ));
    assert!(matches!(result, Err(ShadowError::DeadlineExhausted)));
    assert!(sink.0.lock().unwrap().is_empty());
}
#[test]
fn shadow_fixture_manifest_is_sealed_and_classification_is_stable() {
    let mut intentional = BTreeMap::new();
    intentional.insert(
        "$.status".to_owned(),
        DifferenceClassification::IntentionalVersionChange,
    );
    let mut regression = BTreeMap::new();
    regression.insert(
        "$.providers[0].trust".to_owned(),
        DifferenceClassification::Regression,
    );
    let manifest = ShadowFixtureManifest::sealed(vec![
        ShadowFixture::new("status-change", intentional).unwrap(),
        ShadowFixture::new("provider-trust", regression).unwrap(),
    ])
    .unwrap();
    let seal = manifest.seal().to_owned();
    assert!(seal.starts_with("sha256:"));
    assert!(manifest.verify_seal(&seal));
    assert_eq!(manifest.fixtures()[0].name(), "provider-trust");
    assert_eq!(
        manifest.fixtures()[0]
            .policy()
            .classify("$.providers[0].trust"),
        DifferenceClassification::Regression
    );
    assert_eq!(
        manifest.fixtures()[1].policy().classify("$.status"),
        DifferenceClassification::IntentionalVersionChange
    );
}
