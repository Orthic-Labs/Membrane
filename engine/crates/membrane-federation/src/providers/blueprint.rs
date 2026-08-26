//! Blueprint provider lane.
//!
//! The lane consumes only the typed `BlueprintSource` contract.  It does not
//! parse repository files, open Blueprint storage, or invoke a CLI/process.

use crate::blueprint_client::ContextualBlueprintSource;
use membrane_protocol::{
    FederationProviderStatusV1, ProviderId, ProviderOmissionV1, ProviderWarningV1, ReasonCode,
    WarningSeverity, PROVIDER_OUTPUT_SCHEMA_VERSION,
};
use membrane_provider_sdk::{
    BlueprintSource, Provider, ProviderContext, ProviderError, ProviderOutput,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub const BLUEPRINT_DEFAULT_CAP: usize = 64;
pub const BLUEPRINT_MAX_CAP: usize = 256;

#[derive(Clone)]
pub struct BlueprintProvider {
    contextual_source: Option<Arc<dyn ContextualBlueprintSource>>,
    candidate_cap: usize,
}

impl BlueprintProvider {
    /// Construct a provider without a request-aware source.
    ///
    /// This constructor is retained for composition compatibility, but it is
    /// deliberately fail-closed: live dispatch never falls back to
    /// `BlueprintSource::query`, which cannot carry request deadline or
    /// cancellation.
    pub fn new(_source: Arc<dyn BlueprintSource>) -> Self {
        Self {
            contextual_source: None,
            candidate_cap: BLUEPRINT_DEFAULT_CAP,
        }
    }

    pub fn with_contextual_source<S>(source: Arc<S>) -> Self
    where
        S: BlueprintSource + ContextualBlueprintSource + 'static,
    {
        Self {
            contextual_source: Some(source),
            candidate_cap: BLUEPRINT_DEFAULT_CAP,
        }
    }

    /// Construct from the ordinary source projection plus its mandatory
    /// request-aware adapter.  The ordinary source is retained by runtime
    /// source composition for other provider lanes; this provider dispatches
    /// only through the contextual adapter.
    pub fn with_contextual_source_pair(
        _source: Arc<dyn BlueprintSource>,
        contextual_source: Arc<dyn ContextualBlueprintSource>,
    ) -> Self {
        Self {
            contextual_source: Some(contextual_source),
            candidate_cap: BLUEPRINT_DEFAULT_CAP,
        }
    }

    pub fn with_candidate_cap(mut self, cap: usize) -> Self {
        self.candidate_cap = cap.clamp(1, BLUEPRINT_MAX_CAP);
        self
    }

    pub fn candidate_cap(&self) -> usize {
        self.candidate_cap
    }

    async fn provide_inner(
        &self,
        context: &ProviderContext,
    ) -> Result<ProviderOutput, ProviderError> {
        if context.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        if context.is_deadline_exhausted() {
            return Err(ProviderError::DeadlineExceeded);
        }
        let mut query = context.query();
        query.generation = context
            .scope_grant
            .as_ref()
            .map(|grant| grant.blueprint_generation.clone())
            .or(query.generation);
        let expected_generation = query.generation.clone();
        let response = match match self.contextual_source.as_ref() {
            Some(source) => {
                source
                    .query_with_context(&query, context.deadline, context.cancellation.clone())
                    .await
            }
            None => Err(ProviderError::MissingSource("blueprint_context")),
        } {
            Ok(response) => response,
            Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
            Err(ProviderError::DeadlineExceeded) => return Err(ProviderError::DeadlineExceeded),
            Err(error) => return Ok(gap_output(error)),
        };
        if context.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        if context.is_deadline_exhausted() {
            return Err(ProviderError::DeadlineExceeded);
        }
        if let Some(expected) = expected_generation
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            if response.generation.as_deref() != Some(expected)
                || response.value.generation != expected
            {
                return Ok(generation_gap(
                    expected,
                    response
                        .generation
                        .as_deref()
                        .unwrap_or(&response.value.generation),
                ));
            }
        }
        if response.value.candidates.len() > self.candidate_cap {
            return Ok(oversized_output(self.candidate_cap));
        }
        let mut output = ProviderOutput {
            schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
            provider: ProviderId::Blueprint,
            status: if response.complete {
                FederationProviderStatusV1::Complete
            } else {
                FederationProviderStatusV1::Partial
            },
            generation: Some(response.value.generation.clone()),
            candidates: response.value.candidates,
            warnings: response.warnings.into_iter().map(warning).collect(),
            omissions: Vec::new(),
            diagnostics: None,
            extensions: Default::default(),
        };
        for candidate in &mut output.candidates {
            candidate.provider = Some(ProviderId::Blueprint.as_str().to_owned());
        }
        if output.candidates.is_empty() && output.warnings.is_empty() {
            output.status = FederationProviderStatusV1::Partial;
            output.warnings.push(ProviderWarningV1 {
                provider: ProviderId::Blueprint,
                reason: ReasonCode::ProviderUnavailable,
                severity: WarningSeverity::Warning,
                detail_id: Some("no_relevant_seed".to_owned()),
                stage: Some("blueprint_recall".to_owned()),
                message: None,
            });
        }
        Ok(output)
    }
}

