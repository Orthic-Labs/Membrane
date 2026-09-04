//! Native audit provider over the injected [`AuditFindingSource`] contract.
//!
//! Audit records are owner-produced.  This lane only checks their binding,
//! preserves their provenance, and accounts for source gaps; it never loads
//! Python modules or derives repository identity from a filesystem path.

use membrane_protocol::{
    CandidateV1, FederationProviderStatusV1, ProviderDiagnosticsV1, ProviderId, ProviderOmissionV1,
    ProviderOutputV1, ProviderWarningV1, ReasonCode, WarningSeverity,
    PROVIDER_OUTPUT_SCHEMA_VERSION,
};
use membrane_provider_sdk::{
    AuditFinding, AuditFindingSource, Provider, ProviderContext, ProviderError, ProviderOutput,
    SourceResponse, SourceWarning,
};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

pub const PROVIDER_ID: ProviderId = ProviderId::Audit;

/// Native audit lane.  The source is selected by composition through
/// `ProviderContext::sources`; this type has no storage or transport state.
#[derive(Clone, Copy, Debug, Default)]
pub struct AuditProvider;

impl AuditProvider {
    pub const fn new() -> Self {
        Self
    }

    pub async fn provide_from_source(
        &self,
        context: &ProviderContext,
        source: &dyn AuditFindingSource,
    ) -> Result<ProviderOutput, ProviderError> {
        provide_from_source(context, source).await
    }
}

impl Provider for AuditProvider {
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
            let Some(source) = context.sources.audit.as_deref() else {
                return Ok(gap_output(
                    FederationProviderStatusV1::Failed,
                    ReasonCode::ProviderUnavailable,
                    "source_missing",
                ));
            };
            provide_from_source(context, source).await
        })
    }

    fn list_capabilities(&self) -> Vec<membrane_provider_sdk::CapabilityV1> {
        vec![membrane_provider_sdk::CapabilityV1 {
            name: "membrane_audit".into(),
            schema_version: 1,
            error_version: 1,
        }]
    }
}

/// Execute one audit lane from an explicitly injected source.
pub async fn provide_from_source(
    context: &ProviderContext,
    source: &dyn AuditFindingSource,
) -> Result<ProviderOutput, ProviderError> {
    if context.is_cancelled() {
        return Err(ProviderError::Cancelled);
    }
    if context.is_deadline_exhausted() {
        return Err(ProviderError::DeadlineExceeded);
    }

    let response = match source.findings(&context.query()).await {
        Ok(response) => response,
        Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
        Err(ProviderError::DeadlineExceeded) => return Err(ProviderError::DeadlineExceeded),
        Err(error) => {
            return Ok(gap_output(
                FederationProviderStatusV1::Failed,
                reason_for_error(&error),
                detail_for_error(&error),
            ))
        }
    };

    if context.is_cancelled() {
        return Err(ProviderError::Cancelled);
    }
    if context.is_deadline_exhausted() {
        return Err(ProviderError::DeadlineExceeded);
    }

    // Prefer the snapshot generation over the release generation.
    //
    // A source here answers with the content identity of what it indexed;
    // the release generation is the Membrane build's own sha256. Checking
    // the build's identity first meant every source disagreed with it and
    // was gapped as generation_incoherent — measured on this machine as
    // seven such omissions and zero candidates on a request whose sources
    // had answered normally.
    let expected_generation = context
        .freshness
        .generation
        .as_deref()
        .or(context.release_generation.as_deref());
    let observed_generation = response.generation.clone().or_else(|| {
        response
            .value
            .first()
            .map(|finding| finding.generation.clone())
    });

    if let Some(expected) = expected_generation.filter(|value| !value.trim().is_empty()) {
        if observed_generation.as_deref() != Some(expected)
            || response
                .value
                .iter()
                .any(|finding| finding.generation != expected)
        {
            return Ok(generation_gap(
                expected,
                observed_generation.as_deref().unwrap_or(""),
            ));
        }
    } else if let Some(observed) = observed_generation.as_deref() {
        if response
            .value
            .iter()
            .any(|finding| finding.generation != observed)
        {
            return Ok(generation_gap(observed, "mixed"));
        }
    }

    let mut output = empty_output(observed_generation.clone());
    output
        .warnings
        .extend(response.warnings.iter().map(source_warning));
    for finding in response.value {
        match normalize_audit_finding(&finding, &context.repository_id, expected_generation) {
            Ok(candidate) => output.candidates.push(candidate),
            Err(reason) => output.omissions.push(ProviderOmissionV1 {
                provider: PROVIDER_ID,
                reason,
                candidate_id: nonempty(&finding.id),
                detail_id: Some("finding_malformed".into()),
                stage: Some("normalization".into()),
            }),
        }
    }

    if !response.complete {
        output.warnings.push(ProviderWarningV1 {
            provider: PROVIDER_ID,
            reason: ReasonCode::ProviderFailed,
            severity: WarningSeverity::Warning,
            detail_id: Some("source_incomplete".into()),
            stage: Some("source".into()),
            message: None,
        });
    }
    if response.complete
        && output.candidates.is_empty()
        && output.warnings.is_empty()
        && output.omissions.is_empty()
    {
        output.status = FederationProviderStatusV1::Partial;
        output.warnings.push(ProviderWarningV1 {
            provider: PROVIDER_ID,
            reason: ReasonCode::ProviderUnavailable,
            severity: WarningSeverity::Warning,
            detail_id: Some("source_complete_empty".into()),
            stage: Some("source".into()),
            message: None,
        });
    } else {
        output.status =
            if response.complete && output.warnings.is_empty() && output.omissions.is_empty() {
                FederationProviderStatusV1::Complete
            } else {
                FederationProviderStatusV1::Partial
            };
    }

    output
        .candidates
        .sort_by(|left, right| left.id.cmp(&right.id));
    output.warnings.sort_by(|left, right| {
        left.reason
            .as_str()
            .cmp(right.reason.as_str())
            .then_with(|| left.detail_id.cmp(&right.detail_id))
    });
    output.omissions.sort_by(|left, right| {
        left.reason
            .as_str()
            .cmp(right.reason.as_str())
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
            .then_with(|| left.detail_id.cmp(&right.detail_id))
    });
    if output.candidates.is_empty() && output.warnings.is_empty() && output.omissions.is_empty() {
        output.status = FederationProviderStatusV1::Partial;
        output.warnings.push(ProviderWarningV1 {
            provider: PROVIDER_ID,
            reason: ReasonCode::ProviderUnavailable,
            severity: WarningSeverity::Warning,
            detail_id: Some("no_eligible_findings".into()),
            stage: Some("normalization".into()),
            message: None,
        });
    }
    Ok(output)
}

