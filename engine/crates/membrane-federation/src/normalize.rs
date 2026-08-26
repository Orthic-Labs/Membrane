//! Candidate normalization and generation admission for Pull federation.
//!
//! Provider payloads are untrusted lane input.  This module gives every
//! candidate one owned, typed representation before merge.  It intentionally
//! does not manufacture authority, freshness, or generation values.

use membrane_protocol::canonical_json_of;
use membrane_protocol::{
    CandidateV1, ProviderId, ProviderOmissionV1, ProviderOutputV1, ProviderWarningV1, ReasonCode,
    FEDERATION_RESPONSE_SCHEMA_VERSION, PROVIDER_OUTPUT_SCHEMA_VERSION,
};
use std::collections::BTreeMap;
use std::fmt;

/// The provider envelope version retained beside every normalized candidate.
pub const NORMALIZED_PROVIDER_VERSION: u32 = PROVIDER_OUTPUT_SCHEMA_VERSION;

/// Closed policy labels admitted by the existing federation/planner
/// contract.  Unknown or executable-like labels must never be promoted by a
/// normalizer into an authority-bearing default.
pub const SUPPORTED_INSTRUCTION_POLICIES: [&str; 3] = ["data_only", "none", "advisory"];

#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedCandidate {
    pub provider: ProviderId,
    pub provider_version: u32,
    pub generation: Option<String>,
    pub provenance: Option<String>,
    pub candidate: CandidateV1,
}

impl NormalizedCandidate {
    pub fn id(&self) -> &str {
        &self.candidate.id
    }

    /// Canonical semantic and security payload used by idempotent dedupe.
    /// Provider identity and generation are retained separately because they
    /// describe the lane envelope rather than candidate body semantics.
    pub fn semantic_bytes(&self) -> String {
        canonical_json_of(&self.candidate)
    }