impl Provider for BlueprintProvider {
    fn provide<'life0, 'life1, 'async_trait>(
        &'life0 self,
        context: &'life1 ProviderContext,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderOutput, ProviderError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(self.provide_inner(context))
    }

    fn list_capabilities(&self) -> Vec<membrane_provider_sdk::CapabilityV1> {
        vec![membrane_provider_sdk::CapabilityV1 {
            name: "membrane_blueprint".into(),
            schema_version: 1,
            error_version: 1,
        }]
    }
}

fn warning(source: membrane_provider_sdk::SourceWarning) -> ProviderWarningV1 {
    ProviderWarningV1 {
        provider: ProviderId::Blueprint,
        reason: ReasonCode::parse(&source.code).unwrap_or(ReasonCode::ProviderFailed),
        severity: WarningSeverity::Warning,
        detail_id: source.detail_id,
        stage: Some("blueprint_source".to_owned()),
        message: None,
    }
}

fn gap_output(error: ProviderError) -> ProviderOutput {
    let (reason, detail) = match error {
        ProviderError::Unavailable(_) | ProviderError::MissingSource(_) => {
            (ReasonCode::ProviderUnavailable, "blueprint_unavailable")
        }
        ProviderError::Typed { ref code, .. } if code == "root_not_enrolled" => (
            ReasonCode::ProviderUnavailable,
            "blueprint_root_not_enrolled",
        ),
        ProviderError::Typed { ref code, .. } if code == "graph_missing" => {
            (ReasonCode::ProviderUnavailable, "blueprint_graph_missing")
        }
        ProviderError::Typed { ref code, .. } if code == "not_configured" => {
            (ReasonCode::ProviderUnavailable, "blueprint_not_configured")
        }
        ProviderError::Typed { ref code, .. }
            if matches!(code.as_str(), "stale_blocked" | "generation_mismatch") =>
        {
            (ReasonCode::GenerationIncoherent, "blueprint_stale")
        }
        ProviderError::SourceFailure(_) => (ReasonCode::ProviderFailed, "blueprint_source_failed"),
        ProviderError::MalformedOutput(_) => (ReasonCode::ProviderMalformed, "blueprint_malformed"),
        ProviderError::IdentityMismatch(_) => (ReasonCode::GenerationIncoherent, "blueprint_stale"),
        _ => (ReasonCode::ProviderFailed, "blueprint_failed"),
    };
    ProviderOutput {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider: ProviderId::Blueprint,
        status: FederationProviderStatusV1::Partial,
        generation: None,
        candidates: Vec::new(),
        warnings: vec![ProviderWarningV1 {
            provider: ProviderId::Blueprint,
            reason,
            severity: WarningSeverity::Warning,
            detail_id: Some(detail.into()),
            stage: Some("blueprint_source".into()),
            message: None,
        }],
        omissions: vec![ProviderOmissionV1 {
            provider: ProviderId::Blueprint,
            reason,
            candidate_id: None,
            detail_id: Some(detail.into()),
            stage: Some("blueprint_source".into()),
        }],
        diagnostics: None,
        extensions: Default::default(),
    }
}

fn generation_gap(expected: &str, observed: &str) -> ProviderOutput {
    ProviderOutput {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider: ProviderId::Blueprint,
        status: FederationProviderStatusV1::Partial,
        generation: (!observed.is_empty()).then(|| observed.to_owned()),
        candidates: Vec::new(),
        warnings: vec![ProviderWarningV1 {
            provider: ProviderId::Blueprint,
            reason: ReasonCode::GenerationIncoherent,
            severity: WarningSeverity::Warning,
            detail_id: Some("generation_incoherent".into()),
            stage: Some("generation_admission".into()),
            message: None,
        }],
        omissions: vec![ProviderOmissionV1 {
            provider: ProviderId::Blueprint,
            reason: ReasonCode::GenerationIncoherent,
            candidate_id: None,
            detail_id: Some(format!("expected:{expected}")),
            stage: Some("generation_admission".into()),
        }],
        diagnostics: None,
        extensions: Default::default(),
    }
}

fn oversized_output(cap: usize) -> ProviderOutput {
    ProviderOutput {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider: ProviderId::Blueprint,
        status: FederationProviderStatusV1::Partial,
        generation: None,
        candidates: Vec::new(),
        warnings: vec![ProviderWarningV1 {
            provider: ProviderId::Blueprint,
            reason: ReasonCode::ProviderMalformed,
            severity: WarningSeverity::Warning,
            detail_id: Some("candidate_cap_exceeded".into()),
            stage: Some("blueprint_bounds".into()),
            message: None,
        }],
        omissions: vec![ProviderOmissionV1 {
            provider: ProviderId::Blueprint,
            reason: ReasonCode::ProviderMalformed,
            candidate_id: None,
            detail_id: Some(format!("cap:{cap}")),
            stage: Some("blueprint_bounds".into()),
        }],
        diagnostics: None,
        extensions: Default::default(),
    }
}
