use serde_json::{json, Value};

pub(crate) const PROTOCOL: &str = "2025-03-26";
pub(crate) const SERVER_NAME: &str = "membrane";
pub(crate) const SERVER_VERSION: &str = "1.0.0";

pub fn initialize_response() -> Value {
    json!({"protocolVersion": PROTOCOL, "capabilities": {"tools": {}, "resources": {}, "prompts": {}}, "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION}, "instructions": "Use membrane_context for federated context through /federate. Never expect raw memory CRUD. Membrane workflow prompts are bounded to operations in operations/operations/operations-index.v1.golden.json and grant no authority beyond those operations."})
}

pub fn discovery_response() -> Value {
    json!({"protocolVersion": PROTOCOL, "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION}, "tools": crate::tools::definitions(), "prompts": crate::prompts::list_payload(), "resource": {"uri": "membrane://protocol/v1", "mimeType": "text/markdown"}})
}
