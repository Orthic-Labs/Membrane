//! Deterministic federation candidate merge.
//!
//! Merge is intentionally boring: normalize and admit lanes first, group by
//! stable identity, collapse exact duplicates, omit conflicts, then apply the
//! fixed provider/security ordering.  Completion order never reaches output.

use crate::normalize::{normalize_provider_output, NormalizedCandidate, NormalizedProviderOutput};
use crate::omission::{canonical_omissions, canonical_warnings, conflict_omission, reconcile_lanes};
use membrane_protocol::{
    canonical_json_of, FederationProviderStatusV1, FederationResponseV1, FederationStatus,
    ProviderId, ProviderOmissionV1, ProviderOutputV1, ProviderWarningV1, ReasonCode,
    WarningSeverity, FEDERATION_RESPONSE_SCHEMA_VERSION,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub struct MergeResult {
    pub candidates: Vec<NormalizedCandidate>,
    pub warnings: Vec<ProviderWarningV1>,
    pub omissions: Vec<ProviderOmissionV1>,
    pub providers: Vec<NormalizedProviderOutput>,
}

impl MergeResult {
    pub fn canonicalize(&mut self) {
        self.candidates.sort_by(candidate_order);
        self.providers.sort_by_key(|provider| provider.provider.rank());
        self.warnings = canonical_warnings(std::mem::take(&mut self.warnings));
        self.omissions = canonical_omissions(std::mem::take(&mut self.omissions));
    }

    pub fn is_partial(&self) -> bool {
        !self.warnings.is_empty() || !self.omissions.is_empty()
    }

    pub fn response(
        &self,
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> FederationResponseV1 {
        let mut response = FederationResponseV1 {
            schema_version: FEDERATION_RESPONSE_SCHEMA_VERSION,
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            status: if self.is_partial() {
                FederationStatus::Partial
            } else {
                FederationStatus::Complete
            },
            providers: self
                .providers
                .iter()
                .map(provider_output)
                .collect(),
            candidates: self
                .candidates
                .iter()
                .map(|candidate| candidate.candidate.clone())
                .collect(),
            warnings: self.warnings.clone(),
            omissions: self.omissions.clone(),
            diagnostics: None,
            error: None,
            extensions: BTreeMap::new(),
        };
        response.canonicalize_collections();
        response
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MergeError {
    #[error("provider output normalization failed: {0}")]
    Normalization(#[from] crate::normalize::CandidateNormalizationError),
    #[error("provider output does not contain expected lane")]
    MissingProvider,
}

/// Normalize all expected provider lanes, perform generation admission before
/// dedupe, and merge deterministic candidates.
pub fn merge_outputs(
    expected: &[ProviderId],
    outputs: &[ProviderOutputV1],
    expected_generation: Option<&str>,
) -> Result<MergeResult, MergeError> {
    let accounting = reconcile_lanes(expected, outputs, expected_generation)?;
    Ok(merge_normalized(
        accounting.outputs,
        accounting.warnings,
        accounting.omissions,
    ))
}

/// Merge already-normalized lanes.  This is the integration seam for a
/// scheduler that has performed lane accounting itself.
pub fn merge_normalized(
    providers: Vec<NormalizedProviderOutput>,
    mut warnings: Vec<ProviderWarningV1>,
    mut omissions: Vec<ProviderOmissionV1>,
) -> MergeResult {
    let mut by_id: BTreeMap<String, Vec<NormalizedCandidate>> = BTreeMap::new();
    for provider in &providers {
        for candidate in &provider.candidates {
            by_id.entry(candidate.id().to_owned()).or_default().push(candidate.clone());
        }
    }

    let mut candidates = Vec::new();
    for (id, mut entries) in by_id {
        entries.sort_by(candidate_order);
        let first = entries.first().expect("group is never empty");
        if entries.iter().all(|entry| equivalent_identity(first, entry)) {
            candidates.push(first.clone());
            continue;
        }

        // The sealed parity corpus defines duplicate IDs across provider
        // lanes as canonical first-wins: provider order is the authority for
        // deterministic identity ownership.  Conflicting observations within
        // one provider remain unsafe and are omitted below.
        let first_provider = first.provider;
        let first_provider_entries = entries
            .iter()
            .filter(|entry| entry.provider == first_provider)
            .collect::<Vec<_>>();
        if first_provider_entries
            .iter()
            .all(|entry| equivalent_identity(first_provider_entries[0], entry))
        {
            candidates.push(first.clone());
            continue;
        }

        // A conflict has no safe winner.  Exclude every identity and leave a
        // content-free receipt for each involved lane.
        let mut providers_seen = BTreeSet::new();
        for entry in entries {
            if providers_seen.insert(entry.provider.as_str().to_owned()) {
                omissions.push(conflict_omission(entry.provider, id.clone()));
                warnings.push(ProviderWarningV1 {
                    provider: entry.provider,
                    reason: ReasonCode::CandidateIdentityConflict,
                    severity: WarningSeverity::Warning,
                    detail_id: Some(id.clone()),
                    stage: Some("merge".to_owned()),
                    message: None,
                });
            }
        }
    }

    let mut result = MergeResult {
        candidates,
        warnings,
        omissions,
        providers,
    };
    result.canonicalize();
    result
}

/// Convenience response-producing entry point used by resident composition.
pub fn merge_provider_outputs(
    request_id: impl Into<String>,
    trace_id: impl Into<String>,
    expected: &[ProviderId],
    outputs: &[ProviderOutputV1],
    expected_generation: Option<&str>,
) -> Result<FederationResponseV1, MergeError> {
    Ok(merge_outputs(expected, outputs, expected_generation)?.response(request_id, trace_id))
}

pub fn canonical_response_bytes(response: &FederationResponseV1) -> String {
    let mut response = response.clone();
    response.canonicalize_collections();
    canonical_json_of(&response)
}

pub fn candidate_order(left: &NormalizedCandidate, right: &NormalizedCandidate) -> Ordering {
    left.provider
        .rank()
        .cmp(&right.provider.rank())
        .then_with(|| right.candidate.protected.cmp(&left.candidate.protected))
        .then_with(|| right.candidate.exact.cmp(&left.candidate.exact))
        .then_with(|| left.candidate.id.cmp(&right.candidate.id))
        .then_with(|| left.candidate.source_hash.cmp(&right.candidate.source_hash))
        .then_with(|| left.candidate.source_ref.cmp(&right.candidate.source_ref))
        .then_with(|| left.candidate.text.cmp(&right.candidate.text))
        .then_with(|| left.canonical_bytes().cmp(&right.canonical_bytes()))
}

fn equivalent_identity(left: &NormalizedCandidate, right: &NormalizedCandidate) -> bool {
    left.provider == right.provider
        && left.provider_version == right.provider_version
        && left.generation == right.generation
        && left.provenance == right.provenance
        && left.semantic_bytes() == right.semantic_bytes()
}

fn provider_output(provider: &NormalizedProviderOutput) -> ProviderOutputV1 {
    ProviderOutputV1 {
        schema_version: provider.schema_version,
        provider: provider.provider,
        status: provider.status,
        generation: provider.generation.clone(),
        candidates: provider
            .candidates
            .iter()
            .map(|candidate| candidate.candidate.clone())
            .collect(),
        warnings: provider.warnings.clone(),
        omissions: provider.omissions.clone(),
        diagnostics: None,
        extensions: BTreeMap::new(),
    }
}

/// Normalize one output for schedulers that want to retain explicit lane
/// status without performing a full merge.
pub fn normalize_lane(
    output: &ProviderOutputV1,
) -> Result<NormalizedProviderOutput, crate::normalize::CandidateNormalizationError> {
    normalize_provider_output(output, output.provider)
}

/// Construct a provider-local warning without carrying message text into
/// canonical merge decisions.
pub fn merge_warning(provider: ProviderId, reason: ReasonCode, detail_id: Option<String>) -> ProviderWarningV1 {
    ProviderWarningV1 {
        provider,
        reason,
        severity: WarningSeverity::Warning,
        detail_id,
        stage: Some("merge".to_owned()),
        message: None,
    }
}

/// Provider status contributes to response degradation, but does not reorder
/// or erase candidates from other lanes.
pub fn has_failed_lane(providers: &[NormalizedProviderOutput]) -> bool {
    providers.iter().any(|provider| {
        matches!(
            provider.status,
            FederationProviderStatusV1::Partial
                | FederationProviderStatusV1::Failed
                | FederationProviderStatusV1::Cancelled
        )
    })
}
