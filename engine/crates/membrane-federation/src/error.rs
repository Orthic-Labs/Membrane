//! Stable construction errors for federation-owned immutable state.

use membrane_protocol::ProviderId;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RegistryError {
    #[error("provider registry must contain exactly nine providers")]
    Incomplete,
    #[error("duplicate provider registration: {0}")]
    DuplicateProvider(ProviderId),
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
    #[error("provider registry is invalid: {0}")]
    Invalid(String),
    #[error("provider SDK registry error: {0}")]
    ProviderSdk(String),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ConfigError {
    #[error("unsupported federation configuration schema version: {0}")]
    SchemaVersion(u32),
    #[error("federation configuration must declare exactly nine providers")]
    Incomplete,
    #[error("duplicate provider configuration: {0}")]
    DuplicateProvider(ProviderId),
    #[error("unknown provider configuration: {0}")]
    UnknownProvider(String),
    #[error("federation configuration is invalid: {0}")]
    Invalid(String),
}
