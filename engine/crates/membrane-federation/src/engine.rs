//! In-process federation coordinator.
//!
//! `FederationEngine` is deliberately a composition type.  It owns no source
//! storage and has no transport fallback: request validation, owner bindings,
//! bounded provider scheduling, and deterministic merge are one pipeline.

use crate::config::FederationConfig;
use crate::corrective::{
    corrective_plan, corrective_trigger, evaluate_sufficiency, receipt_for_request,
    validated_contract_for_request, CorrectiveRetrievalReceiptV1, SufficiencyAssessmentV1,
    SufficiencyStateV1,
};
use crate::deadline::{Deadline, SystemClock};
use crate::freshness::FreshnessBinding;
use crate::merge::{merge_normalized_with_strategy, FusionStrategy, MergeError, MergeResult};
use crate::normalize::{admit_generation, malformed_omission, normalize_provider_output};
use crate::omission::{generation_omission, missing_lane, warning_from_omission};
use crate::registry::ProviderRegistry;
use crate::release::{ReleaseBinding, ReleaseError, ReleaseIdentity, ReleaseSource};
use crate::request::{NormalizedFederationRequest, RequestValidationError};
use crate::root::{FilesystemRootSource, RootPathSource};
use crate::scheduler::{schedule_providers, ProviderTask, ScheduleResult, SchedulerPolicy};
use membrane_protocol::{
    FederationRequestV1, FederationResponseV1, InsufficientConfidenceLaneSearchV1,
    InsufficientConfidenceReasonV1, InsufficientConfidenceStatusV1, InsufficientConfidenceV1,
    ProviderDiagnosticsV1, ProviderId, ProviderOmissionV1, ProviderOutputV1, ProviderWarningV1,
    PublicationFenceV1, ReasonCode, WarningSeverity,
};
use membrane_provider_sdk::{ProviderContext, SourceQuery, SourceSet};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

/// Errors which prevent request-bound composition.  Provider-local errors are
/// represented in the response as lane omissions and do not abort healthy
/// lanes.
#[derive(Debug, thiserror::Error)]
pub enum FederationEngineError {
    #[error("federation configuration is invalid: {0}")]
    Config(#[from] crate::error::ConfigError),
    #[error("provider registry is invalid: {0}")]
    Registry(#[from] crate::error::RegistryError),
    #[error("request validation failed: {0}")]
    Request(#[from] RequestValidationError),
    #[error("release binding failed: {0}")]
    Release(String),
    #[error("freshness binding failed: {0}")]
    Freshness(String),
    #[error("scope grant binding failed: {0}")]
    Scope(String),
    #[error("publication fence refused emission: {0}")]
    Fence(String),
    #[error("federation internal failure: {0}")]
    Internal(String),
    #[error("federation merge failed: {0}")]
    Merge(#[from] MergeError),
}

/// Non-secret metrics emitted by one completed federation cycle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FederationMetrics {
    pub elapsed_ms: u64,
    pub expected_lanes: usize,
    pub active_lanes: usize,
    pub output_lanes: usize,
    pub omission_lanes: usize,
    pub candidate_count: usize,
    pub deadline_exhausted: bool,
    pub cancelled: bool,
    pub provider_timings: Vec<ProviderDiagnosticsV1>,
}

/// Immutable native federation composition.
pub struct FederationEngine {
    registry: Arc<ProviderRegistry>,
    config: FederationConfig,
    sources: SourceSet,
    release_source: Arc<dyn ReleaseSource + Send + Sync>,
    root_source: Arc<dyn RootPathSource + Send + Sync>,
    scheduler_policy: SchedulerPolicy,
    fusion_strategy: FusionStrategy,
}

impl std::fmt::Debug for FederationEngine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FederationEngine")
            .field("providers", &self.registry.ids())
            .field("config", &self.config)
            .field("sources", &self.sources)
            .field("release_source", &"injected")
            .field("scheduler_policy", &self.scheduler_policy)
            .field("fusion_strategy", &self.fusion_strategy)
            .finish()
    }
}

impl FederationEngine {
    /// Construct a frozen coordinator.  The registry and configuration are
    /// validated before any request can enter the provider pipeline.
    pub fn new(
        registry: ProviderRegistry,
        config: FederationConfig,
        sources: SourceSet,
    ) -> Result<Self, FederationEngineError> {
        Self::with_release_and_root_source(
            registry,
            config,
            sources,
            MissingReleaseSource,
            FilesystemRootSource,
        )
    }

