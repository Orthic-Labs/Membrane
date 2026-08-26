//! Direct Cortex memory lane for native federation.
//!
//! Cortex is an injected, typed source.  This module deliberately contains no
//! transport or storage code: resident composition decides how
//! [`MemoryCandidateSource`] is backed and supplies it through `ProviderContext`.

use membrane_protocol::{
    sort_candidates, sort_omissions, sort_warnings, CandidateV1, FederationProviderStatusV1,
    ProviderDiagnosticsV1, ProviderId, ProviderOmissionV1, ProviderOutputV1, ProviderWarningV1,
    ReasonCode, WarningSeverity, PROVIDER_OUTPUT_SCHEMA_VERSION,
};
use membrane_provider_sdk::{
    CapabilityV1, MemoryCandidate, MemoryCandidateSource, Provider, ProviderContext, ProviderError,
    ProviderOutput, SourceResponse, SourceWarning,
};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

/// Native provider identity for durable Cortex memory.
pub const PROVIDER_ID: ProviderId = ProviderId::Cortex;
pub const PROVIDER_NAME: &str = "cortex";

/// Typed provider over an injected Cortex source.
#[derive(Clone, Copy, Debug, Default)]
pub struct CortexProvider;

/// Alias used by composition code that names the lane by its source.
pub type MemoryProvider = CortexProvider;

impl CortexProvider {
    pub const fn new() -> Self {
        Self
    }

    /// Run this lane with one explicit source.  Composition normally calls
    /// [`Provider::provide`] so all request-bound sources stay in context.
    pub async fn provide_from_source(
        &self,
        context: &ProviderContext,
        source: &dyn MemoryCandidateSource,
    ) -> Result<ProviderOutput, ProviderError> {
        self.provide_source(context, source).await
    }

    async fn provide_source(
        &self,
        context: &ProviderContext,
        source: &dyn MemoryCandidateSource,
    ) -> Result<ProviderOutput, ProviderError> {
        if let Some(output) = boundary_output(context) {
            return Ok(output);
        }

        let response = match source.candidates(&context.query()).await {
            Ok(response) => response,
            Err(error) => return Ok(source_error_output(error)),
        };

        if let Some(output) = boundary_output(context) {
            return Ok(output);
        }
        Ok(build_output(context, response))
    }
}

impl Provider for CortexProvider {
    fn provide<'life0, 'life1, 'async_trait>(
        &'life0 self,
        context: &'life1 ProviderContext,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderOutput, ProviderError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let source = context.sources.memory.as_deref();
            match source {
                Some(source) => self.provide_source(context, source).await,
                None => Ok(unavailable_output("source_missing")),
            }
        })
    }

    fn list_capabilities(&self) -> Vec<CapabilityV1> {
        Vec::new()
    }
}

fn boundary_output(context: &ProviderContext) -> Option<ProviderOutput> {
    let (status, reason, detail) = if context.is_cancelled() {
        (
            FederationProviderStatusV1::Cancelled,
            ReasonCode::ProviderCancelled,
            "request_cancelled",
        )
    } else if context.is_deadline_exhausted() {
        (
            FederationProviderStatusV1::Cancelled,
            ReasonCode::DeadlineExhausted,
            "deadline_exhausted",
        )
    } else {
        return None;
    };
    Some(output_with_gap(status, reason, detail))
}

fn unavailable_output(detail: &'static str) -> ProviderOutput {
    output_with_gap(
        FederationProviderStatusV1::Failed,
        ReasonCode::ProviderUnavailable,
        detail,
    )
}

fn source_error_output(error: ProviderError) -> ProviderOutput {
    match error {
        ProviderError::Cancelled => output_with_gap(
            FederationProviderStatusV1::Cancelled,
            ReasonCode::ProviderCancelled,
            "source_cancelled",
        ),
        ProviderError::DeadlineExceeded => output_with_gap(
            FederationProviderStatusV1::Cancelled,
            ReasonCode::ProviderTimeout,
            "source_timeout",
        ),
        _ => unavailable_output("source_unavailable"),
    }
}

fn output_with_gap(
    status: FederationProviderStatusV1,
    reason: ReasonCode,
    detail: &'static str,
) -> ProviderOutput {
    let warning = ProviderWarningV1 {
        provider: PROVIDER_ID,
        reason,
        severity: WarningSeverity::Warning,
        detail_id: Some(detail.to_owned()),
        stage: Some("provider".to_owned()),
        message: None,
    };
    let omission = ProviderOmissionV1 {
        provider: PROVIDER_ID,
        reason,
        candidate_id: None,
        detail_id: Some(detail.to_owned()),
        stage: Some("provider".to_owned()),
    };
    ProviderOutputV1 {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider: PROVIDER_ID,
        status,
        generation: None,
        candidates: Vec::new(),
        warnings: vec![warning],
        omissions: vec![omission],
        diagnostics: None,
        extensions: BTreeMap::new(),
    }
}