    pub fn canonical_bytes(&self) -> String {
        let mut value = self.semantic_bytes();
        value.push('|');
        value.push_str(self.provider.as_str());
        value.push('|');
        value.push_str(self.generation.as_deref().unwrap_or(""));
        value.push('|');
        value.push_str(self.provenance.as_deref().unwrap_or(""));
        value
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedProviderOutput {
    pub schema_version: u32,
    pub provider: ProviderId,
    pub status: membrane_protocol::FederationProviderStatusV1,
    pub generation: Option<String>,
    pub candidates: Vec<NormalizedCandidate>,
    pub warnings: Vec<ProviderWarningV1>,
    pub omissions: Vec<ProviderOmissionV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CandidateNormalizationError {
    #[error("provider output schema version is unsupported")]
    SchemaVersion,
    #[error("provider output provider does not match expected lane")]
    ProviderMismatch,
    #[error("provider generation is malformed")]
    GenerationMalformed,
    #[error("provider generation does not match request binding")]
    GenerationMismatch,
    #[error("candidate field is empty: {0}")]
    EmptyField(&'static str),
    #[error("candidate score is not finite or is outside [0, 1]")]
    InvalidScore,
    #[error("candidate score component is not finite")]
    InvalidScoreComponent,
    #[error("candidate instruction policy is unsupported")]
    InvalidInstructionPolicy,
}

impl CandidateNormalizationError {
    pub const fn reason(&self) -> ReasonCode {
        match self {
            Self::GenerationMismatch | Self::GenerationMalformed => {
                ReasonCode::GenerationIncoherent
            }
            _ => ReasonCode::ProviderMalformed,
        }
    }
}

impl fmt::Display for NormalizedCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.provider, self.candidate.id)
    }
}

/// Normalize one candidate and stamp the expected provider identity.  The
/// provider field in a candidate is never trusted as an authority claim.
pub fn normalize_candidate(
    provider: ProviderId,
    provider_version: u32,
    generation: Option<&str>,
    provenance: Option<&str>,
    mut candidate: CandidateV1,
) -> Result<NormalizedCandidate, CandidateNormalizationError> {
    if provider_version != NORMALIZED_PROVIDER_VERSION {
        return Err(CandidateNormalizationError::SchemaVersion);
    }
    let generation = normalize_generation(generation)?;
    let provenance = normalize_optional(provenance);
    validate_candidate(&candidate)?;
    candidate.provider = Some(provider.as_str().to_owned());
    Ok(NormalizedCandidate {
        provider,
        provider_version,
        generation,
        provenance,
        candidate,
    })
}

/// Validate and normalize a complete provider output.  Generation admission
/// is deliberately a separate step so callers can account for an omitted
/// lane before deduplication.
pub fn normalize_provider_output(
    output: &ProviderOutputV1,
    expected_provider: ProviderId,
) -> Result<NormalizedProviderOutput, CandidateNormalizationError> {
    if output.schema_version != PROVIDER_OUTPUT_SCHEMA_VERSION {
        return Err(CandidateNormalizationError::SchemaVersion);
    }
    if output.provider != expected_provider {
        return Err(CandidateNormalizationError::ProviderMismatch);
    }
    let generation = normalize_generation(output.generation.as_deref())?;
    let provenance = output
        .diagnostics
        .as_ref()
        .and_then(|diagnostics| diagnostics.attributes.get("provenance"))
        .map(String::as_str);
    let candidates = output
        .candidates
        .iter()
        .cloned()
        .map(|candidate| {
            normalize_candidate(
                expected_provider,
                output.schema_version,
                generation.as_deref(),
                provenance,
                candidate,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_attribution(&output.warnings, &output.omissions, expected_provider)?;
    Ok(NormalizedProviderOutput {
        schema_version: output.schema_version,
        provider: expected_provider,
        status: output.status,
        generation,
        candidates,
        warnings: output.warnings.clone(),
        omissions: output.omissions.clone(),
    })
}

/// Generation-sealed lanes are admitted only when their observed generation
/// equals request binding.  Missing generation is incoherent when a binding
/// exists; no local fallback is permitted.
pub fn admit_generation(
    output: &ProviderOutputV1,
    expected_generation: Option<&str>,
) -> Result<(), ProviderOmissionV1> {
    let Some(expected) = expected_generation
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    if output.generation.as_deref() == Some(expected) {
        return Ok(());
    }
    Err(ProviderOmissionV1 {
        provider: output.provider,
        reason: ReasonCode::GenerationIncoherent,
        candidate_id: None,
        detail_id: Some("generation_incoherent".to_owned()),
        stage: Some("generation_admission".to_owned()),
    })
}

pub fn generation_admission(
    provider: ProviderId,
    observed_generation: Option<&str>,
    expected_generation: Option<&str>,
) -> Result<(), ProviderOmissionV1> {
    let output = ProviderOutputV1 {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider,
        status: membrane_protocol::FederationProviderStatusV1::Complete,
        generation: observed_generation.map(str::to_owned),
        candidates: Vec::new(),
        warnings: Vec::new(),
        omissions: Vec::new(),
        diagnostics: None,
        extensions: BTreeMap::new(),
    };
    admit_generation(&output, expected_generation)
}

fn normalize_generation(
    value: Option<&str>,
) -> Result<Option<String>, CandidateNormalizationError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some(digest) = value.strip_prefix("sha256:") else {
        return Err(CandidateNormalizationError::GenerationMalformed);
    };
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CandidateNormalizationError::GenerationMalformed);
    }
    Ok(Some(value.to_ascii_lowercase()))
}

fn normalize_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn validate_candidate(candidate: &CandidateV1) -> Result<(), CandidateNormalizationError> {
    for (field, value) in [
        ("id", candidate.id.as_str()),
        ("source_kind", candidate.source_kind.as_str()),
        ("source_ref", candidate.source_ref.as_str()),
        ("source_hash", candidate.source_hash.as_str()),
        ("trust_class", candidate.trust_class.as_str()),
        ("instruction_policy", candidate.instruction_policy.as_str()),
        ("resolver", candidate.resolver.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(CandidateNormalizationError::EmptyField(field));
        }
    }
    if !candidate.provider_score.is_finite() || !(0.0..=1.0).contains(&candidate.provider_score) {
        return Err(CandidateNormalizationError::InvalidScore);
    }
    if !SUPPORTED_INSTRUCTION_POLICIES
        .iter()
        .any(|policy| *policy == candidate.instruction_policy.trim())
    {
        return Err(CandidateNormalizationError::InvalidInstructionPolicy);
    }
    if candidate
        .score_components
        .values()
        .any(|value| !value.is_finite())
    {
        return Err(CandidateNormalizationError::InvalidScoreComponent);
    }
    Ok(())
}

fn validate_attribution(
    warnings: &[ProviderWarningV1],
    omissions: &[ProviderOmissionV1],
    provider: ProviderId,
) -> Result<(), CandidateNormalizationError> {
    if warnings.iter().any(|warning| warning.provider != provider)
        || omissions
            .iter()
            .any(|omission| omission.provider != provider)
    {
        return Err(CandidateNormalizationError::ProviderMismatch);
    }
    Ok(())
}

/// Build one typed omission for malformed lane output while retaining only
/// content-free identifiers.
pub fn malformed_omission(
    provider: ProviderId,
    detail_id: impl Into<String>,
) -> ProviderOmissionV1 {
    ProviderOmissionV1 {
        provider,
        reason: ReasonCode::ProviderMalformed,
        candidate_id: None,
        detail_id: Some(detail_id.into()),
        stage: Some("normalization".to_owned()),
    }
}

pub fn canonical_candidate_bytes(candidate: &NormalizedCandidate) -> String {
    candidate.canonical_bytes()
}

/// Kept private to this module but intentionally exercised by merge callers
/// through `NormalizedCandidate::semantic_bytes`.
#[allow(dead_code)]
fn _response_schema_version() -> u32 {
    FEDERATION_RESPONSE_SCHEMA_VERSION
}
