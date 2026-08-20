//! Resident lifecycle authority carried over inherited stdio.

use serde::{Deserialize, Serialize};

pub const RESIDENT_LEASE_SCHEMA_VERSION: u32 = 1;

/// Exact binding minted by Hub for one resident process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResidentLeaseV1 {
    pub schema_version: u32,
    pub instance_id: String,
    pub capability: String,
    pub release_generation: String,
    pub declared_data_root: String,
    pub artifact_digest: String,
    pub fence: u64,
}

/// First frame sent by Hub on resident stdin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResidentHelloV1 {
    pub kind: String,
    pub lifecycle_version: u8,
    pub fence: u64,
    pub installation_id: String,
    pub product_id: String,
    pub instance_id: String,
    pub release_generation: String,
    pub artifact_digest: String,
    pub declared_data_root: String,
    pub capability: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResidentEndpointV1 {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResidentLifecycleFrameV1 {
    pub kind: String,
    pub state: Option<String>,
    pub command: Option<String>,
    pub fence: u64,
    pub endpoint: Option<ResidentEndpointV1>,
    pub capability: Option<String>,
}