    /// Construct with an owner-provided release identity.  The source is
    /// resolved at each request boundary; no release identity is inferred from
    /// local repository contents.
    pub fn with_release_source<R>(
        registry: ProviderRegistry,
        config: FederationConfig,
        sources: SourceSet,
        release_source: R,
    ) -> Result<Self, FederationEngineError>
    where
        R: ReleaseSource + Send + Sync + 'static,
    {
        Self::with_release_and_root_source(
            registry,
            config,
            sources,
            release_source,
            FilesystemRootSource,
        )
    }

    /// Construct with an injected root owner.  Root policy remains outside
    /// federation, while the engine retains the resulting canonical identity.
    pub fn with_root_source<R>(
        registry: ProviderRegistry,
        config: FederationConfig,
        sources: SourceSet,
        root_source: R,
    ) -> Result<Self, FederationEngineError>
    where
        R: RootPathSource + Send + Sync + 'static,
    {
        Self::with_release_and_root_source(
            registry,
            config,
            sources,
            MissingReleaseSource,
            root_source,
        )
    }

    /// Construct with both owner-provided release and repository-root
    /// sources.  This is the canonical composition entry point for runtimes.
    pub fn with_release_and_root_source<R, O>(
        registry: ProviderRegistry,
        config: FederationConfig,
        sources: SourceSet,
        release_source: R,
        root_source: O,
    ) -> Result<Self, FederationEngineError>
    where
        R: ReleaseSource + Send + Sync + 'static,
        O: RootPathSource + Send + Sync + 'static,
    {
        config.validate()?;
        if registry.ids().len() != ProviderId::ALL.len() {
            return Err(crate::error::RegistryError::Incomplete.into());
        }
        Ok(Self {
            registry: Arc::new(registry),
            config,
            sources,
            release_source: Arc::new(release_source),
            root_source: Arc::new(root_source),
            scheduler_policy: SchedulerPolicy::default(),
            fusion_strategy: FusionStrategy::default(),
        })
    }

    pub fn with_scheduler_policy(mut self, policy: SchedulerPolicy) -> Self {
        self.scheduler_policy = policy;
        self
    }

    /// Select a versioned fusion strategy explicitly. The default remains the
    /// fixed provider/security ordering used by current production.
    pub fn with_fusion_strategy(mut self, strategy: FusionStrategy) -> Self {
        self.fusion_strategy = strategy;
        self
    }

    pub fn registry(&self) -> &ProviderRegistry {
        &self.registry
    }

    pub fn config(&self) -> &FederationConfig {
        &self.config
    }

    pub const fn fusion_strategy(&self) -> FusionStrategy {
        self.fusion_strategy
    }

    pub fn sources(&self) -> &SourceSet {
        &self.sources
    }

