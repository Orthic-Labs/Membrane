//! Bounded, read-only projection of agent adapter and device evidence.
//! Missing evidence is never treated as liveness or enforcement.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_ADAPTERS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Status {
    pub state: String,
    pub mechanism: String,
    pub evidence: String,
}

impl Status {
    fn unknown() -> Self { Self { state: "unknown".into(), mechanism: "unavailable".into(), evidence: "unknown — no evidence".into() } }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentAdapter {
    pub id: String,
    pub client: String,
    pub device: String,
    #[serde(rename = "declaredCapability")] pub declared_capability: String,
    #[serde(rename = "clientCapability")] pub client_capability: String,
    pub capability: Status,
    pub installed: Status,
    pub active: Status,
    pub delivering: Status,
    pub enforced: Status,
    pub proven: Status,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentAdapterView {
    #[serde(rename = "schemaVersion")] pub schema_version: u32,
    pub adapters: Vec<AgentAdapter>,
}

fn text(root: &Value, key: &str) -> Option<String> { root.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_owned) }
fn status(root: &Value, key: &str) -> Status {
    let Some(value) = root.get(key) else { return Status::unknown() };
    if let Some(state) = value.as_str() { return Status { state: state.into(), mechanism: "declared".into(), evidence: "declared status".into() }; }
    Status { state: text(value, "state").unwrap_or_else(|| "unknown".into()), mechanism: text(value, "mechanism").unwrap_or_else(|| "unavailable".into()), evidence: text(value, "evidence").unwrap_or_else(|| "unknown — no evidence".into()) }
}

fn level(value: &str) -> Option<u8> { value.strip_prefix('L')?.parse().ok() }

/// Project externally assembled adapter evidence. No data-plane reads or writes occur here.
pub fn project(report: &Value) -> AgentAdapterView {
    let data = report.get("result").and_then(|v| v.get("data")).unwrap_or(report);
    let source = data.get("adapters").or_else(|| data.get("agentsAdapters")).and_then(Value::as_array).cloned().unwrap_or_default();
    let adapters = source.into_iter().take(MAX_ADAPTERS).map(|item| {
        let declared = text(&item, "declaredCapability").or_else(|| text(&item, "declared_capability")).unwrap_or_else(|| "unknown".into());
        let client = text(&item, "clientCapability").or_else(|| text(&item, "client_capability")).unwrap_or_else(|| "unknown".into());
        let capability = match level(&client).zip(level(&declared)) {
            Some((actual, wanted)) if actual < wanted => Status { state: "degraded".into(), mechanism: "client capability below declared capability".into(), evidence: format!("client {client} < declared {declared}") },
            Some(_) => Status { state: "available".into(), mechanism: "capability comparison".into(), evidence: format!("client {client} meets declared {declared}") },
            None => Status::unknown(),
        };
        AgentAdapter { id: text(&item, "id").unwrap_or_else(|| "unknown".into()), client: text(&item, "client").unwrap_or_else(|| "unknown".into()), device: text(&item, "device").unwrap_or_else(|| "unknown".into()), declared_capability: declared, client_capability: client, capability, installed: status(&item, "installed"), active: status(&item, "active"), delivering: status(&item, "delivering"), enforced: status(&item, "enforced"), proven: status(&item, "proven") }
    }).collect();
    AgentAdapterView { schema_version: SCHEMA_VERSION, adapters }
}
