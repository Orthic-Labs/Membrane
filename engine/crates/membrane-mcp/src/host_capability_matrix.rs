//! Port of legacy `mcp/host/capability-matrix.mjs`.
//!
//! Host capability is an observation of an integration seam. It never grants
//! authority, selects evidence, or claims host-internal context enforcement.

use membrane_protocol::digest_str;
use serde_json::Value;

pub const HOST_EVENTS: &[&str] = &[
    "UserPromptSubmit",
    "tool_result_egress",
    "delegated_agent_egress",
    "PreCompact",
    "PostCompact",
    "SessionStart",
    "Stop",
];

pub const CAPABILITY_LEVELS: &[&str] = &["native", "projection", "unavailable"];

/// Embedded copy of the legacy fixture `mcp/host/capability-matrix.v1.json`
/// (copied verbatim into this crate; the legacy `mcp/host` tree is read-only
/// reference and is never edited or imported at build time).
const CAPABILITY_MATRIX_FIXTURE: &str =
    include_str!("../fixtures/capability-matrix.v1.json");

/// Returns the capability matrix as a `serde_json::Value`, mirroring the JS
/// `HOST_CAPABILITY_MATRIX` object shape exactly. Loaded from the embedded
/// fixture rather than re-declared inline, so the two cannot drift.
pub fn host_capability_matrix() -> Value {
    let mut value: Value =
        serde_json::from_str(CAPABILITY_MATRIX_FIXTURE).expect("embedded capability matrix fixture parses");
    // The fixture carries a `sourceHash` provenance field the JS in-memory
    // constant does not; strip it so this function's output matches
    // `HOST_CAPABILITY_MATRIX` byte-for-byte.
    if let Value::Object(map) = &mut value {
        map.remove("sourceHash");
    }
    value
}

/// Canonical string form used for digesting: object keys sorted, arrays kept
/// in order — mirrors the JS `canonical()` helper exactly.
fn canonical(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(&map[key])
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        other => serde_json::to_string(other).unwrap(),
    }
}

pub fn capability_matrix_digest(matrix: &Value) -> String {
    digest_str(&canonical(matrix))
}

pub fn capability_for(client: &str, event: &str) -> &'static str {
    let matrix = host_capability_matrix();
    if !HOST_EVENTS.contains(&event) {
        return "unavailable";
    }
    matrix["clients"][client][event]
        .as_str()
        .map(|level| match level {
            "native" => "native",
            "projection" => "projection",
            _ => "unavailable",
        })
        .unwrap_or("unavailable")
}

pub struct CapabilityMatrixValidation {
    pub valid: bool,
    pub failures: Vec<String>,
}

pub fn validate_capability_matrix(matrix: &Value) -> CapabilityMatrixValidation {
    let mut failures = Vec::new();
    if matrix.get("schema").and_then(Value::as_str) != Some("membrane.host-capability-matrix.v1")
    {
        failures.push("schema".to_string());
    }
    let levels: Vec<&str> = matrix["levels"]
        .as_array()
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for client in ["claude_code", "codex"] {
        for event in HOST_EVENTS {
            let value = matrix["clients"][client][event].as_str();
            let ok = value.map(|v| levels.contains(&v)).unwrap_or(false);
            if !ok {
                failures.push(format!("{client}.{event}"));
            }
        }
    }
    CapabilityMatrixValidation {
        valid: failures.is_empty(),
        failures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_valid_and_digest_is_stable() {
        let matrix = host_capability_matrix();
        assert!(validate_capability_matrix(&matrix).valid);
        let digest = capability_matrix_digest(&matrix);
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest, capability_matrix_digest(&host_capability_matrix()));
    }

    #[test]
    fn capability_for_matches_legacy_rows() {
        assert_eq!(
            capability_for("claude_code", "delegated_agent_egress"),
            "projection"
        );
        assert_eq!(capability_for("codex", "delegated_agent_egress"), "unavailable");
        assert_eq!(capability_for("unknown", "UserPromptSubmit"), "unavailable");
    }
}
