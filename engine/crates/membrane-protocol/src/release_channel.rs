//! Read-only release-channel contract. It describes support and update evidence;
//! it never authorizes or performs an update.
use serde::{Deserialize, Serialize};

pub const RELEASE_CHANNEL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseChannel {
    Stable,
    Beta,
    Nightly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportState {
    Supported,
    Degraded,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaCompatibility {
    Compatible,
    MigrationRequired,
    Incompatible,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupportWindowV1 {
    pub state: SupportState,
    pub starts_at: String,
    pub ends_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseChannelV1 {
    pub schema_version: u32,
    pub channel: ReleaseChannel,
    pub release: String,
    pub support: SupportWindowV1,
    pub schema_compatibility: SchemaCompatibility,
    pub migration: String,
    pub rollback: String,
    /// None means signed update evidence is unavailable, never that an update exists.
    pub signed_update_evidence: Option<String>,
}

impl ReleaseChannelV1 {
    pub fn update_available(&self) -> bool {
        self.signed_update_evidence.as_deref().is_some_and(|value| !value.trim().is_empty())
    }
    pub fn is_schema_current(&self) -> bool {
        self.schema_version == RELEASE_CHANNEL_SCHEMA_VERSION
            && self.schema_compatibility == SchemaCompatibility::Compatible
    }
}

impl ReleaseChannel {
    /// The exact wire string this variant serializes to/from (`#[serde(rename_all = "lowercase")]`).
    /// Used by `compatibility_policy` to run the one raw-string fail-closed
    /// parse path (`compatibility_policy::parse_channel`) against both an
    /// untrusted wire value and an already-typed `ReleaseChannel`, so there
    /// is a single enforcement code path rather than two that could drift.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Nightly => "nightly",
        }
    }
}

impl SchemaCompatibility {
    /// The exact wire string this variant serializes to/from (`#[serde(rename_all = "snake_case")]`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::MigrationRequired => "migration_required",
            Self::Incompatible => "incompatible",
            Self::Unknown => "unknown",
        }
    }
}
