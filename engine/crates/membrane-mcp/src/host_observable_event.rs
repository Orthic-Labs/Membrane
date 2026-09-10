//! Port of legacy `mcp/host/observable-event.cjs`.
//!
//! This is a distinct, older host-side schema (`membrane.observable-event.v1`
//! with snake_case wire fields) from the newer cross-agent
//! `membrane_protocol::observable_event::ObservableEventV1` envelope
//! (`kind`-enum based, camelCase). The two are NOT drop-in equivalents —
//! different field sets and a different closed enum — so this is ported as a
//! new, self-contained module rather than a parity test over the protocol
//! crate's type.

use membrane_protocol::digest_str;
use serde_json::{json, Value};

pub const EVENT_TYPES: &[&str] = &[
    "user", "assistant", "model", "packet_delivered", "tool_receipt", "tool_receipt_failed",
    "git", "file", "test", "error", "retry", "gate", "delegation", "cost", "correction",
];
const ORIGINS: &[&str] = &["host", "user", "assistant", "tool", "repository", "service"];

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}
fn is_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    })
}

pub struct BuildObservableEvent<'a> {
    pub installation_id: &'a str,
    pub client_id: &'a str,
    pub session_id: &'a str,
    pub task_id: &'a str,
    pub turn_id: &'a str,
    pub trace_id: &'a str,
    pub event_type: &'a str,
    pub origin: &'a str,
    pub content: &'a str,
    pub completeness: Vec<(&'a str, bool)>,
    pub policy_digest: &'a str,
    pub timestamp: String,
    pub char_count: Option<i64>,
}

impl<'a> Default for BuildObservableEvent<'a> {
    fn default() -> Self {
        Self {
            installation_id: "",
            client_id: "",
            session_id: "",
            task_id: "",
            turn_id: "",
            trace_id: "",
            event_type: "",
            origin: "",
            content: "",
            completeness: vec![],
            policy_digest: "membrane-policy-v1",
            timestamp: String::new(),
            char_count: None,
        }
    }
}

/// Mirrors `buildObservableEvent(...)`. Returns `Err(message)` in place of
/// the JS `throw`.
pub fn build_observable_event(mut params: BuildObservableEvent<'_>) -> Result<Value, String> {
    if !EVENT_TYPES.contains(&params.event_type) {
        return Err(format!(
            "unsupported observable event type: {}",
            params.event_type
        ));
    }
    if !ORIGINS.contains(&params.origin) {
        return Err(format!(
            "unsupported observable event origin: {}",
            params.origin
        ));
    }
    let identifiers = [
        params.installation_id,
        params.client_id,
        params.session_id,
        params.task_id,
        params.turn_id,
        params.trace_id,
    ];
    if identifiers.iter().any(|value| !is_identifier(value)) {
        return Err("observable event identifiers must be opaque identifiers".to_string());
    }
    let lineage = format!("{}:{}", identifiers.join(":"), params.event_type);
    let event_id = format!(
        "observable-{}",
        &digest_str(&lineage).strip_prefix("sha256:").unwrap()[..24]
    );
    if params.timestamp.is_empty() {
        params.timestamp = "1970-01-01T00:00:00.000Z".to_string();
    }
    let completeness: Value = params
        .completeness
        .into_iter()
        .map(|(k, v)| (k.to_string(), json!(v)))
        .collect::<serde_json::Map<_, _>>()
        .into();

    let mut event = json!({
        "schema": "membrane.observable-event.v1",
        "installation_id": params.installation_id,
        "client_id": params.client_id,
        "session_id": params.session_id,
        "task_id": params.task_id,
        "turn_id": params.turn_id,
        "trace_id": params.trace_id,
        "event_id": event_id,
        "event_type": params.event_type,
        "origin": params.origin,
        "content_ref_or_digest": digest_str(params.content),
        "timestamp": params.timestamp,
        "completeness": completeness,
        "policy_snapshot_digest": digest_str(params.policy_digest),
    });
    if let Some(char_count) = params.char_count {
        event["char_count"] = json!(char_count);
    }
    validate_observable_event(&event)?;
    Ok(event)
}

