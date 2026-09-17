//! Compatibility handshake required before a service can become canonical.

use crate::error::ClientError;
use serde_json::{Map, Value};

pub const HANDSHAKE_OPERATION: &str = "/health";
pub const CLIENT_PROTOCOL_VERSION: u32 = 1;
pub const CLIENT_SCHEMA_VERSION: u32 = 1;

/// MEM-054 declared per-subsystem compatibility surface from
/// `serviceCapabilities` in the /health payload. Every field is optional at
/// the transport layer; `CompatibilityRequirement` decides which absences are
/// typed incompatibilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceCapabilities {
    pub ledger_index_identity: Option<String>,
    pub ledger_projection_schema: Option<String>,
    pub ledger_fts_schema: Option<String>,
    pub blueprint_provider: Option<String>,
    pub blueprint_provider_version: Option<String>,
    pub blueprint_graph_schema: Option<u64>,
    pub blueprint_ready: Option<bool>,
    pub adapt_contract_versions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceIdentity {
    pub service_id: String,
    pub installation_id: String,
    pub cortex_store_id: String,
    pub release_generation: String,
    pub startup_generation: u64,
    pub runtime_origin: String,
    pub stable_install_root: Option<String>,
    pub protocol_version: u32,
    pub schema_version: u32,
    pub native_only: bool,
    pub subsystems: Vec<String>,
    pub capabilities: Vec<String>,
    pub service_capabilities: Option<ServiceCapabilities>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityRequirement {
    pub protocol_version: u32,
    pub schema_version: u32,
    pub release_generation: Option<String>,
    pub installation_id: Option<String>,
    pub cortex_store_id: Option<String>,
    pub required_runtime_origin: Option<String>,
    pub require_native_only: bool,
    pub required_subsystems: Vec<String>,
    pub required_capabilities: Vec<String>,
    /// MEM-054: required `serviceCapabilities.ledgerIndex.projectionSchema`.
    pub required_ledger_projection_schema: Option<String>,
    /// MEM-054: required `serviceCapabilities.blueprint.provider`.
    pub required_blueprint_provider: Option<String>,
    /// MEM-054: required subset of
    /// `serviceCapabilities.adaptContractVersions`.
    pub required_adapt_contracts: Vec<String>,
}

impl Default for CompatibilityRequirement {
    fn default() -> Self {
        Self {
            protocol_version: CLIENT_PROTOCOL_VERSION,
            schema_version: CLIENT_SCHEMA_VERSION,
            release_generation: None,
            installation_id: None,
            cortex_store_id: None,
            required_runtime_origin: Some("installed".into()),
            require_native_only: true,
            required_subsystems: ["pull", "cortex", "blueprint", "ledger", "adapt"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            // MEM-054: the public `push` durable-memory write is a declared
            // capability; an engine that cannot accept it fails the handshake
            // with typed incompatibility, never an alternate-runtime probe.
            required_capabilities: vec!["push".to_owned()],
            required_ledger_projection_schema: Some("ledger.projection.v5".to_owned()),
            required_blueprint_provider: Some("native-rust".to_owned()),
            // The CodeRight-facing Adapt seam contracts (source consts:
            // membrane-runtime adapt.rs ADAPT_PROPOSAL_SERVICE_CONTRACT,
            // membrane-adapt learner.rs ADAPT_LEARNER_CONTRACT,
            // detector_contract.rs INSIGHTS_* contracts). Duplicated as
            // literals so this crate keeps no dependency on either crate.
            required_adapt_contracts: [
                "adapt.proposal-service.v1",
                "adapt.learner-proposal.v1",
                "adapt.insights-detector-catalog.v1",
                "adapt.transcript-event.v1",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
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
    let startup_generation = object
        .get("startupGeneration")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| incompatible("startupGeneration must be a non-zero unsigned integer"))?;
    let runtime_origin = required_string(object, "runtimeOrigin", "runtime origin")?;
    let stable_install_root = object
        .get("stableInstallRoot")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    let protocol_version = required_u32(object, "protocolVersion")?;
    let schema_version = required_u32(object, "schemaVersion")?;
    let native_only = object
        .get("nativeOnly")
        .and_then(Value::as_bool)
        .ok_or_else(|| incompatible("nativeOnly must be a boolean"))?;
    let subsystems = required_string_array(object, "subsystems")?;
    let capabilities = required_string_array(object, "capabilities")?;
    let service_capabilities = match object.get("serviceCapabilities") {
        None | Some(Value::Null) => None,
        Some(caps) => {
            let caps = caps.as_object().ok_or_else(|| {
                incompatible("serviceCapabilities must be an object")
            })?;
            let optional_string = |parent: &Map<String, Value>, field: &str| {
                parent
                    .get(field)
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned)
            };
            let ledger_index = caps
                .get("ledgerIndex")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let blueprint = caps
                .get("blueprint")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let adapt_contract_versions = caps
                .get("adaptContractVersions")
                .and_then(Value::as_array)
                .map(|versions| {
                    versions
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            Some(ServiceCapabilities {
                ledger_index_identity: optional_string(&ledger_index, "identity"),
                ledger_projection_schema: optional_string(&ledger_index, "projectionSchema"),
                ledger_fts_schema: optional_string(&ledger_index, "ftsSchema"),
                blueprint_provider: optional_string(&blueprint, "provider"),
                blueprint_provider_version: optional_string(&blueprint, "providerVersion"),
                blueprint_graph_schema: blueprint
                    .get("graphSchema")
                    .and_then(Value::as_u64),
                blueprint_ready: blueprint.get("ready").and_then(Value::as_bool),
                adapt_contract_versions,
            })
        }
    };

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
    if let Some(expected) = requirement.required_runtime_origin.as_deref() {
        if runtime_origin != expected {
            return Err(incompatible("runtime origin does not match"));
        }
    }
    if runtime_origin == "installed" && stable_install_root.is_none() {
        return Err(incompatible("installed runtime omitted stableInstallRoot"));
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
    // MEM-054: the declared index/Blueprint/Adapt seam fields bind with the
    // same typed incompatibility as every other handshake element.
    if let Some(expected) = requirement.required_ledger_projection_schema.as_deref() {
        let actual = service_capabilities
            .as_ref()
            .and_then(|caps| caps.ledger_projection_schema.as_deref());
        if actual != Some(expected) {
            return Err(incompatible(
                "Ledger index projection schema is missing or does not match",
            ));
        }
    }
    if let Some(expected) = requirement.required_blueprint_provider.as_deref() {
        let actual = service_capabilities
            .as_ref()
            .and_then(|caps| caps.blueprint_provider.as_deref());
        if actual != Some(expected) {
            return Err(incompatible(
                "Blueprint provider identity is missing or does not match",
            ));
        }
    }
    if let Some(missing) = requirement.required_adapt_contracts.iter().find(|contract| {
        !service_capabilities
            .as_ref()
            .is_some_and(|caps| caps.adapt_contract_versions.iter().any(|found| found == *contract))
    }) {
        return Err(incompatible(format!(
            "required Adapt contract {missing} is unavailable"
        )));
    }
    Ok(ServiceIdentity {
        service_id,
        installation_id,
        cortex_store_id,
        release_generation,
        startup_generation,
        runtime_origin,
        stable_install_root,
        protocol_version,
        schema_version,
        native_only,
        subsystems,
        capabilities,
        service_capabilities,
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