    /// Run one request through the fixed native pipeline.
    pub async fn federate(
        &self,
        request: &FederationRequestV1,
        cancellation: CancellationToken,
    ) -> Result<FederationResponseV1, FederationEngineError> {
        let started = Instant::now();
        let root_source = RootSourceRef(self.root_source.as_ref());
        let normalized = NormalizedFederationRequest::normalize(request, &root_source)?;
        // Publication fence (pending §17.2): re-validate grant identity,
        // policy epoch and revocation before grant binding. The engine has
        // no post-fusion grant source to consult, so the request envelope
        // must carry the caller's post-fusion re-check; a tripped fence is
        // refused here and the packet authorized under the superseded grant
        // is never emitted. A held fence is stamped into the response for
        // downstream publication seams to preserve.
        let publication_fence = validated_fence_for_request(request)?;
        let query = source_query(&normalized);

        // Owner bindings happen before any provider task is created.
        let release = self.bind_release(normalized.release_generation.as_deref())?;
        let freshness = self.bind_freshness(&query, release.clone()).await?;
        let scope_grant = self.bind_scope(&normalized, &query).await?;
        let expected_generation = normalized
            .release_generation
            .as_deref()
            .or(freshness.snapshot.generation.as_deref());

        let deadline = Deadline::from_budget(&SystemClock, normalized.deadline);
        let provider_context = ProviderContext::new(
            normalized.request_id.clone(),
            normalized.repository_root.clone(),
            normalized.repository_id.clone(),
            normalized.task.clone(),
            normalized.session_id.clone(),
            normalized.client.clone(),
            normalized
                .anchors
                .iter()
                .map(|anchor| anchor.value.clone())
                .collect(),
            scope_grant,
            normalized.release_generation.clone(),
            freshness.snapshot.clone(),
            deadline.instant(),
            cancellation,
            normalized.trace_id.clone(),
            self.sources.clone(),
        );

        let active: Vec<ProviderId> = self
            .config
            .expected_providers()
            .filter(|provider| self.config.is_enabled(*provider))
            .collect();
        let fatal = CancellationToken::new();
        let tasks = self.provider_tasks(&active, fatal.clone());
        let mut schedule = schedule_providers(
            provider_context.clone(),
            deadline,
            tasks,
            self.scheduler_policy,
        )
        .await;

        // A provider panic or internal invariant is a composition failure,
        // not a lane-local omission.  Provider task wrappers cancel sibling
        // work through `fatal`; return a typed engine error after the bounded
        // scheduler drain has joined every task.
        if fatal.is_cancelled() {
            return Err(FederationEngineError::Internal(
                "provider panic or internal invariant".to_owned(),
            ));
        }

        let mut merged = merge_scheduled_outputs(
            &active,
            &schedule.outputs,
            expected_generation,
            self.fusion_strategy,
        );
        let mut corrective_receipt = receipt_for_request(request, &merged.providers, &active);
        if let Some(contract) = validated_contract_for_request(request) {
            let initial_assessment = corrective_receipt
                .sufficiency
                .clone()
                .filter(|assessment| assessment.state == SufficiencyStateV1::Insufficient);
            if let Some(initial_assessment) = initial_assessment {
                let trigger =
                    corrective_trigger(&contract, &initial_assessment, &merged.providers, &active);
                if let Some((trigger_provider, target_provider, target_requirement)) =
                    corrective_plan(&contract, &initial_assessment, &merged.providers, &active)
                {
                    if deadline.is_exhausted(&SystemClock) {
                        corrective_receipt = CorrectiveRetrievalReceiptV1::after_stage(
                            initial_assessment,
                            trigger_provider,
                            target_provider,
                            target_requirement,
                            false,
                            "terminal_insufficient_deadline_exhausted",
                        );
                    } else if provider_context.is_cancelled() {
                        corrective_receipt = CorrectiveRetrievalReceiptV1::after_stage(
                            initial_assessment,
                            trigger_provider,
                            target_provider,
                            target_requirement,
                            false,
                            "terminal_insufficient_cancelled",
                        );
                    } else if estimated_tokens(&merged) >= u64::from(normalized.max_tokens) {
                        corrective_receipt = CorrectiveRetrievalReceiptV1::after_stage(
                            initial_assessment,
                            trigger_provider,
                            target_provider,
                            target_requirement,
                            false,
                            "terminal_insufficient_budget_exhausted",
                        );
                    } else {
                        let stage_fatal = CancellationToken::new();
                        let stage_tasks =
                            self.provider_tasks(&[target_provider], stage_fatal.clone());
                        let stage = schedule_providers(
                            provider_context.clone(),
                            deadline,
                            stage_tasks,
                            self.scheduler_policy,
                        )
                        .await;
                        if stage_fatal.is_cancelled() {
                            return Err(FederationEngineError::Internal(
                                "provider panic or internal invariant".to_owned(),
                            ));
                        }
                        let attempted = !stage.timings.is_empty();
                        let mut admitted = false;
                        let mut budget_rejected = false;
                        if let Some(correction) = stage
                            .outputs
                            .iter()
                            .find(|output| output.provider == target_provider)
                            .cloned()
                        {
                            let available = u64::from(normalized.max_tokens)
                                .saturating_sub(estimated_tokens(&merged));
                            if output_tokens(&correction) <= available {
                                crate::corrective::append_output(&mut schedule.outputs, correction);
                                admitted = true;
                            } else {
                                budget_rejected = true;
                                schedule.omissions.push(corrective_omission(
                                    target_provider,
                                    ReasonCode::ProviderUnavailable,
                                    "budget_exhausted",
                                ));
                            }
                        } else if !stage.outputs.is_empty() {
                            schedule.omissions.push(corrective_omission(
                                target_provider,
                                ReasonCode::ProviderMalformed,
                                "provider_identity_mismatch",
                            ));
                        }
                        schedule
                            .outputs
                            .sort_by_key(|output| output.provider.rank());
                        merged = merge_scheduled_outputs(
                            &active,
                            &schedule.outputs,
                            expected_generation,
                            self.fusion_strategy,
                        );
                        let final_assessment =
                            evaluate_sufficiency(&contract, &merged.providers, &active)
                                .unwrap_or_else(|_| initial_assessment.clone());
                        let outcome = corrective_stage_outcome(
                            &stage,
                            admitted,
                            budget_rejected,
                            &final_assessment,
                        );
                        schedule.omissions.extend(stage.omissions);
                        schedule.timings.extend(stage.timings);
                        schedule.deadline_exhausted |= stage.deadline_exhausted;
                        schedule.cancelled |= stage.cancelled;
                        corrective_receipt = CorrectiveRetrievalReceiptV1::after_stage(
                            final_assessment,
                            trigger_provider,
                            target_provider,
                            target_requirement,
                            attempted,
                            outcome,
                        );
                    }
                } else if let Some((trigger_provider, target_requirement)) = trigger {
                    corrective_receipt = CorrectiveRetrievalReceiptV1::terminal_insufficiency(
                        initial_assessment,
                        Some(trigger_provider),
                        None,
                        Some(target_requirement),
                        "terminal_insufficient_no_alternate_provider",
                    );
                } else {
                    corrective_receipt = CorrectiveRetrievalReceiptV1::terminal_insufficiency(
                        initial_assessment,
                        None,
                        None,
                        None,
                        "terminal_insufficient_no_trigger_lane",
                    );
                }
            }
        }
        append_schedule_accounting(&mut merged, &schedule);
        for provider in self.config.expected_providers() {
            if let Some(omission) = self.config.disabled_omission(provider) {
                merged.omissions.push(omission.clone());
                merged.warnings.push(warning_from_omission(&omission));
            }
        }
        merged.canonicalize();
        let mut response = merged.response(normalized.request_id, normalized.trace_id);
        let metrics = metrics(&started, &active, &schedule, &merged);
        response.diagnostics = Some(diagnostics(&metrics));
        response.extensions.insert(
            "correctiveRetrieval".to_owned(),
            serde_json::to_value(corrective_receipt).unwrap_or(serde_json::Value::Null),
        );
        if let Some(fence) = publication_fence {
            response.extensions.insert(
                "publicationFence".to_owned(),
                serde_json::to_value(fence).unwrap_or(serde_json::Value::Null),
            );
        }
        // Typed abstention (pending §17.1): when every active lane searched
        // and nothing was admitted, publish the typed no-answer envelope
        // instead of an empty/below-floor candidate list.
        if response.candidates.is_empty() {
            response.extensions.insert(
                "insufficientConfidence".to_owned(),
                serde_json::to_value(insufficient_confidence_from_merge(&merged, &active))
                    .unwrap_or(serde_json::Value::Null),
            );
        }
        Ok(response)
    }