/// Mirrors `validateObservableEvent(event)`.
pub fn validate_observable_event(event: &Value) -> Result<(), String> {
    let Value::Object(map) = event else {
        return Err("observable event must be an object".to_string());
    };
    const ALLOWED: &[&str] = &[
        "schema", "installation_id", "client_id", "session_id", "task_id", "turn_id", "trace_id",
        "event_id", "event_type", "origin", "content_ref_or_digest", "timestamp", "completeness",
        "policy_snapshot_digest", "char_count",
    ];
    for key in map.keys() {
        if !ALLOWED.contains(&key.as_str()) {
            return Err(format!("observable event contains forbidden field: {key}"));
        }
    }
    if event.get("schema").and_then(Value::as_str) != Some("membrane.observable-event.v1") {
        return Err("unsupported observable event schema".to_string());
    }
    for key in [
        "installation_id", "client_id", "session_id", "task_id", "turn_id", "trace_id", "event_id",
    ] {
        let value = event.get(key).and_then(Value::as_str);
        if !value.map(is_identifier).unwrap_or(false) {
            return Err(format!("invalid observable event {key}"));
        }
    }
    let event_type = event.get("event_type").and_then(Value::as_str).unwrap_or("");
    let origin = event.get("origin").and_then(Value::as_str).unwrap_or("");
    if !EVENT_TYPES.contains(&event_type) || !ORIGINS.contains(&origin) {
        return Err("unsupported observable event type or origin".to_string());
    }
    let has_char_count = map.contains_key("char_count");
    if event_type != "packet_delivered" && has_char_count {
        return Err("char_count is only valid for packet_delivered".to_string());
    }
    if event_type == "packet_delivered" && has_char_count && !event["char_count"].is_null() {
        let ok = event["char_count"]
            .as_i64()
            .is_some_and(|n| (0..=30000).contains(&n));
        if !ok {
            return Err("char_count must be an integer between 0 and 30000".to_string());
        }
    }
    let content_digest = event.get("content_ref_or_digest").and_then(Value::as_str).unwrap_or("");
    let policy_digest = event.get("policy_snapshot_digest").and_then(Value::as_str).unwrap_or("");
    if !is_digest(content_digest) || !is_digest(policy_digest) {
        return Err("observable event requires digests, not content".to_string());
    }
    let completeness_ok = event
        .get("completeness")
        .and_then(Value::as_object)
        .is_some_and(|obj| obj.values().all(Value::is_boolean));
    if !completeness_ok {
        return Err("observable event completeness must contain only booleans".to_string());
    }
    if event.get("timestamp").and_then(Value::as_str).is_none() {
        return Err("invalid observable event timestamp".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_every_required_event_class_and_preserves_lineage() {
        let required = [
            "user", "assistant", "model", "packet_delivered", "tool_receipt", "git", "file",
            "test", "error", "retry", "gate", "delegation", "cost", "correction",
        ];
        for r in required {
            assert!(EVENT_TYPES.contains(&r), "missing event type {r}");
        }
        let event = build_observable_event(BuildObservableEvent {
            installation_id: "i",
            client_id: "claude_code",
            session_id: "s",
            task_id: "t",
            turn_id: "u",
            trace_id: "x",
            event_type: "correction",
            origin: "user",
            content: "private",
            completeness: vec![("receipt", true)],
            timestamp: "2026-08-01T00:00:00.000Z".to_string(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(event["schema"], "membrane.observable-event.v1");
        assert!(event["event_id"].as_str().unwrap().starts_with("observable-"));
        assert!(event["content_ref_or_digest"].as_str().unwrap().starts_with("sha256:"));
        assert!(!serde_json::to_string(&event).unwrap().contains("private"));

        let err = build_observable_event(BuildObservableEvent {
            installation_id: "i",
            client_id: "c",
            session_id: "s",
            task_id: "t",
            turn_id: "u",
            trace_id: "x",
            event_type: "unknown",
            origin: "host",
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.contains("unsupported observable event type"));
    }

    #[test]
    fn char_count_is_bounded_and_packet_only() {
        let event = build_observable_event(BuildObservableEvent {
            installation_id: "installation-1",
            client_id: "codex",
            session_id: "session-1",
            task_id: "task-1",
            turn_id: "turn-1",
            trace_id: "trace-1",
            event_type: "packet_delivered",
            origin: "host",
            content: "private content",
            char_count: Some(42),
            timestamp: "2026-08-01T00:00:00.000Z".to_string(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(event["char_count"], 42);
        let mut too_big = event.clone();
        too_big["char_count"] = json!(30001);
        assert!(validate_observable_event(&too_big).unwrap_err().contains("char_count"));
        let mut wrong_type = event.clone();
        wrong_type["event_type"] = json!("tool_receipt");
        assert!(validate_observable_event(&wrong_type)
            .unwrap_err()
            .contains("packet_delivered"));
    }
}