fn build_output(
    context: &ProviderContext,
    response: SourceResponse<Vec<MemoryCandidate>>,
) -> ProviderOutput {
    let mut warnings: Vec<ProviderWarningV1> =
        response.warnings.iter().map(source_warning).collect();
    let mut omissions = Vec::new();
    let mut candidates = Vec::new();

    for record in response.value {
        match normalize_memory_candidate(&record, &context.repository_id) {
            Ok(candidate) => candidates.push(candidate),
            Err(reason) => omissions.push(ProviderOmissionV1 {
                provider: PROVIDER_ID,
                reason,
                candidate_id: nonempty(&record.id),
                detail_id: Some("candidate_malformed".to_owned()),
                stage: Some("normalization".to_owned()),
            }),
        }
    }

    if !response.complete {
        warnings.push(ProviderWarningV1 {
            provider: PROVIDER_ID,
            reason: ReasonCode::ProviderFailed,
            severity: WarningSeverity::Warning,
            detail_id: Some("source_incomplete".to_owned()),
            stage: Some("source".to_owned()),
            message: None,
        });
    }

    // The provider envelope requires explicit coverage even when Cortex has
    // no eligible memories.  Keep that state lane-local and content-free.
    if response.complete && candidates.is_empty() && warnings.is_empty() && omissions.is_empty() {
        warnings.push(ProviderWarningV1 {
            provider: PROVIDER_ID,
            reason: ReasonCode::Internal,
            severity: WarningSeverity::Warning,
            detail_id: Some("source_complete_empty".to_owned()),
            stage: Some("source".to_owned()),
            message: None,
        });
    }

    let status = if response.complete && warnings.is_empty() && omissions.is_empty() {
        FederationProviderStatusV1::Complete
    } else {
        FederationProviderStatusV1::Partial
    };
    sort_candidates(&mut candidates);
    sort_warnings(&mut warnings);
    sort_omissions(&mut omissions);
    let generation = response.generation.filter(|value| !value.trim().is_empty());
    let mut attributes = BTreeMap::new();
    attributes.insert("provenance".to_owned(), "cortex-memory-source".to_owned());

    ProviderOutputV1 {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider: PROVIDER_ID,
        status,
        generation: generation.clone(),
        candidates,
        warnings,
        omissions,
        diagnostics: Some(ProviderDiagnosticsV1 {
            provider: PROVIDER_ID,
            elapsed_ms: None,
            generation,
            attributes,
        }),
        extensions: BTreeMap::new(),
    }
}

/// Validate source identity and preserve the source-owned candidate payload.
/// Provider stamping is the only mutation: it records lane provenance and does
/// not upgrade authority, trust, freshness, or influence semantics.
pub fn normalize_memory_candidate(
    record: &MemoryCandidate,
    repository_id: &str,
) -> Result<CandidateV1, ReasonCode> {
    if record.repository_id.trim().is_empty()
        || record.repository_id != repository_id
        || record.generation.trim().is_empty()
        || record.source_hash.trim().is_empty()
        || record.candidate.id != record.id
    {
        return Err(ReasonCode::ProviderMalformed);
    }
    let candidate = &record.candidate;
    if [
        candidate.id.as_str(),
        candidate.source_kind.as_str(),
        candidate.source_ref.as_str(),
        candidate.source_hash.as_str(),
        candidate.trust_class.as_str(),
        candidate.instruction_policy.as_str(),
        candidate.resolver.as_str(),
    ]
    .iter()
    .any(|value| value.trim().is_empty())
        || !candidate.provider_score.is_finite()
        || !(0.0..=1.0).contains(&candidate.provider_score)
        || candidate
            .score_components
            .values()
            .any(|value| !value.is_finite())
        || candidate.source_hash != record.source_hash
    {
        return Err(ReasonCode::ProviderMalformed);
    }
    let mut candidate = candidate.clone();
    candidate.provider = Some(PROVIDER_NAME.to_owned());
    Ok(candidate)
}

fn source_warning(warning: &SourceWarning) -> ProviderWarningV1 {
    let reason = ReasonCode::parse(warning.code.trim()).unwrap_or(ReasonCode::Internal);
    ProviderWarningV1 {
        provider: PROVIDER_ID,
        reason,
        severity: WarningSeverity::Warning,
        detail_id: warning
            .detail_id
            .clone()
            .or_else(|| nonempty(&warning.code)),
        stage: Some("source".to_owned()),
        message: None,
    }
}

fn nonempty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.to_owned())
}
