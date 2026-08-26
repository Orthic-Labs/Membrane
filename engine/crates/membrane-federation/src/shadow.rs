//! Side-effect-free differential execution for the migration qualification
//! harness.
//!
//! Shadow execution is deliberately outside the production federation path.
//! It gives legacy and native adapters one owned snapshot and one absolute
//! deadline, compares only canonical semantic fields, and returns the legacy
//! result as the sole authoritative output.  Adapters report effects through
//! [`ShadowEffects`]; a non-zero persistent or duplicate effect is rejected
//! before the legacy result is published.

use membrane_protocol::{canonical_json_of, digest_str, FederationRequestV1, FederationResponseV1};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

pub const SHADOW_SCHEMA_VERSION: u32 = 1;
pub const SHADOW_OPERATION: &str = "federation.shadow.v1";

/// Immutable source identity captured before either engine is invoked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShadowSourceSnapshot {
    name: String,
    identity: String,
    hash: String,
    generation: Option<String>,
}

impl ShadowSourceSnapshot {
    pub fn new(
        name: impl Into<String>,
        identity: impl Into<String>,
        hash: impl Into<String>,
        generation: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            identity: identity.into(),
            hash: hash.into(),
            generation,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn hash(&self) -> &str {
        &self.hash
    }
    pub fn generation(&self) -> Option<&str> {
        self.generation.as_deref()
    }
}

/// Read-only effect budget made available to adapters.  A shadow adapter may
/// simulate writes in memory, but it may not persist them or publish native
/// output to a user-visible channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShadowEffectPolicy {
    pub immutable_snapshot: bool,
    pub persistent_writes_allowed: u64,
    pub duplicate_effects_allowed: u64,
    pub native_output_allowed: bool,
}

impl Default for ShadowEffectPolicy {
    fn default() -> Self {
        Self {
            immutable_snapshot: true,
            persistent_writes_allowed: 0,
            duplicate_effects_allowed: 0,
            native_output_allowed: false,
        }
    }
}

/// Request and source state captured once for both engines.  Fields are
/// private so callers cannot mutate the snapshot after dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShadowSnapshot {
    request: FederationRequestV1,
    sources: Vec<ShadowSourceSnapshot>,
    effect_policy: ShadowEffectPolicy,
}

impl ShadowSnapshot {
    pub fn new(
        request: FederationRequestV1,
        mut sources: Vec<ShadowSourceSnapshot>,
    ) -> Result<Self, ShadowError> {
        let mut names = BTreeSet::new();
        for source in &sources {
            if source.name.trim().is_empty() || !names.insert(source.name.clone()) {
                return Err(ShadowError::InvalidSnapshot("source names must be unique"));
            }
        }
        sources.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Self {
            request,
            sources,
            effect_policy: ShadowEffectPolicy::default(),
        })
    }

    pub fn request(&self) -> &FederationRequestV1 {
        &self.request
    }
    pub fn sources(&self) -> &[ShadowSourceSnapshot] {
        &self.sources
    }
    pub fn effect_policy(&self) -> ShadowEffectPolicy {
        self.effect_policy
    }

    /// Digest only; request/source contents never enter a report directly.
    pub fn digest(&self) -> String {
        let material = ShadowSnapshotMaterial {
            request: canonical_json_of(&self.request),
            sources: self
                .sources
                .iter()
                .map(ShadowSourceMaterial::from)
                .collect(),
        };
        digest_str(&canonical_json_of(&material))
    }
}

/// A cancellation signal shared by both shadow adapters.  It is deliberately
/// independent of any executor so qualification can run on the same minimal
/// executor as production federation.
#[derive(Clone, Debug, Default)]
pub struct ShadowCancellation(Arc<ShadowCancellationState>);

#[derive(Debug, Default)]
struct ShadowCancellationState {
    cancelled: AtomicBool,
    wakers: Mutex<Vec<Waker>>,
}

