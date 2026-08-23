//! Versioned, typed federation configuration.

use crate::error::ConfigError;
use membrane_protocol::{ProviderId, ProviderOmissionV1, ReasonCode};
use serde::{Deserialize, Serialize};

pub const FEDERATION_CONFIG_SCHEMA_VERSION: u32 = 1;

/// One explicit lane switch.  Disabled lanes remain expected lanes and are
/// represented by an omission during composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderConfig {
    pub id: ProviderId,
    pub enabled: bool,
}

impl ProviderConfig {
    pub const fn enabled(id: ProviderId) -> Self {
        Self { id, enabled: true }
    }

    pub const fn disabled(id: ProviderId) -> Self {
        Self { id, enabled: false }
    }
}

/// Complete configuration for the fixed federation provider set.
///
/// A vector is used deliberately: validation can detect duplicate entries
/// instead of silently applying last-write-wins map semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FederationConfig {
    pub schema_version: u32,
    pub providers: Vec<ProviderConfig>,
}

impl Default for FederationConfig {
    fn default() -> Self {
        Self::all_enabled()
    }
}

impl FederationConfig {
    pub fn all_enabled() -> Self {
        Self {
            schema_version: FEDERATION_CONFIG_SCHEMA_VERSION,
            providers: ProviderId::ALL.into_iter().map(ProviderConfig::enabled).collect(),
        }
    }

    pub fn new(providers: Vec<ProviderConfig>) -> Result<Self, ConfigError> {
        let config = Self {
            schema_version: FEDERATION_CONFIG_SCHEMA_VERSION,
            providers,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != FEDERATION_CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::SchemaVersion(self.schema_version));
        }
        if self.providers.len() != ProviderId::ALL.len() {
            return Err(ConfigError::Incomplete);
        }
        for (position, provider) in self.providers.iter().enumerate() {
            if self.providers[..position].iter().any(|prior| prior.id == provider.id) {
                return Err(ConfigError::DuplicateProvider(provider.id));
            }
        }
        if ProviderId::ALL.iter().any(|id| !self.providers.iter().any(|entry| entry.id == *id)) {
            return Err(ConfigError::Incomplete);
        }
        Ok(())
    }

    pub fn is_enabled(&self, provider: ProviderId) -> bool {
        self.providers
            .iter()
            .find(|entry| entry.id == provider)
            .map(|entry| entry.enabled)
            .unwrap_or(false)
    }

    pub fn expected_providers(&self) -> impl Iterator<Item = ProviderId> + '_ {
        ProviderId::ALL.into_iter()
    }

    pub fn disabled_omission(&self, provider: ProviderId) -> Option<ProviderOmissionV1> {
        (!self.is_enabled(provider)).then_some(ProviderOmissionV1 {
            provider,
            reason: ReasonCode::ProviderUnavailable,
            candidate_id: None,
            detail_id: Some("provider_disabled".to_owned()),
            stage: Some("configuration".to_owned()),
        })
    }
}
