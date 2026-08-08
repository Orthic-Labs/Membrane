use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HUB_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HubStateV1 {
    Available,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HubSectionV1 {
    pub state: HubStateV1,
    pub reason: String,
    pub items: Vec<Value>,
    pub resolver: Option<String>,
    pub source: Option<String>,
    pub evidence: Option<String>,
    pub observed_at_unix_ms: u64,
    pub cache_age_ms: u64,
}

impl HubSectionV1 {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            state: HubStateV1::Unavailable,
            reason: if reason.is_empty() {
                "reason_unavailable".into()
            } else {
                reason
            },
            items: Vec::new(),
            resolver: None,
            source: None,
            evidence: None,
            observed_at_unix_ms: 0,
            cache_age_ms: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HubStreamV1 {
    pub state: HubStateV1,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolver: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HubCapabilitiesV1 {
    pub schema_version: u32,
    pub read_only: bool,
    pub resources: Vec<String>,
    pub operations: Vec<String>,
    pub installation_id: String,
    pub service_id: String,
    pub release_generation: String,
    pub data_root_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<HubStreamV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HubSnapshotV1 {
    pub schema_version: u32,
    pub observed_at_unix_ms: u64,
    pub deliveries: HubSectionV1,
    pub providers: HubSectionV1,
    pub repositories: HubSectionV1,
    pub adapters: HubSectionV1,
    pub devices: HubSectionV1,
    pub memory: HubSectionV1,
    pub sentinel: HubSectionV1,
    pub alerts: HubSectionV1,
}
