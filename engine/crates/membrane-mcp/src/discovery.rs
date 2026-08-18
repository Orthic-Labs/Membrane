use serde_json::{json, Value};

pub(crate) const PROTOCOL: &str = "2025-03-26";
pub(crate) const SERVER_NAME: &str = "membrane";
pub(crate) const SERVER_VERSION: &str = "1.0.0";

pub fn initialize_response() -> Value {
    json!({"protocolVersion": PROTOCOL, "capabilities": {"tools": {}, "resources": {}, "prompts": {}}, "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION}, "instructions": "Use membrane_context for federated context through /federate. Never expect raw memory CRUD. Membrane workflow prompts are bounded to operations in schemas/registry/operations/operations-index.v1.golden.json and grant no authority beyond those operations. Membrane MCP resources are bounded by the accessGrants declared in schemas/registry/resources/*.json; reads outside those grants return a typed resource_access_denied rejection with no content leak."})
}

pub fn discovery_response() -> Value {
    let resources = crate::resources::list_payload()["resources"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    json!({"protocolVersion": PROTOCOL, "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION}, "tools": crate::tools::definitions(), "prompts": crate::prompts::list_payload(), "resources": resources})
}