    fn bind_release(
        &self,
        observed_generation: Option<&str>,
    ) -> Result<ReleaseBinding, FederationEngineError> {
        let binding = ReleaseBinding::resolve(self.release_source.as_ref(), observed_generation)
            .map_err(|error| FederationEngineError::Release(error.to_string()))?;
        if !binding.is_compatible() {
            return Err(FederationEngineError::Release(
                binding
                    .warning_code()
                    .unwrap_or("release_generation_unavailable")
                    .to_owned(),
            ));
        }
        Ok(binding)
    }

    async fn bind_freshness(
        &self,
        query: &SourceQuery,
        release: ReleaseBinding,
    ) -> Result<FreshnessBinding, FederationEngineError> {
        let Some(source) = self.sources.freshness.as_deref() else {
            return Err(FederationEngineError::Freshness(
                "source_missing".to_owned(),
            ));
        };
        FreshnessBinding::acquire(source, query, Some(release))
            .await
            .map_err(|error| FederationEngineError::Freshness(error.to_string()))
    }

    async fn bind_scope(
        &self,
        request: &NormalizedFederationRequest,
        query: &SourceQuery,
    ) -> Result<Option<membrane_provider_sdk::source::ScopeGrantView>, FederationEngineError> {
        let Some(grant_id) = request.scope_grant_id.as_deref() else {
            return Ok(None);
        };
        let Some(source) = self.sources.scope_grant.as_deref() else {
            return Err(FederationEngineError::Scope("source_missing".to_owned()));
        };
        let response = source
            .grant(query)
            .await
            .map_err(|error| FederationEngineError::Scope(error.to_string()))?;
        if !response.complete {
            return Err(FederationEngineError::Scope("source_incomplete".to_owned()));
        }
        let grant = response.value;
        if grant.id != grant_id
            || grant.repository_id != request.repository_id
            || grant.repository_root != request.repository_root
            || grant.task_id != request.task
            || grant.session_id != request.session_id
            || request
                .manifest_digest
                .as_deref()
                .is_some_and(|digest| grant.manifest_digest != digest)
        {
            return Err(FederationEngineError::Scope("binding_mismatch".to_owned()));
        }
        Ok(Some(grant))
    }

