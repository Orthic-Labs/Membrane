//! Native FederationEngine composition used by Membrane runtime.
//!
//! The engine is built once per runtime composition from typed owner source
//! handles.  It returns candidates only; planner admission remains the next
//! step in [`super::super::federation`].

use super::federation_sources::{NativeSourceBindings, RuntimeDeliveryLedger, RuntimeRuleSource};
use crate::pull::metrics::{FederationMetricStatus, FederationMetrics};
use membrane_federation::providers::{
    anchors::AnchorsProvider, architect::ArchitectProvider, audit::AuditProvider,
    blueprint::BlueprintProvider, cortex::CortexProvider, git::GitProvider,
    live_files::LiveFilesProvider, rules::RulesProvider, skills::SkillsProvider,
};
use membrane_federation::{FederationConfig, FederationEngine, ProviderRegistry};
use membrane_protocol::{FederationRequestV1, FederationResponseV1, ProviderId};
use membrane_provider_sdk::{
    FreshnessSource, Provider, ProviderContext, ProviderError, ProviderOutput,
    ProviderRegistration, SourceQuery,
};
use std::future::Future;
use std::path::Path;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

// Provider lanes are admitted concurrently (ten lanes in V1), while their
// owner-backed work is moved to Tokio's bounded blocking pool. Keep a small
// async scheduling pool for timers, cancellation, and result assembly.
const NATIVE_FEDERATION_ASYNC_WORKERS: usize = 2;
const NATIVE_FEDERATION_BLOCKING_WORKERS: usize = 10;

pub(crate) fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(NATIVE_FEDERATION_ASYNC_WORKERS)
        .max_blocking_threads(NATIVE_FEDERATION_BLOCKING_WORKERS)
        .enable_all()
        .build()
        .map_err(|error| format!("create native federation runtime: {error}"))
}

/// Native engine plus immutable source handles.  No Python gateway, worker,
/// stdio framing, or dynamic provider lookup is reachable from this type.
pub struct NativeFederation {
    engine: FederationEngine,
    metrics: Arc<FederationMetrics>,
    freshness: Arc<dyn FreshnessSource>,
    last_freshness: Arc<Mutex<Option<membrane_protocol::FreshnessSnapshotV1>>>,
    cancellations: Arc<Mutex<HashMap<String, CancellationToken>>>,
    temporal_queries: Arc<Mutex<HashMap<String, cortex_store::TemporalFactQuery>>>,
}

impl std::fmt::Debug for NativeFederation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeFederation")
            .field("engine", &self.engine)
            .field("freshness", &"injected")
            .finish()
    }
}