impl ShadowCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        if self.0.cancelled.swap(true, Ordering::SeqCst) {
            return;
        }
        let wakers = self
            .0
            .wakers
            .lock()
            .map(|mut wakers| std::mem::take(&mut *wakers))
            .unwrap_or_default();
        for waker in wakers {
            waker.wake();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    fn register(&self, waker: &Waker) {
        if self.is_cancelled() {
            waker.wake_by_ref();
            return;
        }
        if let Ok(mut wakers) = self.0.wakers.lock() {
            if !wakers.iter().any(|existing| existing.will_wake(waker)) {
                wakers.push(waker.clone());
            }
        }
        if self.is_cancelled() {
            waker.wake_by_ref();
        }
    }
}

/// Read-only execution context supplied to one adapter invocation.  The only
/// effect capability is an in-memory recorder; no persistent sink, native
/// output channel, or mutable source handle is exposed to adapters.
#[derive(Clone, Debug)]
pub struct ShadowExecutionContext {
    snapshot: ShadowSnapshot,
    deadline: ShadowDeadline,
    cancellation: ShadowCancellation,
    effects: ShadowEffectRecorder,
}

impl ShadowExecutionContext {
    fn new(
        snapshot: ShadowSnapshot,
        deadline: ShadowDeadline,
        cancellation: ShadowCancellation,
    ) -> Self {
        Self {
            snapshot,
            deadline,
            cancellation,
            effects: ShadowEffectRecorder::default(),
        }
    }

    pub fn snapshot(&self) -> &ShadowSnapshot {
        &self.snapshot
    }
    pub fn deadline(&self) -> ShadowDeadline {
        self.deadline
    }
    pub fn cancellation(&self) -> &ShadowCancellation {
        &self.cancellation
    }
    pub fn effects(&self) -> ShadowEffectRecorder {
        self.effects.clone()
    }
}

/// In-memory, duplicate-aware effect capability for shadow adapters.
#[derive(Clone, Debug, Default)]
pub struct ShadowEffectRecorder {
    state: Arc<Mutex<ShadowEffectRecorderState>>,
}

#[derive(Debug, Default)]
struct ShadowEffectRecorderState {
    simulated_writes: BTreeSet<String>,
}

impl ShadowEffectRecorder {
    /// Record a simulated write.  Reusing a key is rejected instead of being
    /// silently counted as a second persistent effect.
    pub fn simulated_write(&self, key: impl Into<String>) -> Result<(), ShadowError> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err(ShadowError::InvalidEffect("write key must not be empty"));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| ShadowError::InvalidEffect("effect recorder poisoned"))?;
        if !state.simulated_writes.insert(key) {
            return Err(ShadowError::DuplicateEffect);
        }
        Ok(())
    }

    /// Shadow mode has no persistent-write capability.  Adapters must model
    /// writes through [`Self::simulated_write`] only.
    pub fn persistent_write(&self, _key: impl Into<String>) -> Result<(), ShadowError> {
        Err(ShadowError::PersistentWriteForbidden)
    }

    /// Native output is never user-visible in shadow mode.
    pub fn native_output(&self) -> Result<(), ShadowError> {
        Err(ShadowError::NativeOutputForbidden)
    }

    fn effects(&self) -> ShadowEffects {
        let simulated_writes = self
            .state
            .lock()
            .map(|state| state.simulated_writes.len() as u64)
            .unwrap_or(u64::MAX);
        ShadowEffects {
            simulated_writes,
            ..ShadowEffects::default()
        }
    }
}

#[derive(Serialize)]
struct ShadowSnapshotMaterial {
    request: String,
    sources: Vec<ShadowSourceMaterial>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShadowSourceMaterial {
    name: String,
    identity: String,
    hash: String,
    generation: Option<String>,
}

impl From<&ShadowSourceSnapshot> for ShadowSourceMaterial {
    fn from(source: &ShadowSourceSnapshot) -> Self {
        Self {
            name: source.name.clone(),
            identity: source.identity.clone(),
            hash: source.hash.clone(),
            generation: source.generation.clone(),
        }
    }
}

/// One monotonic deadline shared by legacy and native adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShadowDeadline {
    at: Instant,
    budget_ms: u64,
}

