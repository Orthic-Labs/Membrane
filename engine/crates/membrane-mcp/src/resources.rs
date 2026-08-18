//! Canonical Membrane MCP resources.
//!
//! Every resource definition lives in `schemas/registry/resources/*.json` so the
//! native (Rust) MCP server and the legacy JS MCP server serve the exact same
//! payload. This module embeds those JSON files at compile time via
//! `include_str!` and exposes a tiny façade so `resources/list` and
//! `resources/read` JSON-RPC methods can answer from a single canonical
//! registry, with the same access-grant boundary the JS surface enforces.

use serde_json::{json, Value};

const RESOURCES_INDEX: &str =
    include_str!("../../../../schemas/registry/resources/resources-index.v1.json");
const INSTALLATION_MANIFEST: &str =
    include_str!("../../../../schemas/registry/resources/installation-manifest.v1.json");
const LEASE_STATUS: &str = include_str!("../../../../schemas/registry/resources/lease-status.v1.json");
const OPERATION_REGISTRY: &str =
    include_str!("../../../../schemas/registry/resources/operation-registry.v1.json");

/// All canonical resource definitions, embedded at compile time.
const DEFINITION_RAWS: &[&str] = &[
    RESOURCES_INDEX,
    INSTALLATION_MANIFEST,
    LEASE_STATUS,
    OPERATION_REGISTRY,
];

fn definitions() -> Vec<Value> {
    DEFINITION_RAWS
        .iter()
        .map(|raw| serde_json::from_str::<Value>(raw).expect("canonical resource must parse"))
        .collect()
}

/// Names of every canonical resource. Exposed so callers (tests, the discovery
/// surface, and the JSON-RPC dispatch table) can iterate without re-parsing.
pub const NAMES: &[&str] = &[
    "resources-index",
    "installation-manifest",
    "lease-status",
    "operation-registry",
];

/// URIs of every canonical resource, in the same order as [`NAMES`].
pub const URIS: &[&str] = &[
    "membrane://resource/resources-index/v1",
    "membrane://resource/installation-manifest/v1",
    "membrane://resource/lease-status/v1",
    "membrane://resource/operation-registry/v1",
];

fn parse(raw: &str) -> Value {
    serde_json::from_str::<Value>(raw).expect("canonical resource must parse")
}

fn shape_list_entry(resource: &Value) -> Value {
    json!({
        "name": resource["name"],
        "version": resource["version"],
        "uri": resource["uri"],
        "mimeType": resource["mimeType"],
        "description": resource["description"],
        "accessGrants": resource.get("accessGrants").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
        "authorityEscalation": resource.get("authorityEscalation").cloned().unwrap_or(Value::Bool(false)),
        "authorityNotes": resource.get("authorityNotes").cloned().unwrap_or(Value::Null),
    })
}

/// Return the canonical `resources/list` payload. Mirrors the MCP wire shape
/// and embeds Membrane-specific metadata (`accessGrants`,
/// `authorityEscalation`, `authorityNotes`) so a client can prove the
/// resource is bounded before issuing a read.
pub fn list_payload() -> Value {
    let resources: Vec<Value> = definitions().iter().map(shape_list_entry).collect();
    json!({ "resources": resources })
}

fn find_by_uri(uri: &str) -> Option<Value> {
    for raw in DEFINITION_RAWS {
        let value = parse(raw);
        if value["uri"].as_str() == Some(uri) {
            return Some(value);
        }
    }
    None
}

fn find_by_name(name: &str) -> Option<Value> {
    for raw in DEFINITION_RAWS {
        let value = parse(raw);
        if value["name"].as_str() == Some(name) {
            return Some(value);
        }
    }
    None
}

/// Typed outcome of a `resources/read` request. The `Err` branch never
/// serializes the resource body, so a denied request cannot leak content.
#[derive(Debug)]
pub enum ReadOutcome {
    Ok(Value),
    AccessDenied {
        resource: String,
        required: Vec<String>,
        presented: Vec<String>,
    },
    NotFound {
        uri: String,
    },
}

/// Return the canonical `resources/read` payload for a single resource.
///
/// `uri` is the Membrane resource URI (e.g. `membrane://resource/installation-manifest/v1`).
/// `access_grants` is the slice of grant names the caller presents; the read
/// succeeds iff that slice contains at least one entry of the resource's
/// declared `accessGrants`.
pub fn read_payload(uri: &str, access_grants: &[&str]) -> ReadOutcome {
    let resource = match find_by_uri(uri) {
        Some(resource) => resource,
        None => {
            return ReadOutcome::NotFound {
                uri: uri.to_string(),
            }
        }
    };
    let required: Vec<String> = resource
        .get("accessGrants")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let presented: Vec<String> = access_grants.iter().map(|s| s.to_string()).collect();
    let granted = required
        .iter()
        .any(|grant| access_grants.iter().any(|presented| presented == grant));
    if !granted {
        return ReadOutcome::AccessDenied {
            resource: resource["name"].as_str().unwrap_or("").to_string(),
            required,
            presented,
        };
    }
    ReadOutcome::Ok(json!({
        "name": resource["name"],
        "version": resource["version"],
        "uri": resource["uri"],
        "mimeType": resource["mimeType"],
        "description": resource["description"],
        "accessGrants": resource.get("accessGrants").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
        "authorityEscalation": resource.get("authorityEscalation").cloned().unwrap_or(Value::Bool(false)),
        "authorityNotes": resource.get("authorityNotes").cloned().unwrap_or(Value::Null),
        "body": resource.get("body").cloned().unwrap_or(Value::Null),
    }))
}

/// Convenience helper that resolves a resource by its short name (e.g.
/// `installation-manifest`) instead of its full URI. The same access-grant
/// boundary applies.
pub fn read_payload_by_name(name: &str, access_grants: &[&str]) -> ReadOutcome {
    let resource = match find_by_name(name) {
        Some(resource) => resource,
        None => {
            return ReadOutcome::NotFound {
                uri: name.to_string(),
            }
        }
    };
    let uri = resource["uri"].as_str().unwrap_or("").to_string();
    read_payload(&uri, access_grants)
}

/// Serialize a [`ReadOutcome`] into the canonical `resources/read` JSON-RPC
/// result envelope. The body is **never** serialized into the response when
/// the read is denied or the URI is unknown.
pub fn read_result_payload(outcome: ReadOutcome) -> Value {
    match outcome {
        ReadOutcome::Ok(body) => body,
        ReadOutcome::AccessDenied {
            resource,
            required,
            presented,
        } => json!({
            "error": {
                "kind": "error",
                "code": "resource_access_denied",
                "message": format!(
                    "resources/read requires one of the grants in accessGrants: [{}]; caller presented [{}]",
                    required.join(", "),
                    presented.join(", ")
                ),
                "retryable": false,
                "details": {
                    "resource": resource,
                    "requiredGrants": required,
                    "presentedGrants": presented,
                }
            }
        }),
        ReadOutcome::NotFound { uri } => json!({
            "error": {
                "kind": "error",
                "code": "resource_not_found",
                "message": format!("resources/read received an unknown URI: {uri}"),
                "retryable": false,
                "details": {
                    "uri": uri,
                }
            }
        }),
    }
}
