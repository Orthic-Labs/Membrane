//! Compatibility handshake required before a service can become canonical.

use crate::error::ClientError;
use serde_json::{Map, Value};

pub const HANDSHAKE_OPERATION: &str = "/health";
pub const CLIENT_PROTOCOL_VERSION: u32 = 1;
pub const CLIENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceIdentity {
    pub service_id: String,
    pub installation_id: String,
    pub cortex_store_id: String,
    pub release_generation: String,
    pub protocol_version: u32,
    pub schema_version: u32,
    pub native_only: bool,
    pub subsystems: Vec<String>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityRequirement {
    pub protocol_version: u32,
    pub schema_version: u32,
    pub release_generation: Option<String>,
    pub installation_id: Option<String>,
    pub cortex_store_id: Option<String>,
    pub require_native_only: bool,
    pub required_subsystems: Vec<String>,
    pub required_capabilities: Vec<String>,
}

impl Default for CompatibilityRequirement {
    fn default() -> Self {
        Self {
            protocol_version: CLIENT_PROTOCOL_VERSION,
            schema_version: CLIENT_SCHEMA_VERSION,
            release_generation: None,
            installation_id: None,
            cortex_store_id: None,
            require_native_only: true,
            required_subsystems: ["pull", "push", "cortex", "blueprint", "ledger", "adapt"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            required_capabilities: Vec::new(),
        }
    }
}

pub fn verify(
    value: &Value,
    requirement: &CompatibilityRequirement,
) -> Result<ServiceIdentity, ClientError> {
    let object = value
        .as_object()
        .ok_or_else(|| incompatible("health response is not an object"))?;

    // Legacy health payloads used snake_case names and omitted compatibility
    // fields.  Accepting either shape would let an embedded/old service become
    // canonical, so the native Hub wire shape is deliberately exact.
    for legacy in [
        "service_id",
        "installation_id",
        "cortex_store_id",
        "release_generation",
        "protocol_version",
        "schema_version",
        "native_only",
    ] {
        if object.contains_key(legacy) {
            return Err(incompatible(format!(
                "legacy handshake field {legacy} is unsupported"
            )));
        }
    }

    let service_id = required_string(object, "serviceId", "service identity")?;
    let installation_id = required_string(object, "installationId", "Hub installation identity")?;
    let cortex_store_id = required_string(object, "cortexStoreId", "Cortex store identity")?;
    let release_generation = required_string(object, "releaseGeneration", "release generation")?;
    let protocol_version = required_u32(object, "protocolVersion")?;
    let schema_version = required_u32(object, "schemaVersion")?;
    let native_only = object
        .get("nativeOnly")
        .and_then(Value::as_bool)
        .ok_or_else(|| incompatible("nativeOnly must be a boolean"))?;
    let subsystems = required_string_array(object, "subsystems")?;
    let capabilities = required_string_array(object, "capabilities")?;

    if protocol_version != requirement.protocol_version
        || schema_version != requirement.schema_version
    {
        return Err(incompatible(format!(
            "protocol/schema {protocol_version}/{schema_version} does not match {}/{}",
            requirement.protocol_version, requirement.schema_version
        )));
    }
    if let Some(expected) = requirement.release_generation.as_deref() {
        if release_generation != expected {
            return Err(incompatible("release generation does not match"));
        }
    }
    if let Some(expected) = requirement.installation_id.as_deref() {
        if installation_id != expected {
            return Err(incompatible("Hub installation identity does not match"));
        }
    }
    if let Some(expected) = requirement.cortex_store_id.as_deref() {
        if cortex_store_id != expected {
            return Err(incompatible("Cortex store identity does not match"));
        }
    }
    if requirement.require_native_only && !native_only {
        return Err(incompatible("Hub is not native-only"));
    }
    if let Some(missing) = requirement
        .required_subsystems
        .iter()
        .find(|name| !subsystems.iter().any(|found| found == *name))
    {
        return Err(incompatible(format!(
            "required subsystem {missing} is unavailable"
        )));
    }
    if let Some(missing) = requirement
        .required_capabilities
        .iter()
        .find(|cap| !capabilities.iter().any(|found| found == *cap))
    {
        return Err(incompatible(format!(
            "required capability {missing} is unavailable"
        )));
    }
    Ok(ServiceIdentity {
        service_id,
        installation_id,
        cortex_store_id,
        release_generation,
        protocol_version,
        schema_version,
        native_only,
        subsystems,
        capabilities,
    })
}

fn incompatible(message: impl Into<String>) -> ClientError {
    ClientError::Incompatible {
        message: message.into(),
    }
}

fn required_string(
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<String, ClientError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| incompatible(format!("{label} is missing or invalid")))
}

fn required_u32(object: &Map<String, Value>, field: &str) -> Result<u32, ClientError> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| incompatible(format!("{field} must be an unsigned 32-bit integer")))
}

fn required_string_array(
    object: &Map<String, Value>,
    field: &str,
) -> Result<Vec<String>, ClientError> {
    let values = object
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| incompatible(format!("{field} must be an array of strings")))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|item| !item.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| incompatible(format!("{field} must be an array of strings")))
        })
        .collect()
}

pub(crate) fn request() -> Map<String, Value> {
    Map::new()
}