impl ShadowDeadline {
    pub fn after(duration: Duration) -> Self {
        let now = Instant::now();
        Self {
            at: now.checked_add(duration).unwrap_or(now),
            budget_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
        }
    }

    pub const fn at(at: Instant) -> Self {
        Self { at, budget_ms: 0 }
    }
    pub const fn instant(self) -> Instant {
        self.at
    }
    pub const fn budget_ms(self) -> u64 {
        self.budget_ms
    }
    pub fn is_exhausted(self) -> bool {
        Instant::now() >= self.at
    }
    pub fn remaining(self) -> Duration {
        self.at.saturating_duration_since(Instant::now())
    }
    pub fn remaining_ms(self) -> u64 {
        self.remaining().as_millis().min(u128::from(u64::MAX)) as u64
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShadowEffects {
    pub simulated_writes: u64,
    pub persistent_writes: u64,
    pub duplicate_effects: u64,
    pub native_user_visible_output: bool,
}

impl ShadowEffects {
    fn violates_policy(self, policy: ShadowEffectPolicy) -> bool {
        self.persistent_writes > policy.persistent_writes_allowed
            || self.duplicate_effects > policy.duplicate_effects_allowed
            || (!policy.native_output_allowed && self.native_user_visible_output)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShadowEngineOutput {
    pub response: FederationResponseV1,
    pub effects: ShadowEffects,
}

pub type ShadowFuture =
    Pin<Box<dyn Future<Output = Result<ShadowEngineOutput, ShadowError>> + Send>>;

/// Adapter boundary for legacy and native qualification implementations.
/// Both adapters receive a read-only context with one shared deadline and
/// cancellation signal.  Effects must be recorded through its in-memory
/// capability; direct effect declarations are checked against that ledger.
pub trait ShadowAdapter: Send + Sync {
    fn execute(&self, context: ShadowExecutionContext) -> ShadowFuture;
}

impl<F> ShadowAdapter for F
where
    F: Fn(ShadowExecutionContext) -> ShadowFuture + Send + Sync,
{
    fn execute(&self, context: ShadowExecutionContext) -> ShadowFuture {
        self(context)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DifferenceClassification {
    IntentionalVersionChange,
    BaselineDefect,
    Regression,
    Unexplained,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShadowDifference {
    pub path: String,
    pub classification: DifferenceClassification,
    pub legacy_hash: String,
    pub native_hash: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ShadowComparisonPolicy {
    expected: BTreeMap<String, DifferenceClassification>,
}

impl ShadowComparisonPolicy {
    pub fn expected(mut self, path: impl Into<String>, class: DifferenceClassification) -> Self {
        self.expected.insert(path.into(), class);
        self
    }

    pub fn classify(&self, path: &str) -> DifferenceClassification {
        self.expected
            .get(path)
            .copied()
            .unwrap_or(DifferenceClassification::Unexplained)
    }
}

/// A sealed, deterministic corpus of semantic expectations.  Qualification
/// callers may only use a manifest after verifying its digest; this prevents
/// a native result from rewriting its own expected classifications.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShadowFixture {
    name: String,
    expected: BTreeMap<String, DifferenceClassification>,
}

impl ShadowFixture {
    pub fn new(
        name: impl Into<String>,
        expected: BTreeMap<String, DifferenceClassification>,
    ) -> Result<Self, ShadowError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(ShadowError::InvalidSnapshot(
                "fixture name must not be empty",
            ));
        }
        Ok(Self { name, expected })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn policy(&self) -> ShadowComparisonPolicy {
        let mut policy = ShadowComparisonPolicy::default();
        for (path, class) in &self.expected {
            policy = policy.expected(path.clone(), *class);
        }
        policy
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShadowFixtureManifest {
    fixtures: Vec<ShadowFixture>,
    seal: String,
}

impl ShadowFixtureManifest {
    /// Seal fixture identity and expected classifications once, then retain
    /// only private data so an adapter cannot mutate qualification policy.
    pub fn sealed(mut fixtures: Vec<ShadowFixture>) -> Result<Self, ShadowError> {
        let mut names = BTreeSet::new();
        for fixture in &fixtures {
            if !names.insert(fixture.name.clone()) {
                return Err(ShadowError::InvalidSnapshot("fixture names must be unique"));
            }
        }
        fixtures.sort_by(|left, right| left.name.cmp(&right.name));
        let seal = digest_str(&canonical_json_of(&FixtureManifestMaterial {
            fixtures: &fixtures,
        }));
        Ok(Self { fixtures, seal })
    }

    pub fn seal(&self) -> &str {
        &self.seal
    }
    pub fn fixtures(&self) -> &[ShadowFixture] {
        &self.fixtures
    }
    pub fn verify_seal(&self, expected: &str) -> bool {
        self.seal == expected
    }
}

#[derive(Serialize)]
struct FixtureManifestMaterial<'a> {
    fixtures: &'a [ShadowFixture],
}

impl Serialize for ShadowFixture {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ShadowFixture", 2)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("expected", &self.expected)?;
        state.end()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShadowEngineTiming {
    pub elapsed_ms: u64,
    pub remaining_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShadowAccounting {
    pub outer_deadline_ms: u64,
    pub legacy: ShadowEngineTiming,
    pub native: ShadowEngineTiming,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShadowReport {
    pub schema_version: u32,
    pub operation: String,
    pub status: String,
    pub request_digest: String,
    pub snapshot_digest: String,
    pub legacy_output_digest: String,
    pub native_output_digest: String,
    pub differences: Vec<ShadowDifference>,
    pub legacy_effects: ShadowEffects,
    pub native_effects: ShadowEffects,
    pub accounting: ShadowAccounting,
}

impl ShadowReport {
    pub fn is_match(&self) -> bool {
        self.differences.is_empty()
    }
}

pub trait ShadowReceiptSink: Send + Sync {
    fn store(&self, report: &ShadowReport) -> Result<(), String>;
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum ShadowError {
    #[error("invalid immutable shadow snapshot: {0}")]
    InvalidSnapshot(&'static str),
    #[error("shadow deadline exhausted")]
    DeadlineExhausted,
    #[error("shadow execution cancelled")]
    Cancelled,
    #[error("legacy shadow adapter failed: {0}")]
    Legacy(String),
    #[error("native shadow adapter failed: {0}")]
    Native(String),
    #[error("shadow effect policy violated by {engine} adapter")]
    EffectViolation { engine: &'static str },
    #[error("shadow effect capability rejected persistent write")]
    PersistentWriteForbidden,
    #[error("shadow effect capability rejected native output")]
    NativeOutputForbidden,
    #[error("shadow effect key was recorded more than once")]
    DuplicateEffect,
    #[error("invalid shadow effect: {0}")]
    InvalidEffect(&'static str),
    #[error("shadow adapter output did not match its read-only effect ledger")]
    EffectLedgerMismatch,
    #[error("redacted shadow receipt could not be stored: {0}")]
    ReceiptSink(String),
    #[error("shadow semantic output could not be normalized: {0}")]
    Normalization(String),
}

pub struct ShadowResult {
    pub legacy_response: FederationResponseV1,
    pub report: ShadowReport,
}

struct GuardedAdapterFuture {
    inner: ShadowFuture,
    deadline: ShadowDeadline,
    cancellation: ShadowCancellation,
    timer_waker: Arc<Mutex<Option<Waker>>>,
    timer_started: bool,
}

impl GuardedAdapterFuture {
    fn new(
        inner: ShadowFuture,
        deadline: ShadowDeadline,
        cancellation: ShadowCancellation,
    ) -> Self {
        Self {
            inner,
            deadline,
            cancellation,
            timer_waker: Arc::new(Mutex::new(None)),
            timer_started: false,
        }
    }

    fn arm_timer(&mut self) {
        if self.timer_started {
            return;
        }
        self.timer_started = true;
        let timer_waker = Arc::clone(&self.timer_waker);
        let duration = self.deadline.remaining();
        std::thread::spawn(move || {
            std::thread::sleep(duration);
            if let Ok(mut waker) = timer_waker.lock() {
                if let Some(waker) = waker.take() {
                    waker.wake();
                }
            }
        });
    }
}

impl Future for GuardedAdapterFuture {
    type Output = Result<ShadowEngineOutput, ShadowError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.cancellation.is_cancelled() {
            return Poll::Ready(Err(ShadowError::Cancelled));
        }
        if self.deadline.is_exhausted() {
            self.cancellation.cancel();
            return Poll::Ready(Err(ShadowError::DeadlineExhausted));
        }
        self.cancellation.register(cx.waker());
        if let Ok(mut waker) = self.timer_waker.lock() {
            *waker = Some(cx.waker().clone());
        }
        self.arm_timer();
        match self.inner.as_mut().poll(cx) {
            Poll::Ready(result) => Poll::Ready(result),
            Poll::Pending => {
                if self.cancellation.is_cancelled() {
                    Poll::Ready(Err(ShadowError::Cancelled))
                } else if self.deadline.is_exhausted() {
                    self.cancellation.cancel();
                    Poll::Ready(Err(ShadowError::DeadlineExhausted))
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

fn guarded_adapter(
    adapter: &impl ShadowAdapter,
    context: ShadowExecutionContext,
) -> GuardedAdapterFuture {
    let deadline = context.deadline();
    let cancellation = context.cancellation().clone();
    GuardedAdapterFuture::new(adapter.execute(context), deadline, cancellation)
}

fn check_boundary(
    deadline: ShadowDeadline,
    cancellation: &ShadowCancellation,
) -> Result<(), ShadowError> {
    if cancellation.is_cancelled() {
        Err(ShadowError::Cancelled)
    } else if deadline.is_exhausted() {
        Err(ShadowError::DeadlineExhausted)
    } else {
        Ok(())
    }
}

fn validate_adapter_output(
    engine: &'static str,
    context: &ShadowExecutionContext,
    output: &ShadowEngineOutput,
    policy: ShadowEffectPolicy,
) -> Result<(), ShadowError> {
    if output.effects != context.effects.effects() {
        return Err(ShadowError::EffectLedgerMismatch);
    }
    if output.effects.violates_policy(policy) {
        return Err(ShadowError::EffectViolation { engine });
    }
    Ok(())
}

fn adapter_error(error: ShadowError, engine: &'static str) -> ShadowError {
    match error {
        ShadowError::DeadlineExhausted => ShadowError::DeadlineExhausted,
        ShadowError::Cancelled => ShadowError::Cancelled,
        other => match engine {
            "legacy" => ShadowError::Legacy(other.to_string()),
            _ => ShadowError::Native(other.to_string()),
        },
    }
}

/// Execute legacy first (authoritative), then native against the same frozen
/// snapshot.  Native output is retained only as a digest in the redacted
/// receipt and is never returned to the caller.
pub async fn execute_shadow<L, N, S>(
    legacy: &L,
    native: &N,
    snapshot: ShadowSnapshot,
    deadline: ShadowDeadline,
    policy: ShadowComparisonPolicy,
    sink: &S,
) -> Result<ShadowResult, ShadowError>
where
    L: ShadowAdapter,
    N: ShadowAdapter,
    S: ShadowReceiptSink,
{
    execute_shadow_with_cancellation(
        legacy,
        native,
        snapshot,
        deadline,
        ShadowCancellation::new(),
        policy,
        sink,
    )
    .await
}

/// Execute shadow qualification with a caller-owned cancellation signal.
/// Both adapter futures observe this same signal and the same absolute
/// deadline; no adapter receives a fresh timeout budget.
pub async fn execute_shadow_with_cancellation<L, N, S>(
    legacy: &L,
    native: &N,
    snapshot: ShadowSnapshot,
    deadline: ShadowDeadline,
    cancellation: ShadowCancellation,
    policy: ShadowComparisonPolicy,
    sink: &S,
) -> Result<ShadowResult, ShadowError>
where
    L: ShadowAdapter,
    N: ShadowAdapter,
    S: ShadowReceiptSink,
{
    let snapshot_digest = snapshot.digest();
    let request_digest = digest_str(&canonical_json_of(snapshot.request()));
    let effect_policy = snapshot.effect_policy();

    if !effect_policy.immutable_snapshot {
        return Err(ShadowError::InvalidSnapshot("mutable effect policy"));
    }
    check_boundary(deadline, &cancellation)?;
    let legacy_started = Instant::now();
    let legacy_context =
        ShadowExecutionContext::new(snapshot.clone(), deadline, cancellation.clone());
    let legacy_output = guarded_adapter(legacy, legacy_context.clone())
        .await
        .map_err(|error| adapter_error(error, "legacy"))?;
    let legacy_timing = ShadowEngineTiming {
        elapsed_ms: elapsed_ms(legacy_started),
        remaining_ms: deadline.remaining_ms(),
    };
    validate_adapter_output("legacy", &legacy_context, &legacy_output, effect_policy)
        .map_err(|error| ShadowError::Legacy(error.to_string()))?;
    check_boundary(deadline, &cancellation)?;

    let native_started = Instant::now();
    let native_context = ShadowExecutionContext::new(snapshot, deadline, cancellation.clone());
    let native_output = guarded_adapter(native, native_context.clone())
        .await
        .map_err(|error| adapter_error(error, "native"))?;
    let native_timing = ShadowEngineTiming {
        elapsed_ms: elapsed_ms(native_started),
        remaining_ms: deadline.remaining_ms(),
    };
    validate_adapter_output("native", &native_context, &native_output, effect_policy)
        .map_err(|error| ShadowError::Native(error.to_string()))?;
    // Do not normalize, persist, or publish a result after native crosses the
    // shared boundary, even if its future returned an otherwise valid value.
    check_boundary(deadline, &cancellation)?;

    let legacy_semantics = semantic_response(&legacy_output.response)?;
    let native_semantics = semantic_response(&native_output.response)?;
    let mut differences = Vec::new();
    collect_differences(
        "$",
        &legacy_semantics,
        &native_semantics,
        &policy,
        &mut differences,
    );
    differences.sort_by(|left, right| left.path.cmp(&right.path));
    let report = ShadowReport {
        schema_version: SHADOW_SCHEMA_VERSION,
        operation: SHADOW_OPERATION.to_owned(),
        status: if differences.is_empty() {
            "matched"
        } else {
            "differed"
        }
        .to_owned(),
        request_digest,
        snapshot_digest,
        legacy_output_digest: digest_str(&canonical_json_of(&legacy_semantics)),
        native_output_digest: digest_str(&canonical_json_of(&native_semantics)),
        differences,
        legacy_effects: legacy_output.effects,
        native_effects: native_output.effects,
        accounting: ShadowAccounting {
            outer_deadline_ms: deadline.budget_ms(),
            legacy: legacy_timing,
            native: native_timing,
        },
    };
    sink.store(&report).map_err(ShadowError::ReceiptSink)?;
    Ok(ShadowResult {
        legacy_response: legacy_output.response,
        report,
    })
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

/// Remove transport identity, diagnostics, timing, and free-form messages.
/// Candidate identity, provenance, trust, generation, omissions, and status
/// remain semantic fields.
fn semantic_response(response: &FederationResponseV1) -> Result<Value, ShadowError> {
    let mut response = response.clone();
    response.canonicalize_collections();
    let mut value = serde_json::to_value(&response)
        .map_err(|error| ShadowError::Normalization(error.to_string()))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| ShadowError::Normalization("response is not an object".to_owned()))?;
    object.insert(
        "requestId".to_owned(),
        Value::String("fixture-request".to_owned()),
    );
    object.insert(
        "traceId".to_owned(),
        Value::String("fixture-trace".to_owned()),
    );
    object.remove("diagnostics");
    strip_transport_keys(object);
    if let Some(providers) = object.get_mut("providers").and_then(Value::as_array_mut) {
        for provider in providers {
            if let Some(provider) = provider.as_object_mut() {
                provider.remove("diagnostics");
                strip_transport_keys(provider);
                redact_warnings(provider.get_mut("warnings"));
            }
        }
    }
    redact_warnings(object.get_mut("warnings"));
    if let Some(error) = object.get_mut("error").and_then(Value::as_object_mut) {
        error.remove("message");
    }
    Ok(value)
}

fn redact_warnings(value: Option<&mut Value>) {
    let Some(Value::Array(warnings)) = value else {
        return;
    };
    for warning in warnings {
        if let Some(warning) = warning.as_object_mut() {
            warning.remove("message");
        }
    }
}

fn strip_transport_keys(object: &mut Map<String, Value>) {
    for key in [
        "completionOrder",
        "schedulerCompletionOrder",
        "indexedAt",
        "elapsedMs",
        "stageElapsedMs",
        "providerElapsedMs",
        "providerStageElapsedMs",
        "worker",
        "transport",
        "mode",
    ] {
        object.remove(key);
    }
}

fn collect_differences(
    path: &str,
    legacy: &Value,
    native: &Value,
    policy: &ShadowComparisonPolicy,
    differences: &mut Vec<ShadowDifference>,
) {
    match (legacy, native) {
        (Value::Object(left), Value::Object(right)) => {
            let keys = left
                .keys()
                .chain(right.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            for key in keys {
                let child = format!("{path}.{}", key);
                match (left.get(&key), right.get(&key)) {
                    (Some(left), Some(right)) => {
                        collect_differences(&child, left, right, policy, differences)
                    }
                    (left, right) => push_difference(&child, left, right, policy, differences),
                }
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            for index in 0..left.len().max(right.len()) {
                let child = format!("{path}[{index}]");
                push_or_recurse(
                    child,
                    left.get(index),
                    right.get(index),
                    policy,
                    differences,
                );
            }
        }
        _ if legacy != native => {
            push_difference(path, Some(legacy), Some(native), policy, differences)
        }
        _ => {}
    }
}

fn push_or_recurse(
    path: String,
    legacy: Option<&Value>,
    native: Option<&Value>,
    policy: &ShadowComparisonPolicy,
    differences: &mut Vec<ShadowDifference>,
) {
    match (legacy, native) {
        (Some(left), Some(right)) => collect_differences(&path, left, right, policy, differences),
        (left, right) => push_difference(&path, left, right, policy, differences),
    }
}

fn push_difference(
    path: &str,
    legacy: Option<&Value>,
    native: Option<&Value>,
    policy: &ShadowComparisonPolicy,
    differences: &mut Vec<ShadowDifference>,
) {
    let hash = |value: Option<&Value>| {
        value
            .map(|value| canonical_json_of(value))
            .map(|text| digest_str(&text))
            .unwrap_or_else(|| digest_str("<missing>"))
    };
    differences.push(ShadowDifference {
        path: path.to_owned(),
        classification: policy.classify(path),
        legacy_hash: hash(legacy),
        native_hash: hash(native),
    });
}