    fn provider_tasks(&self, active: &[ProviderId], fatal: CancellationToken) -> Vec<ProviderTask> {
        active
            .iter()
            .filter_map(|provider| {
                let registration = self.registry.get(*provider)?;
                let implementation = Arc::clone(&registration.provider);
                let fatal = fatal.clone();
                Some(
                    ProviderTask::new(*provider, move |context| {
                        let implementation = Arc::clone(&implementation);
                        let fatal = fatal.clone();
                        async move {
                            // Run provider futures in a child task so both a
                            // panic during construction and a panic while
                            // polling become typed internal failures.  The
                            // abort handle prevents a sibling cancelled by
                            // `fatal` from becoming detached work.
                            let task =
                                tokio::spawn(async move { implementation.run(&context).await });
                            let abort = task.abort_handle();
                            match fatal.clone().run_until_cancelled_owned(task).await {
                                None => {
                                    abort.abort();
                                    Err(membrane_provider_sdk::ProviderError::Internal(
                                        "sibling_cancelled_after_internal".to_owned(),
                                    ))
                                }
                                Some(Ok(Ok(output))) => Ok(output),
                                Some(Ok(Err(error))) => {
                                    if matches!(
                                        &error,
                                        membrane_provider_sdk::ProviderError::Internal(_)
                                    ) {
                                        fatal.cancel();
                                    }
                                    Err(error)
                                }
                                Some(Err(_join_error)) => {
                                    fatal.cancel();
                                    Err(membrane_provider_sdk::ProviderError::Internal(
                                        "provider_panic".to_owned(),
                                    ))
                                }
                            }
                        }
                    })
                    .with_prerequisites(
                        registration
                            .dependencies
                            .iter()
                            .copied()
                            .filter(|dependency| active.contains(dependency)),
                    ),
                )
            })
            .collect()
    }
}

fn source_query(request: &NormalizedFederationRequest) -> SourceQuery {
    SourceQuery {
        request_id: request.request_id.clone(),
        repository_id: request.repository_id.clone(),
        repository_root: request.repository_root.clone(),
        task: request.task.clone(),
        session_id: request.session_id.clone(),
        generation: request
            .blueprint_generation
            .clone()
            .or(request.release_generation.clone()),
        anchors: request
            .anchors
            .iter()
            .map(|anchor| anchor.value.clone())
            .collect(),
    }
}

/// Read the caller's post-fusion publication fence verdict (pending §17.2)
/// from the extensible request envelope. A tripped fence is a typed engine
/// error — the stale-authorized packet is never emitted; an absent fence
/// means no grant was bound and the fence is a no-op, never a bypass.
fn validated_fence_for_request(
    request: &FederationRequestV1,
) -> Result<Option<PublicationFenceV1>, FederationEngineError> {
    let Some(value) = request.extensions.get("publicationFence") else {
        return Ok(None);
    };
    let fence = serde_json::from_value::<PublicationFenceV1>(value.clone()).map_err(|error| {
        FederationEngineError::Fence(format!("invalid publication fence receipt: {error}"))
    })?;
    match fence.status {
        membrane_protocol::PublicationFenceStatusV1::Held => Ok(Some(fence)),
        membrane_protocol::PublicationFenceStatusV1::PolicyChanged => {
            Err(FederationEngineError::Fence("policy_changed".to_owned()))
        }
    }
}

/// Typed abstention (pending §17.1) built from content-free merge
/// accounting: per-lane searched counts come from expected/observed lane
/// status, never from candidate text.
fn insufficient_confidence_from_merge(
    merged: &MergeResult,
    active: &[ProviderId],
) -> InsufficientConfidenceV1 {
    let searched = active
        .iter()
        .copied()
        .map(|provider| InsufficientConfidenceLaneSearchV1 {
            lane: provider.as_str().to_owned(),
            searched: merged
                .providers
                .iter()
                .filter(|lane| lane.provider == provider)
                .map(|lane| lane.candidates.len() as u32)
                .sum(),
        })
        .collect();
    let expected_active = active.len();
    let observed = merged.providers.len();
    let reason = if expected_active == 0 {
        InsufficientConfidenceReasonV1::NoCandidates
    } else if observed < expected_active {
        // At least one expected lane never produced an observation.
        InsufficientConfidenceReasonV1::NoAuthorizedCandidateAboveThreshold
    } else {
        InsufficientConfidenceReasonV1::EvidenceFloor
    };
    InsufficientConfidenceV1 {
        status: InsufficientConfidenceStatusV1::InsufficientConfidence,
        schema_version: membrane_protocol::INSUFFICIENT_CONFIDENCE_SCHEMA_VERSION,
        policy: membrane_protocol::INSUFFICIENT_CONFIDENCE_POLICY.to_owned(),
        searched,
        reason,
        suggested_action: InsufficientConfidenceV1::suggested_action_for(reason).map(str::to_owned),
    }
}

/// The compatibility constructor intentionally has no implicit release
/// identity.  Runtimes must use `with_release_source`; requests through this
/// constructor fail closed before provider scheduling.
#[derive(Clone, Copy, Debug, Default)]
struct MissingReleaseSource;

impl ReleaseSource for MissingReleaseSource {
    fn current_release(&self) -> Result<ReleaseIdentity, ReleaseError> {
        Err(ReleaseError::Unavailable(
            "release_source_missing".to_owned(),
        ))
    }
}

struct RootSourceRef<'a>(&'a (dyn RootPathSource + Send + Sync));