impl NativeFederation {
    pub fn new(bindings: NativeSourceBindings) -> Result<Self, String> {
        let cancellations = bindings.cancellations.clone();
        let temporal_queries = bindings.temporal_queries.clone();
        let blueprint = bindings
            .blueprint
            .clone()
            .ok_or_else(|| "native Blueprint source unavailable".to_owned())?;
        let contextual = bindings
            .blueprint_contextual
            .clone()
            .ok_or_else(|| "native Blueprint contextual source unavailable".to_owned())?;
        let freshness = bindings
            .freshness
            .clone()
            .ok_or_else(|| "native freshness source unavailable".to_owned())?;
        let release = bindings
            .release
            .ok_or_else(|| "native release source unavailable".to_owned())?;
        let blueprint_provider: Arc<dyn membrane_provider_sdk::Provider> = Arc::new(
            BlueprintProvider::with_contextual_source_pair(blueprint, contextual.clone()),
        );
        let anchors_provider = AnchorsProvider::default().with_blueprint_source(contextual);
        let rules = Arc::new(RuntimeRuleSource);
        let ledger = Arc::new(RuntimeDeliveryLedger::default());
        let providers = vec![
            registration(
                ProviderId::Anchors,
                "native.anchors",
                Arc::new(anchors_provider),
                vec![ProviderId::Blueprint],
            ),
            registration(
                ProviderId::Blueprint,
                "native.blueprint",
                blueprint_provider,
                vec![],
            ),
            registration(
                ProviderId::Rules,
                "native.rules",
                Arc::new(RulesProvider::new(rules, ledger)),
                vec![],
            ),
            registration(
                ProviderId::LiveFiles,
                "native.live_files",
                Arc::new(LiveFilesProvider::default()),
                vec![],
            ),
            registration(ProviderId::Git, "native.git", Arc::new(GitProvider), vec![]),
            registration(
                ProviderId::Audit,
                "native.audit",
                Arc::new(AuditProvider::new()),
                vec![],
            ),
            registration(
                ProviderId::Architect,
                "native.architect",
                Arc::new(ArchitectProvider::new()),
                vec![],
            ),
            registration(
                ProviderId::Skills,
                "native.skills",
                Arc::new(SkillsProvider::new(
                    bindings
                        .skills
                        .clone()
                        .ok_or_else(|| "native skills source unavailable".to_owned())?,
                )),
                vec![],
            ),
            registration(
                ProviderId::Cortex,
                "native.cortex",
                Arc::new(CortexProvider::new()),
                vec![],
            ),
            registration(
                ProviderId::Ledger,
                "native.ledger",
                Arc::new(crate::ledger::provider::LedgerProvider::new(bindings.ledger.clone())),
                vec![],
            ),
        ];
        let registry = ProviderRegistry::new(providers).map_err(|e| e.to_string())?;
        let engine = FederationEngine::with_release_source(
            registry,
            FederationConfig::all_enabled(),
            bindings.source_set(),
            release,
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            engine,
            metrics: Arc::new(FederationMetrics::new()),
            freshness,
            last_freshness: Arc::new(Mutex::new(None)),
            cancellations,
            temporal_queries,
        })
    }

    pub fn metrics_snapshot(&self) -> std::collections::BTreeMap<String, u64> {
        self.metrics.snapshot()
    }

    pub fn freshness_snapshot(&self) -> Option<membrane_protocol::FreshnessSnapshotV1> {
        self.last_freshness
            .lock()
            .ok()
            .and_then(|snapshot| snapshot.clone())
    }

    pub async fn federate(
        &self,
        request: &FederationRequestV1,
        cancellation: CancellationToken,
    ) -> Result<FederationResponseV1, String> {
        let now = std::time::Instant::now();
        let deadline = membrane_federation::deadline::Deadline::at(now.checked_add(
            std::time::Duration::from_millis(request.deadline_ms)).unwrap_or(now));
        self.federate_until(request, cancellation, deadline).await
    }

    pub async fn federate_until(
        &self,
        request: &FederationRequestV1,
        cancellation: CancellationToken,
        deadline: membrane_federation::deadline::Deadline,
    ) -> Result<FederationResponseV1, String> {
        if deadline.is_exhausted_at(std::time::Instant::now()) {
            return Err("federation deadline exhausted during owner binding".to_owned());
        }
        let cancelled = cancellation.is_cancelled();
        let temporal_query = request
            .extensions
            .get("cortexTemporalQuery")
            .cloned()
            .map(|value| {
                serde_json::from_value::<cortex_store::TemporalFactQuery>(value)
                    .map_err(|error| format!("invalid cortexTemporalQuery: {error}"))
            })
            .transpose()?;
        let query = SourceQuery {
            request_id: request.request_id.clone(),
            repository_id: membrane_federation::root::canonical_repository_id(Path::new(
                &request.repository_root,
            )),
            repository_root: request.repository_root.clone(),
            task: request.task.clone(),
            session_id: request.session_id.clone(),
            generation: request
                .release_generation
                .clone()
                .or_else(|| request.blueprint_generation.clone()),
            anchors: request.anchors.clone(),
        };
        let freshness = tokio::time::timeout_at(deadline.instant().into(), self.freshness.freshness(&query))
            .await.map_err(|_| "federation deadline exhausted during owner binding".to_owned())?;
        if deadline.is_exhausted_at(std::time::Instant::now()) {
            return Err("federation deadline exhausted during owner binding".to_owned());
        }
        match freshness {
            Ok(snapshot) => {
                let stale = snapshot.value.stale;
                if let Ok(mut current) = self.last_freshness.lock() {
                    *current = Some(snapshot.value);
                }
                if stale {
                    self.metrics.record(FederationMetricStatus::Stale);
                }
            }
            Err(error) => {
                let status = error_status(&error.to_string(), cancelled);
                self.metrics.record(status);
                return Err(error.to_string());
            }
        }
        if let Ok(mut tokens) = self.cancellations.lock() {
            tokens.insert(request.request_id.clone(), cancellation.clone());
        }
        if let Some(temporal_query) = temporal_query {
            if let Ok(mut queries) = self.temporal_queries.lock() {
                queries.insert(request.request_id.clone(), temporal_query);
            }
        }
        let response = self.engine.federate_until(request, cancellation, deadline).await;
        if let Ok(mut tokens) = self.cancellations.lock() {
            tokens.remove(&request.request_id);
        }
        if let Ok(mut queries) = self.temporal_queries.lock() {
            queries.remove(&request.request_id);
        }
        match &response {
            Ok(value) => self.metrics.record(metric_status(value, cancelled)),
            Err(error) => self
                .metrics
                .record(error_status(&error.to_string(), cancelled)),
        }
        response.map_err(|e| e.to_string())
    }
}

