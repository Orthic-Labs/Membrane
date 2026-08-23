//! Lane-complete warning and omission accounting.
//!
//! A missing or rejected provider lane remains visible.  This module carries
//! no candidate text and sorts all public fragments by fixed provider order.

use crate::normalize::{admit_generation, NormalizedProviderOutput};
use membrane_protocol::{
    ProviderId, ProviderOmissionV1, ProviderOutputV1, ProviderWarningV1, ReasonCode,
    WarningSeverity,
};

#[derive(Clone, Debug, PartialEq)]
pub struct LaneAccounting {
    pub outputs: Vec<NormalizedProviderOutput>,
    pub warnings: Vec<ProviderWarningV1>,
    pub omissions: Vec<ProviderOmissionV1>,
}

impl LaneAccounting {
    pub fn is_complete(&self) -> bool {
        self.omissions.is_empty() && self.warnings.is_empty()
    }

    pub fn canonicalize(&mut self) {
        self.outputs.sort_by_key(|output| output.provider.rank());
        self.warnings = canonical_warnings(std::mem::take(&mut self.warnings));
        self.omissions = canonical_omissions(std::mem::take(&mut self.omissions));
    }
}

/// A lane expected by the fixed provider set but absent from provider output.
pub fn missing_lane(provider: ProviderId) -> ProviderOmissionV1 {
    ProviderOmissionV1 {
        provider,
        reason: ReasonCode::ProviderUnavailable,
        candidate_id: None,
        detail_id: Some("lane_missing".to_owned()),
        stage: Some("lane_accounting".to_owned()),
    }
}

pub fn disabled_lane(provider: ProviderId) -> ProviderOmissionV1 {
    ProviderOmissionV1 {
        provider,
        reason: ReasonCode::ProviderUnavailable,
        candidate_id: None,
        detail_id: Some("provider_disabled".to_owned()),
        stage: Some("configuration".to_owned()),
    }
}

pub fn generation_omission(provider: ProviderId) -> ProviderOmissionV1 {
    ProviderOmissionV1 {
        provider,
        reason: ReasonCode::GenerationIncoherent,
        candidate_id: None,
        detail_id: Some("generation_incoherent".to_owned()),
        stage: Some("generation_admission".to_owned()),
    }
}

pub fn conflict_omission(provider: ProviderId, candidate_id: impl Into<String>) -> ProviderOmissionV1 {
    ProviderOmissionV1 {
        provider,
        reason: ReasonCode::CandidateIdentityConflict,
        candidate_id: Some(candidate_id.into()),
        detail_id: Some("candidate_identity_conflict".to_owned()),
        stage: Some("merge".to_owned()),
    }
}

pub fn warning_from_omission(omission: &ProviderOmissionV1) -> ProviderWarningV1 {
    ProviderWarningV1 {
        provider: omission.provider,
        reason: omission.reason,
        severity: WarningSeverity::Warning,
        detail_id: omission.detail_id.clone(),
        stage: omission.stage.clone(),
        message: None,
    }
}

/// Convert provider warnings into attributable response omissions.  Existing
/// explicit omissions are retained by the caller; this function only maps
/// warnings and never infers content or authority.
pub fn warnings_to_omissions(warnings: &[ProviderWarningV1]) -> Vec<ProviderOmissionV1> {
    warnings
        .iter()
        .map(|warning| ProviderOmissionV1 {
            provider: warning.provider,
            reason: warning.reason,
            candidate_id: None,
            detail_id: warning.detail_id.clone(),
            stage: warning.stage.clone().or_else(|| Some("provider".to_owned())),
        })
        .collect()
}

/// Reconcile expected lanes, admit generation before candidates reach merge,
/// and retain provider-local warning/omission accounting.
pub fn reconcile_lanes(
    expected: &[ProviderId],
    outputs: &[ProviderOutputV1],
    expected_generation: Option<&str>,
) -> Result<LaneAccounting, crate::normalize::CandidateNormalizationError> {
    let mut by_provider = std::collections::BTreeMap::new();
    for output in outputs {
        by_provider
            .entry(output.provider.as_str().to_owned())
            .or_insert(output);
    }
    let mut accounting = LaneAccounting {
        outputs: Vec::new(),
        warnings: Vec::new(),
        omissions: Vec::new(),
    };
    for provider in expected.iter().copied() {
        let Some(output) = by_provider.get(provider.as_str()).copied() else {
            accounting.omissions.push(missing_lane(provider));
            continue;
        };
        if admit_generation(output, expected_generation).is_err() {
            accounting.omissions.push(generation_omission(provider));
            continue;
        }
        let normalized = crate::normalize::normalize_provider_output(output, provider)?;
        accounting.warnings.extend(normalized.warnings.clone());
        accounting.omissions.extend(normalized.omissions.clone());
        accounting.outputs.push(normalized);
    }
    accounting.canonicalize();
    Ok(accounting)
}

pub fn canonical_warnings(mut warnings: Vec<ProviderWarningV1>) -> Vec<ProviderWarningV1> {
    warnings.sort_by(|left, right| {
        left.provider
            .rank()
            .cmp(&right.provider.rank())
            .then_with(|| left.reason.as_str().cmp(right.reason.as_str()))
            .then_with(|| left.detail_id.cmp(&right.detail_id))
            .then_with(|| left.stage.cmp(&right.stage))
    });
    warnings
}

pub fn canonical_omissions(mut omissions: Vec<ProviderOmissionV1>) -> Vec<ProviderOmissionV1> {
    omissions.sort_by(|left, right| {
        left.provider
            .rank()
            .cmp(&right.provider.rank())
            .then_with(|| left.reason.as_str().cmp(right.reason.as_str()))
            .then_with(|| left.detail_id.cmp(&right.detail_id))
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
            .then_with(|| left.stage.cmp(&right.stage))
    });
    omissions
}

pub fn expected_lane_ids() -> &'static [ProviderId; 9] {
    &ProviderId::ALL
}