impl RootPathSource for RootSourceRef<'_> {
    fn resolve_root(
        &self,
        requested: &std::path::Path,
    ) -> Result<crate::root::CanonicalRepositoryRoot, crate::root::RootError> {
        self.0.resolve_root(requested)
    }
}

/// Normalize lanes independently so one malformed provider cannot erase
/// independent valid outputs.  Generation admission is deliberately before
/// normalization/merge and every expected lane receives one terminal.
fn merge_scheduled_outputs(
    expected: &[ProviderId],
    outputs: &[membrane_protocol::ProviderOutputV1],
    expected_generation: Option<&str>,
    strategy: FusionStrategy,
) -> MergeResult {
    let mut normalized = Vec::new();
    let mut warnings = Vec::new();
    let mut omissions = Vec::new();
    for provider in expected.iter().copied() {
        let Some(output) = outputs.iter().find(|output| output.provider == provider) else {
            omissions.push(missing_lane(provider));
            continue;
        };
        if admit_generation(output, expected_generation).is_err() {
            omissions.push(generation_omission(provider));
            continue;
        }
        match normalize_provider_output(output, provider) {
            Ok(output) => {
                warnings.extend(output.warnings.clone());
                omissions.extend(output.omissions.clone());
                normalized.push(output);
            }
            Err(error) => {
                let omission = malformed_omission(provider, error.to_string());
                warnings.push(warning_from_omission(&omission));
                omissions.push(omission);
            }
        }
    }
    merge_normalized_with_strategy(normalized, warnings, omissions, strategy)
}

fn append_schedule_accounting(merged: &mut MergeResult, schedule: &ScheduleResult) {
    for omission in &schedule.omissions {
        if merged.omissions.iter().any(|existing| {
            existing.provider == omission.provider
                && existing.detail_id.as_deref() != Some("lane_missing")
        }) {
            continue;
        }
        merged.omissions.retain(|existing| {
            !(existing.provider == omission.provider
                && existing.detail_id.as_deref() == Some("lane_missing"))
        });
        merged.warnings.push(warning_from_omission(omission));
        merged.omissions.push(omission.clone());
    }
}

fn estimated_tokens(merged: &MergeResult) -> u64 {
    merged
        .candidates
        .iter()
        .map(|candidate| u64::from(candidate.candidate.estimated_tokens))
        .sum()
}