fn metric_status(response: &FederationResponseV1, cancelled: bool) -> FederationMetricStatus {
    if cancelled || response.status == membrane_protocol::FederationStatus::Cancelled {
        return FederationMetricStatus::Cancellation;
    }
    if response
        .diagnostics
        .as_ref()
        .and_then(|d| d.attributes.get("deadline_exhausted"))
        == Some(&"true".to_owned())
    {
        return FederationMetricStatus::Timeout;
    }
    if response.candidates.is_empty()
        && response.status == membrane_protocol::FederationStatus::Complete
    {
        return FederationMetricStatus::EmptyComplete;
    }
    if response.warnings.is_empty() && response.omissions.is_empty() && !response.candidates.is_empty() {
        return FederationMetricStatus::Success;
    }
    if !response.candidates.is_empty() {
        return FederationMetricStatus::Partial;
    }
    response
        .warnings
        .first()
        .map(|warning| match warning.reason {
            membrane_protocol::ReasonCode::ProviderUnavailable => {
                FederationMetricStatus::Unavailable
            }
            membrane_protocol::ReasonCode::ProviderTimeout
            | membrane_protocol::ReasonCode::DeadlineExhausted => FederationMetricStatus::Timeout,
            membrane_protocol::ReasonCode::GenerationIncoherent
            | membrane_protocol::ReasonCode::ReleaseGenerationMismatch => {
                FederationMetricStatus::Incoherent
            }
            membrane_protocol::ReasonCode::ScopeGrantInvalid
            | membrane_protocol::ReasonCode::ScopeGrantMissing => {
                FederationMetricStatus::Unauthorized
            }
            membrane_protocol::ReasonCode::ProviderMalformed => FederationMetricStatus::Malformed,
            _ => FederationMetricStatus::Unavailable,
        })
        .unwrap_or(FederationMetricStatus::Unavailable)
}

fn error_status(error: &str, cancelled: bool) -> FederationMetricStatus {
    if cancelled || error.contains("cancel") {
        FederationMetricStatus::Cancellation
    } else if error.contains("deadline") || error.contains("timeout") {
        FederationMetricStatus::Timeout
    } else if error.contains("scope") || error.contains("unauthor") {
        FederationMetricStatus::Unauthorized
    } else if error.contains("generation") || error.contains("freshness") {
        FederationMetricStatus::Stale
    } else if error.contains("malformed") || error.contains("invalid") {
        FederationMetricStatus::Malformed
    } else {
        FederationMetricStatus::Unavailable
    }
}

fn registration(
    id: ProviderId,
    key: &str,
    provider: Arc<dyn membrane_provider_sdk::Provider>,
    dependencies: Vec<ProviderId>,
) -> ProviderRegistration {
    ProviderRegistration::new(id, key, dependencies, Arc::new(BlockingProvider { inner: provider }))
}

/// Run owner-backed provider work on Tokio's bounded blocking pool. Native
/// providers are async at their contract boundary, but their owner adapters
/// intentionally perform synchronous filesystem, SQLite, and Blueprint work.
/// Keeping that work off scheduler workers lets deadline timers and sibling
/// lanes continue to make progress; the provider/source handles remain the
/// same runtime-owned objects.
struct BlockingProvider {
    inner: Arc<dyn Provider>,
}

impl Provider for BlockingProvider {
    fn provide<'life0, 'life1, 'async_trait>(
        &'life0 self,
        context: &'life1 ProviderContext,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderOutput, ProviderError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let provider = Arc::clone(&self.inner);
        let context = context.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                tokio::runtime::Handle::current().block_on(provider.provide(&context))
            })
            .await
            .map_err(|error| ProviderError::Unavailable(format!("provider blocking worker: {error}")))?
        })
    }

    fn list_capabilities(&self) -> Vec<membrane_provider_sdk::CapabilityV1> {
        self.inner.list_capabilities()
    }

    fn readiness(&self) -> membrane_provider_sdk::provider::ProviderReadinessV1 {
        self.inner.readiness()
    }

    fn handle_operation(
        &self,
        operation: &str,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, ProviderError> {
        self.inner.handle_operation(operation, request)
    }
}

#[cfg(test)]
mod tests {
    use super::runtime;
    use std::time::Duration;

    #[test]
    fn bounded_runtime_keeps_deadline_timer_live_during_blocking_work() {
        let runtime = runtime().expect("bounded native runtime");
        let timer_fired = runtime.block_on(async {
            let blocking = tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_millis(40)));
            let timer = tokio::time::timeout(
                Duration::from_millis(20),
                tokio::time::sleep(Duration::from_millis(2)),
            )
            .await
            .is_ok();
            blocking.await.expect("blocking worker must join");
            timer
        });
        assert!(timer_fired, "blocking provider work must not starve deadline timers");
    }
}
