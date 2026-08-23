//! Freshness acquisition and request-bound freshness policy.
//!
//! The source is injected by composition.  Unknown, stale, or incoherent
//! observations never become current through a local fallback.

use crate::release::ReleaseBinding;
use membrane_protocol::{FreshnessSnapshotV1, ReleaseGenerationStatus};
use membrane_provider_sdk::source::{FreshnessSource, SourceQuery};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreshnessRequirement {
    Any,
    Current,
    Exact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreshnessState {
    Current,
    Stale,
    Unknown,
    GenerationMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreshnessErrorCode {
    Unavailable,
    Malformed,
    GenerationMismatch,
    RequirementUnsatisfied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FreshnessError {
    Unavailable(String),
    Malformed(&'static str),
    GenerationMismatch,
    RequirementUnsatisfied,
}

impl FreshnessError {
    pub const fn code(&self) -> FreshnessErrorCode {
        match self {
            Self::Unavailable(_) => FreshnessErrorCode::Unavailable,
            Self::Malformed(_) => FreshnessErrorCode::Malformed,
            Self::GenerationMismatch => FreshnessErrorCode::GenerationMismatch,
            Self::RequirementUnsatisfied => FreshnessErrorCode::RequirementUnsatisfied,
        }
    }
}

impl fmt::Display for FreshnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(detail) => write!(f, "freshness_unavailable:{detail}"),
            Self::Malformed(detail) => write!(f, "freshness_malformed:{detail}"),
            Self::GenerationMismatch => f.write_str("freshness_generation_mismatch"),
            Self::RequirementUnsatisfied => f.write_str("freshness_requirement_unsatisfied"),
        }
    }
}

impl std::error::Error for FreshnessError {}

/// Freshness provenance bound before any provider content is acquired.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreshnessBinding {
    pub snapshot: FreshnessSnapshotV1,
    pub state: FreshnessState,
    pub source_id: Option<String>,
    pub provenance: Option<String>,
    pub release: Option<ReleaseBinding>,
}

impl FreshnessBinding {
    pub fn from_snapshot(
        snapshot: FreshnessSnapshotV1,
        release: Option<ReleaseBinding>,
    ) -> Result<Self, FreshnessError> {
        validate_snapshot(&snapshot)?;
        let mut state = if snapshot.stale {
            FreshnessState::Stale
        } else if snapshot.generation.as_deref().is_none() {
            FreshnessState::Unknown
        } else {
            FreshnessState::Current
        };
        if let Some(binding) = &release {
            if binding.status == ReleaseGenerationStatus::Mismatch {
                state = FreshnessState::GenerationMismatch;
            } else if binding.status != ReleaseGenerationStatus::Matched {
                state = FreshnessState::Unknown;
            }
            if let Some(observed) = snapshot.generation.as_deref() {
                if binding.expected_generation != observed {
                    state = FreshnessState::GenerationMismatch;
                }
            }
        }
        Ok(Self {
            snapshot,
            state,
            source_id: None,
            provenance: None,
            release,
        })
    }

    pub fn with_provenance(
        mut self,
        source_id: impl Into<String>,
        provenance: Option<String>,
    ) -> Self {
        self.source_id = Some(source_id.into());
        self.provenance = provenance;
        self
    }

    /// Acquire the central verdict through the injected SDK source.
    pub async fn acquire<S: FreshnessSource + ?Sized>(
        source: &S,
        query: &SourceQuery,
        release: Option<ReleaseBinding>,
    ) -> Result<Self, FreshnessError> {
        let response = source
            .freshness(query)
            .await
            .map_err(|error| FreshnessError::Unavailable(error.to_string()))?;
        if !response.complete {
            return Err(FreshnessError::Unavailable("source_incomplete".to_owned()));
        }
        let mut binding = Self::from_snapshot(response.value, release)?;
        binding.provenance = response
            .warnings
            .first()
            .and_then(|warning| warning.detail_id.clone());
        Ok(binding)
    }

    pub fn require(
        &self,
        requirement: FreshnessRequirement,
        expected_generation: Option<&str>,
    ) -> Result<(), FreshnessError> {
        match requirement {
            FreshnessRequirement::Any => Ok(()),
            FreshnessRequirement::Current => self.is_current().then_some(()).ok_or(
                FreshnessError::RequirementUnsatisfied,
            ),
            FreshnessRequirement::Exact => {
                if !self.is_current() {
                    return Err(FreshnessError::RequirementUnsatisfied);
                }
                let expected = expected_generation
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or(FreshnessError::Malformed("exact freshness requires generation"))?;
                (self.snapshot.generation.as_deref() == Some(expected))
                    .then_some(())
                    .ok_or(FreshnessError::GenerationMismatch)
            }
        }
    }

    pub fn is_current(&self) -> bool {
        self.state == FreshnessState::Current
    }

    pub fn warning_code(&self) -> Option<&'static str> {
        match self.state {
            FreshnessState::Current => None,
            FreshnessState::Stale => Some("freshness_stale"),
            FreshnessState::Unknown => Some("freshness_unavailable"),
            FreshnessState::GenerationMismatch => Some("generation_incoherent"),
        }
    }
}

fn validate_snapshot(snapshot: &FreshnessSnapshotV1) -> Result<(), FreshnessError> {
    let graph_state = snapshot.graph_state.trim();
    if graph_state.is_empty() {
        return Err(FreshnessError::Malformed("graph_state is empty"));
    }
    if snapshot.stale && snapshot.generation.is_none() {
        return Ok(());
    }
    if let Some(generation) = snapshot.generation.as_deref() {
        crate::release::validate_generation(generation)
            .map_err(|_| FreshnessError::Malformed("generation is invalid"))?;
    }
    if snapshot.overlay_digest.is_some() && snapshot.base_commit.is_none() {
        return Err(FreshnessError::Malformed("overlay requires base_commit"));
    }
    Ok(())
}