/// Validate source binding while retaining the source-owned candidate fields.
pub fn normalize_audit_finding(
    finding: &AuditFinding,
    repository_id: &str,
    expected_generation: Option<&str>,
) -> Result<CandidateV1, ReasonCode> {
    if finding.id.trim().is_empty()
        || finding.repository_id.trim().is_empty()
        || finding.repository_id != repository_id
        || finding.generation.trim().is_empty()
        || finding.source_hash.trim().is_empty()
        || finding.candidate.id != finding.id
        || finding.candidate.source_hash != finding.source_hash
    {
        return Err(ReasonCode::ProviderMalformed);
    }
    if expected_generation.is_some_and(|expected| expected != finding.generation) {
        return Err(ReasonCode::GenerationIncoherent);
    }
    let candidate = &finding.candidate;
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
    {
        return Err(ReasonCode::ProviderMalformed);
    }
    let mut candidate = candidate.clone();
    candidate.provider = Some(PROVIDER_ID.as_str().to_owned());
    Ok(candidate)
}

fn empty_output(generation: Option<String>) -> ProviderOutputV1 {
    ProviderOutputV1 {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider: PROVIDER_ID,
        status: FederationProviderStatusV1::Partial,
        generation,
        candidates: Vec::new(),
        warnings: Vec::new(),
        omissions: Vec::new(),
        diagnostics: Some(ProviderDiagnosticsV1 {
            provider: PROVIDER_ID,
            elapsed_ms: None,
            generation: None,
            attributes: BTreeMap::from([(
                String::from("provenance"),
                String::from("audit-finding-source"),
            )]),
        }),
        extensions: BTreeMap::new(),
    }
}

fn gap_output(
    status: FederationProviderStatusV1,
    reason: ReasonCode,
    detail: &str,
) -> ProviderOutput {
    ProviderOutputV1 {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider: PROVIDER_ID,
        status,
        generation: None,
        candidates: Vec::new(),
        warnings: vec![ProviderWarningV1 {
            provider: PROVIDER_ID,
            reason,
            severity: WarningSeverity::Warning,
            detail_id: Some(detail.to_owned()),
            stage: Some("source".into()),
            message: None,
        }],
        omissions: vec![ProviderOmissionV1 {
            provider: PROVIDER_ID,
            reason,
            candidate_id: None,
            detail_id: Some(detail.to_owned()),
            stage: Some("source".into()),
        }],
        diagnostics: None,
        extensions: BTreeMap::new(),
    }
}

fn generation_gap(expected: &str, observed: &str) -> ProviderOutput {
    gap_output(
        FederationProviderStatusV1::Partial,
        ReasonCode::GenerationIncoherent,
        &format!("expected:{expected}:observed:{observed}"),
    )
}

fn source_warning(warning: &SourceWarning) -> ProviderWarningV1 {
    ProviderWarningV1 {
        provider: PROVIDER_ID,
        reason: ReasonCode::parse(&warning.code).unwrap_or(ReasonCode::ProviderFailed),
        severity: WarningSeverity::Warning,
        detail_id: warning
            .detail_id
            .clone()
            .or_else(|| nonempty(&warning.code)),
        stage: Some("source".into()),
        message: None,
    }
}

fn reason_for_error(error: &ProviderError) -> ReasonCode {
    match error {
        ProviderError::Unavailable(_) | ProviderError::MissingSource(_) => {
            ReasonCode::ProviderUnavailable
        }
        ProviderError::MalformedOutput(_) => ReasonCode::ProviderMalformed,
        ProviderError::IdentityMismatch(_) => ReasonCode::GenerationIncoherent,
        ProviderError::SourceFailure(_) => ReasonCode::ProviderFailed,
        _ => ReasonCode::ProviderFailed,
    }
}

fn detail_for_error(error: &ProviderError) -> &'static str {
    match error {
        ProviderError::Unavailable(_) | ProviderError::MissingSource(_) => "source_unavailable",
        ProviderError::MalformedOutput(_) => "source_malformed",
        ProviderError::IdentityMismatch(_) => "source_generation_mismatch",
        ProviderError::SourceFailure(_) => "source_failed",
        _ => "provider_failed",
    }
}

fn nonempty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.to_owned())
}