fn output_tokens(output: &ProviderOutputV1) -> u64 {
    output
        .candidates
        .iter()
        .map(|candidate| u64::from(candidate.estimated_tokens))
        .sum()
}

fn corrective_omission(
    provider: ProviderId,
    reason: ReasonCode,
    detail_id: &'static str,
) -> ProviderOmissionV1 {
    ProviderOmissionV1 {
        provider,
        reason,
        candidate_id: None,
        detail_id: Some(detail_id.to_owned()),
        stage: Some("corrective_retrieval".to_owned()),
    }
}

fn corrective_stage_outcome(
    stage: &ScheduleResult,
    admitted: bool,
    budget_rejected: bool,
    assessment: &SufficiencyAssessmentV1,
) -> &'static str {
    if stage.deadline_exhausted
        || stage
            .omissions
            .iter()
            .any(|omission| omission.reason == ReasonCode::ProviderTimeout)
    {
        return "terminal_insufficient_deadline_exhausted";
    }
    if stage.cancelled
        || stage
            .omissions
            .iter()
            .any(|omission| omission.reason == ReasonCode::ProviderCancelled)
    {
        return "terminal_insufficient_cancelled";
    }
    if budget_rejected {
        return "terminal_insufficient_budget_exhausted";
    }
    if !admitted {
        return "terminal_insufficient_provider_failure";
    }
    match assessment.state {
        SufficiencyStateV1::Sufficient => "corrective_stage_sufficient",
        SufficiencyStateV1::Insufficient => "terminal_insufficient_second_assessment",
        SufficiencyStateV1::Unknown => "terminal_insufficient_provider_incomplete",
    }
}

fn metrics(
    started: &Instant,
    active: &[ProviderId],
    schedule: &ScheduleResult,
    merged: &MergeResult,
) -> FederationMetrics {
    FederationMetrics {
        elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        expected_lanes: ProviderId::ALL.len(),
        active_lanes: active.len(),
        output_lanes: schedule.outputs.len(),
        omission_lanes: merged.omissions.len(),
        candidate_count: merged.candidates.len(),
        deadline_exhausted: schedule.deadline_exhausted,
        cancelled: schedule.cancelled,
        provider_timings: schedule
            .timings
            .iter()
            .map(|timing| {
                let mut attributes = BTreeMap::new();
                attributes.insert("queue_ms".to_owned(), timing.queue_ms.to_string());
                attributes.insert("start_ms".to_owned(), timing.start_ms.to_string());
                attributes.insert("end_ms".to_owned(), timing.end_ms.to_string());
                ProviderDiagnosticsV1 {
                    provider: timing.provider,
                    elapsed_ms: Some(timing.end_ms.saturating_sub(timing.start_ms)),
                    generation: None,
                    attributes,
                }
            })
            .collect(),
    }
}

fn diagnostics(metrics: &FederationMetrics) -> membrane_protocol::FederationDiagnosticsV1 {
    let mut attributes = BTreeMap::new();
    attributes.insert("elapsed_ms".to_owned(), metrics.elapsed_ms.to_string());
    attributes.insert(
        "expected_lanes".to_owned(),
        metrics.expected_lanes.to_string(),
    );
    attributes.insert("active_lanes".to_owned(), metrics.active_lanes.to_string());
    attributes.insert("output_lanes".to_owned(), metrics.output_lanes.to_string());
    attributes.insert(
        "omission_lanes".to_owned(),
        metrics.omission_lanes.to_string(),
    );
    attributes.insert(
        "candidate_count".to_owned(),
        metrics.candidate_count.to_string(),
    );
    attributes.insert(
        "deadline_exhausted".to_owned(),
        metrics.deadline_exhausted.to_string(),
    );
    attributes.insert("cancelled".to_owned(), metrics.cancelled.to_string());
    membrane_protocol::FederationDiagnosticsV1 {
        providers: metrics.provider_timings.clone(),
        attributes,
    }
}

#[allow(dead_code)]
fn _content_free_error(provider: ProviderId, reason: ReasonCode) -> ProviderWarningV1 {
    ProviderWarningV1 {
        provider,
        reason,
        severity: WarningSeverity::Warning,
        detail_id: None,
        stage: Some("engine".to_owned()),
        message: None,
    }
}
