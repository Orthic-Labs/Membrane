//! Compatibility handshake required before a service can become canonical.

use crate::error::ClientError;
use serde_json::{Map, Value};

pub const HANDSHAKE_OPERATION: &str = "/health";
pub const CLIENT_PROTOCOL_VERSION: u32 = 1;
pub const CLIENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceIdentity {
    pub service_id: String,
    pub release_generation: String,
    pub protocol_version: u32,
    pub schema_version: u32,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityRequirement {
    pub protocol_version: u32,
    pub schema_version: u32,
    pub release_generation: Option<String>,
    pub required_capabilities: Vec<String>,
}

impl Default for CompatibilityRequirement {
    fn default() -> Self { Self { protocol_version: CLIENT_PROTOCOL_VERSION, schema_version: CLIENT_SCHEMA_VERSION, release_generation: None, required_capabilities: Vec::new() } }
}

pub fn verify(value: &Value, requirement: &CompatibilityRequirement) -> Result<ServiceIdentity, ClientError> {
    let object = value.as_object().ok_or_else(|| ClientError::Incompatible { message: "health response is not an object".into() })?;
    let service_id = object.get("serviceId").or_else(|| object.get("service_id")).and_then(Value::as_str).filter(|v| !v.is_empty()).ok_or_else(|| ClientError::Incompatible { message: "service identity is missing".into() })?.to_string();
    let release_generation = object.get("releaseGeneration").or_else(|| object.get("release_generation")).and_then(Value::as_str).unwrap_or("").to_string();
    let protocol_version = object.get("protocolVersion").or_else(|| object.get("protocol_version")).and_then(Value::as_u64).unwrap_or(CLIENT_PROTOCOL_VERSION as u64) as u32;
    let schema_version = object.get("schemaVersion").and_then(Value::as_u64).unwrap_or(CLIENT_SCHEMA_VERSION as u64) as u32;
    let capabilities = object.get("capabilities").and_then(Value::as_array).map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
    if protocol_version != requirement.protocol_version || schema_version != requirement.schema_version { return Err(ClientError::Incompatible { message: format!("protocol/schema {protocol_version}/{schema_version} does not match {}/{}", requirement.protocol_version, requirement.schema_version) }); }
    if let Some(expected) = requirement.release_generation.as_deref() { if release_generation != expected { return Err(ClientError::Incompatible { message: "release generation does not match".into() }); } }
    if let Some(missing) = requirement.required_capabilities.iter().find(|cap| !capabilities.iter().any(|found| found == *cap)) { return Err(ClientError::Incompatible { message: format!("required capability {missing} is unavailable") }); }
    Ok(ServiceIdentity { service_id, release_generation, protocol_version, schema_version, capabilities })
}

pub(crate) fn request() -> Map<String, Value> { Map::new() }
