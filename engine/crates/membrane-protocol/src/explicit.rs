//! Bounded installed-owner transport. This identity makes no resident-liveness claim.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const EXPLICIT_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplicitOwnerBindingV1 {
    pub schema_version: u32,
    pub mode: ExplicitOwnerMode,
    pub installation_id: String,
    pub cortex_store_id: String,
    pub release_generation: String,
    /// Last installed startup epoch; does not imply a running service.
    pub startup_generation: u64,
    pub stable_install_root: String,
    pub protocol_version: u32,
    pub native_only: bool,
    pub subsystems: Vec<String>,
    pub capabilities: Vec<String>,
    pub embedder_dim: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplicitOwnerMode { BoundedExplicit }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplicitOperation {
    Binding, Activity, ActivityRead, Delete, Federate, Get, List, Metrics, Put, Recall,
    Remember, RememberConsolidated, Scopes, Search, Use, Diagnostic,
}

impl ExplicitOperation {
    /// Reads that record use/accounting are effects too. Never replay an unknown dispatch.
    pub fn may_mutate(self) -> bool {
        !matches!(self, Self::Binding | Self::ActivityRead | Self::List | Self::Metrics | Self::Scopes)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplicitRequestV1 {
    pub schema_version: u32,
    pub operation: ExplicitOperation,
    pub expected_binding: Option<ExplicitOwnerBindingV1>,
    pub request: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplicitResponseV1 {
    pub schema_version: u32,
    pub binding: ExplicitOwnerBindingV1,
    pub status: u16,
    pub data: Value,
}
