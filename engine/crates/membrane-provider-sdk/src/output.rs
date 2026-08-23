//! Federation provider output aliases and contract validation.

use crate::error::{ProviderError, Result};
use membrane_protocol::{
    FederationProviderStatusV1, ProviderId, ProviderOutputV1, PROVIDER_OUTPUT_SCHEMA_VERSION,
};

/// Canonical native federation output.  The protocol crate remains the sole
/// wire-shape authority; this alias prevents a second SDK envelope.
pub type ProviderOutput = ProviderOutputV1;
pub type ProviderStatusV1 = FederationProviderStatusV1;

pub fn empty_output(provider: ProviderId, status: FederationProviderStatusV1) -> ProviderOutput {
    ProviderOutput {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider,
        status,
        generation: None,
        candidates: Vec::new(),
        warnings: Vec::new(),
        omissions: Vec::new(),
        diagnostics: None,
        extensions: Default::default(),
    }
}

/// Validate provider-local identity, generation, and explicit coverage before
/// federation admits an output.
pub fn validate_output(
    output: &ProviderOutput,
    expected_provider: ProviderId,
    expected_generation: Option<&str>,
) -> Result<()> {
    if output.schema_version != PROVIDER_OUTPUT_SCHEMA_VERSION {
        return Err(ProviderError::MalformedOutput(format!(
            "unsupported schema version {}",
            output.schema_version
        )));
    }
    if output.provider != expected_provider {
        return Err(ProviderError::IdentityMismatch(format!(
            "expected provider {}, got {}",
            expected_provider, output.provider
        )));
    }
    if let Some(expected) = expected_generation {
        if output.generation.as_deref() != Some(expected) {
            return Err(ProviderError::IdentityMismatch(format!(
                "expected generation {expected}, got {:?}",
                output.generation
            )));
        }
    }
    if output
        .candidates
        .iter()
        .any(|candidate| candidate.provider.as_deref() != Some(expected_provider.as_str()))
    {
        return Err(ProviderError::MalformedOutput(
            "candidate provider provenance does not match output provider".into(),
        ));
    }
    if output
        .warnings
        .iter()
        .any(|warning| warning.provider != expected_provider)
        || output
            .omissions
            .iter()
            .any(|omission| omission.provider != expected_provider)
    {
        return Err(ProviderError::MalformedOutput(
            "warning or omission provider provenance does not match output provider".into(),
        ));
    }
    if output.status == FederationProviderStatusV1::Complete
        && output.candidates.is_empty()
        && output.warnings.is_empty()
        && output.omissions.is_empty()
    {
        return Err(ProviderError::Incomplete(
            "empty complete output has no coverage or gap accounting".into(),
        ));
    }
    if matches!(
        output.status,
        FederationProviderStatusV1::Partial
            | FederationProviderStatusV1::Failed
            | FederationProviderStatusV1::Cancelled
    )
        && output.warnings.is_empty()
        && output.omissions.is_empty()
    {
        return Err(ProviderError::Incomplete(
            "non-complete output must carry warning or omission accounting".into(),
        ));
    }
    Ok(())
}
