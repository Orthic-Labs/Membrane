//! Native architecture-decision lane.
//!
//! Architect owns decision lifecycle and persistence.  This adapter only
//! consumes the typed [`DecisionRecordSource`] projection and turns records
//! into ordinary, untrusted federation candidates.  It never reads the
//! workspace, starts a process, or interprets decision text as instructions.

use membrane_protocol::{
    sort_candidates, sort_omissions, sort_warnings, CandidateV1, FederationProviderStatusV1,
    ProviderDiagnosticsV1, ProviderId, ProviderOmissionV1, ProviderOutputV1, ProviderWarningV1,
    ReasonCode, WarningSeverity, PROVIDER_OUTPUT_SCHEMA_VERSION,
};
use membrane_provider_sdk::{
    CapabilityV1, DecisionRecord, DecisionRecordSource, Provider, ProviderContext, ProviderError,
    ProviderOutput, SourceResponse, SourceWarning,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

const REVISIT_MARKER: &str = "__membrane_revisit_trigger__:";
const PROVENANCE_MARKER: &str = "__membrane_provenance__:";

pub const PROVIDER_ID: ProviderId = ProviderId::Architect;
pub const LAYER: u8 = 5;
pub const MAX_CANDIDATES: usize = 40;
pub const MAX_ESTIMATED_TOKENS: u32 = 8_000;

#[derive(Clone, Copy, Debug, Default)]
pub struct ArchitectProvider;

impl ArchitectProvider {
    pub const fn new() -> Self {
        Self
    }

    async fn provide_inner(
        &self,
        context: &ProviderContext,
    ) -> Result<ProviderOutput, ProviderError> {
        if context.is_cancelled() {
            return Ok(gap(
                ProviderStatus::Cancelled,
                ReasonCode::ProviderCancelled,
                "request_cancelled",
            ));
        }
        if context.is_deadline_exhausted() {
            return Ok(gap(
                ProviderStatus::Cancelled,
                ReasonCode::DeadlineExhausted,
                "deadline_exhausted",
            ));
        }
        let Some(source) = context.sources.decisions.as_ref() else {
            return Ok(gap(
                ProviderStatus::Failed,
                ReasonCode::ProviderUnavailable,
                "decision_source_missing",
            ));
        };

        let mut query = context.query();
        // Decision records are linked to Blueprint generations, not release
        // generations.  The grant is the authoritative request binding.
        query.generation = context
            .scope_grant
            .as_ref()
            .map(|grant| grant.blueprint_generation.clone())
            .or_else(|| context.freshness.generation.clone());
        let expected_generation = query.generation.clone();
        let response = match source.decisions(&query).await {
            Ok(response) => response,
            Err(error) => return Ok(source_gap(error)),
        };
        if context.is_cancelled() {
            return Ok(gap(
                ProviderStatus::Cancelled,
                ReasonCode::ProviderCancelled,
                "request_cancelled",
            ));
        }
        if context.is_deadline_exhausted() {
            return Ok(gap(
                ProviderStatus::Cancelled,
                ReasonCode::DeadlineExhausted,
                "deadline_exhausted",
            ));
        }

        let mut output = build_output(context, response, expected_generation.as_deref());
        sort_candidates(&mut output.candidates);
        if output.candidates.len() > MAX_CANDIDATES {
            let dropped = output.candidates.split_off(MAX_CANDIDATES);
            output
                .omissions
                .extend(dropped.into_iter().map(|candidate| {
                    omission(
                        ReasonCode::ProviderMalformed,
                        Some(candidate.id),
                        "candidate_cap_exceeded",
                    )
                }));
            output.status = ProviderStatus::Partial;
        }
        sort_warnings(&mut output.warnings);
        sort_omissions(&mut output.omissions);
        Ok(output)
    }
}

type ProviderStatus = FederationProviderStatusV1;

impl Provider for ArchitectProvider {
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

    fn list_capabilities(&self) -> Vec<CapabilityV1> {
        Vec::new()
    }
}

/// Normalize one source-owned record without changing its stable identity or
/// source hash.  Scope and lifecycle filtering remain source-owned.
pub fn normalize_decision(
    record: &DecisionRecord,
    repository_id: &str,
    expected_generation: Option<&str>,
) -> Result<CandidateV1, DecisionNormalizationError> {
    if record.id.trim().is_empty() {
        return Err(DecisionNormalizationError::Missing("id"));
    }
    if record.repository_id != repository_id {
        return Err(DecisionNormalizationError::RepositoryMismatch);
    }
    if record.generation.trim().is_empty() {
        return Err(DecisionNormalizationError::Missing("generation"));
    }
    if let Some(expected) = expected_generation.filter(|value| !value.trim().is_empty()) {
        if record.generation != expected {
            return Err(DecisionNormalizationError::GenerationMismatch);
        }
    }
    if record.source_hash.trim().is_empty() {
        return Err(DecisionNormalizationError::Missing("sourceHash"));
    }
    if record.rationale.trim().is_empty() {
        return Err(DecisionNormalizationError::Missing("rationale"));
    }
    let text = decision_text(record);
    let mut score_components = BTreeMap::new();
    score_components.insert("scope".to_owned(), 1.0);
    score_components.insert("generation".to_owned(), 1.0);
    Ok(CandidateV1 {
        id: record.id.clone(),
        layer: LAYER,
        provider: Some(PROVIDER_ID.as_str().to_owned()),
        source_kind: "architect_decision".to_owned(),
        source_ref: format!("architect://decision:{}", record.id),
        source_hash: record.source_hash.clone(),
        trust_class: "agent_verified".to_owned(),
        instruction_policy: "data_only".to_owned(),
        provider_score: 0.8,
        score_components,
        base_commit: None,
        overlay_digest: None,
        freshness_class: None,
        snapshot_id: None,
        estimated_tokens: ((text.len() as u32).saturating_add(3) / 4).max(1),
        protected: false,
        exact: true,
        recoverable: true,
        resolver: format!("architect resolve {}", record.id),
        text,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecisionNormalizationError {
    Missing(&'static str),
    RepositoryMismatch,
    GenerationMismatch,
}

fn build_output(
    context: &ProviderContext,
    response: SourceResponse<Vec<DecisionRecord>>,
    expected_generation: Option<&str>,
) -> ProviderOutput {
    let generation = response.generation.clone().or_else(|| {
        response
            .value
            .first()
            .map(|record| record.generation.clone())
    });
    let mut output = empty_output(generation);
    output.warnings = response.warnings.iter().map(source_warning).collect();
    let mut evidence = Vec::new();
    for record in response.value {
        match normalize_decision(&record, &context.repository_id, expected_generation) {
            Ok(candidate) => {
                evidence.push(json!({
                    "id": record.id,
                    "sourceHash": record.source_hash,
                    "rationale": record.rationale,
                    "alternatives": record.alternatives,
                    "risks": record.risks,
                    "revisitTriggers": revisit_triggers(&record),
                    "provenance": provenance(&record, &candidate),
                    "generation": record.generation,
                }));
                output.candidates.push(candidate);
            }
            Err(error) => {
                let (reason, detail) = match error {
                    DecisionNormalizationError::GenerationMismatch => {
                        (ReasonCode::GenerationIncoherent, "generation_incoherent")
                    }
                    DecisionNormalizationError::RepositoryMismatch => {
                        (ReasonCode::ProviderMalformed, "repository_mismatch")
                    }
                    DecisionNormalizationError::Missing(_) => {
                        (ReasonCode::ProviderMalformed, "decision_malformed")
                    }
                };
                output
                    .omissions
                    .push(omission(reason, Some(record.id), detail));
            }
        }
    }
    if !response.complete {
        output
            .warnings
            .push(warning(ReasonCode::ProviderFailed, "source_incomplete"));
    }
    let mut admitted_tokens = 0u32;
    let mut retained = Vec::with_capacity(output.candidates.len());
    for candidate in output.candidates.drain(..) {
        let next = admitted_tokens.saturating_add(candidate.estimated_tokens);
        if next > MAX_ESTIMATED_TOKENS {
            output.omissions.push(omission(
                ReasonCode::ProviderMalformed,
                Some(candidate.id),
                "token_ceiling_exceeded",
            ));
        } else {
            admitted_tokens = next;
            retained.push(candidate);
        }
    }
    output.candidates = retained;
    if output.candidates.is_empty() && output.warnings.is_empty() && output.omissions.is_empty() {
        output.warnings.push(warning(
            ReasonCode::ProviderUnavailable,
            "source_complete_empty",
        ));
    }
    output
        .extensions
        .insert("decisionEvidence".to_owned(), json!(evidence));
    output.status =
        if response.complete && output.warnings.is_empty() && output.omissions.is_empty() {
            ProviderStatus::Complete
        } else if output.candidates.is_empty() {
            ProviderStatus::Failed
        } else {
            ProviderStatus::Partial
        };
    output
}

fn decision_text(record: &DecisionRecord) -> String {
    let mut text = format!("Architect decision: {}", record.rationale.trim());
    if !record.alternatives.is_empty() {
        text.push_str(" alternatives=");
        text.push_str(&record.alternatives.join(" | "));
    }
    let risks = record
        .risks
        .iter()
        .filter(|risk| !risk.starts_with(REVISIT_MARKER) && !risk.starts_with(PROVENANCE_MARKER))
        .collect::<Vec<_>>();
    if !risks.is_empty() {
        text.push_str(" risks=");
        text.push_str(
            &risks
                .iter()
                .map(|risk| risk.as_str())
                .collect::<Vec<_>>()
                .join(" | "),
        );
    }
    text
}

fn revisit_triggers(record: &DecisionRecord) -> Vec<String> {
    record
        .risks
        .iter()
        .filter_map(|risk| risk.strip_prefix(REVISIT_MARKER).map(str::to_owned))
        .filter(|trigger| !trigger.trim().is_empty())
        .collect()
}

fn provenance(record: &DecisionRecord, candidate: &CandidateV1) -> Vec<String> {
    let mut values = record
        .risks
        .iter()
        .filter_map(|risk| risk.strip_prefix(PROVENANCE_MARKER).map(str::to_owned))
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>();
    if values.is_empty() {
        values.push(candidate.source_ref.clone());
    }
    values
}

fn empty_output(generation: Option<String>) -> ProviderOutput {
    ProviderOutputV1 {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider: PROVIDER_ID,
        status: ProviderStatus::Partial,
        generation,
        candidates: Vec::new(),
        warnings: Vec::new(),
        omissions: Vec::new(),
        diagnostics: Some(ProviderDiagnosticsV1 {
            provider: PROVIDER_ID,
            elapsed_ms: None,
            generation: None,
            attributes: BTreeMap::from([(
                "provenance".to_owned(),
                "architect-decision-source".to_owned(),
            )]),
        }),
        extensions: BTreeMap::new(),
    }
}

fn source_gap(error: ProviderError) -> ProviderOutput {
    let (reason, detail) = match error {
        ProviderError::MissingSource(_) | ProviderError::Unavailable(_) => (
            ReasonCode::ProviderUnavailable,
            "decision_source_unavailable",
        ),
        ProviderError::MalformedOutput(_) => {
            (ReasonCode::ProviderMalformed, "decision_source_malformed")
        }
        ProviderError::IdentityMismatch(_) => {
            (ReasonCode::GenerationIncoherent, "generation_incoherent")
        }
        ProviderError::Cancelled => (ReasonCode::ProviderCancelled, "request_cancelled"),
        ProviderError::DeadlineExceeded => (ReasonCode::DeadlineExhausted, "deadline_exhausted"),
        _ => (ReasonCode::ProviderFailed, "decision_source_failed"),
    };
    gap(ProviderStatus::Failed, reason, detail)
}

fn gap(status: ProviderStatus, reason: ReasonCode, detail: &'static str) -> ProviderOutput {
    let mut output = empty_output(None);
    output.status = status;
    output.warnings.push(warning(reason, detail));
    output.omissions.push(omission(reason, None, detail));
    output
}

fn warning(reason: ReasonCode, detail: &'static str) -> ProviderWarningV1 {
    ProviderWarningV1 {
        provider: PROVIDER_ID,
        reason,
        severity: WarningSeverity::Warning,
        detail_id: Some(detail.to_owned()),
        stage: Some("decision_source".to_owned()),
        message: None,
    }
}

fn omission(
    reason: ReasonCode,
    candidate_id: Option<String>,
    detail: &'static str,
) -> ProviderOmissionV1 {
    ProviderOmissionV1 {
        provider: PROVIDER_ID,
        reason,
        candidate_id,
        detail_id: Some(detail.to_owned()),
        stage: Some("decision_normalization".to_owned()),
    }
}

fn source_warning(source: &SourceWarning) -> ProviderWarningV1 {
    ProviderWarningV1 {
        provider: PROVIDER_ID,
        reason: ReasonCode::parse(&source.code).unwrap_or(ReasonCode::ProviderFailed),
        severity: WarningSeverity::Warning,
        detail_id: source.detail_id.clone(),
        stage: Some("decision_source".to_owned()),
        message: None,
    }
}
