//! Release-generation compatibility binding.
//!
//! Release identity is an injected observation.  This module deliberately
//! does not read manifests, spawn processes, or infer an identity from local
//! repository contents.  Composition supplies the release source and chooses
//! its transport.

use membrane_protocol::ReleaseGenerationStatus;
use std::fmt;

pub const RELEASE_GENERATION_PREFIX: &str = "sha256:";

/// A release observation with content-free provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseIdentity {
    pub generation: String,
    pub source_id: String,
    pub provenance: Option<String>,
}

impl ReleaseIdentity {
    pub fn new(
        generation: impl Into<String>,
        source_id: impl Into<String>,
        provenance: Option<String>,
    ) -> Result<Self, ReleaseError> {
        let identity = Self {
            generation: generation.into(),
            source_id: source_id.into(),
            provenance,
        };
        validate_generation(&identity.generation)?;
        if identity.source_id.trim().is_empty() {
            return Err(ReleaseError::Malformed("source identity is empty"));
        }
        Ok(identity)
    }
}

/// Owner-provided release observation.  Implementations must not derive a
/// permissive identity when the release source is unavailable.
pub trait ReleaseSource {
    fn current_release(&self) -> Result<ReleaseIdentity, ReleaseError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseErrorCode {
    Unavailable,
    Malformed,
    Mismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReleaseError {
    Unavailable(String),
    Malformed(&'static str),
    Mismatch { expected: String, observed: String },
}

impl ReleaseError {
    pub const fn code(&self) -> ReleaseErrorCode {
        match self {
            Self::Unavailable(_) => ReleaseErrorCode::Unavailable,
            Self::Malformed(_) => ReleaseErrorCode::Malformed,
            Self::Mismatch { .. } => ReleaseErrorCode::Mismatch,
        }
    }
}

impl fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(detail) => write!(f, "release_unavailable:{detail}"),
            Self::Malformed(detail) => write!(f, "release_malformed:{detail}"),
            Self::Mismatch { expected, observed } => {
                write!(f, "release_generation_mismatch:{expected}:{observed}")
            }
        }
    }
}

impl std::error::Error for ReleaseError {}

/// Immutable release compatibility evidence retained for provider context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseBinding {
    pub expected_generation: String,
    pub observed_generation: Option<String>,
    pub status: ReleaseGenerationStatus,
    pub source_id: String,
    pub provenance: Option<String>,
}

impl ReleaseBinding {
    /// Resolve the current release and compare an optional request/receipt
    /// observation.  An absent observation is unavailable, never current.
    pub fn resolve<S: ReleaseSource + ?Sized>(
        source: &S,
        observed_generation: Option<&str>,
    ) -> Result<Self, ReleaseError> {
        let identity = source.current_release()?;
        validate_generation(&identity.generation)?;
        let observed = observed_generation.map(str::trim).filter(|value| !value.is_empty());
        if let Some(value) = observed {
            validate_generation(value)?;
        }
        let status = match observed {
            Some(value) if value == identity.generation => ReleaseGenerationStatus::Matched,
            Some(value) => {
                return Err(ReleaseError::Mismatch {
                    expected: identity.generation,
                    observed: value.to_owned(),
                });
            }
            None => ReleaseGenerationStatus::Unavailable,
        };
        Ok(Self {
            expected_generation: identity.generation,
            observed_generation: observed.map(str::to_owned),
            status,
            source_id: identity.source_id,
            provenance: identity.provenance,
        })
    }

    pub fn from_observation(
        identity: ReleaseIdentity,
        observed_generation: Option<&str>,
    ) -> Result<Self, ReleaseError> {
        struct StaticRelease(ReleaseIdentity);
        impl ReleaseSource for StaticRelease {
            fn current_release(&self) -> Result<ReleaseIdentity, ReleaseError> {
                Ok(self.0.clone())
            }
        }
        Self::resolve(&StaticRelease(identity), observed_generation)
    }

    pub fn is_compatible(&self) -> bool {
        self.status == ReleaseGenerationStatus::Matched
    }

    pub fn warning_code(&self) -> Option<&'static str> {
        match self.status {
            ReleaseGenerationStatus::Matched => None,
            ReleaseGenerationStatus::Mismatch => Some("release_generation_mismatch"),
            ReleaseGenerationStatus::Unavailable => Some("release_generation_unavailable"),
        }
    }
}

pub fn validate_generation(value: &str) -> Result<(), ReleaseError> {
    let digest = value.strip_prefix(RELEASE_GENERATION_PREFIX).ok_or(
        ReleaseError::Malformed("generation must use sha256:<64 hex>"),
    )?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ReleaseError::Malformed("generation must use sha256:<64 hex>"));
    }
    Ok(())
}
