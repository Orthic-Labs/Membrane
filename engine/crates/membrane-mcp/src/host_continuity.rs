//! Port of legacy `mcp/host/continuity.mjs`.
//!
//! Semantic continuity is Membrane-owned; raw transcript bytes remain with
//! Claude/Codex. `service` is the current authenticated Membrane operation
//! seam, so no subprocess or local fallback is permitted.

use membrane_protocol::digest_str;
use serde_json::{json, Value};

const SCHEMA: &str = "membrane.continuity-result.v1";

pub struct ContinuityResult {
    pub schema: &'static str,
    pub state: &'static str,
    pub operation: String,
    pub reason: String,
    pub checkpoint: Option<Value>,
    pub verified: bool,
}

fn unavailable(operation: &str, reason: &str) -> ContinuityResult {
    ContinuityResult {
        schema: SCHEMA,
        state: "degraded",
        operation: operation.to_string(),
        reason: reason.to_string(),
        checkpoint: None,
        verified: false,
    }
}

fn id_for(value: &Value) -> String {
    let digest = digest_str(&serde_json::to_string(value).unwrap());
    let hex = digest.strip_prefix("sha256:").unwrap_or(&digest);
    format!("checkpoint/{}", &hex[..24.min(hex.len())])
}

/// Mirrors `hostReference(input)`: only a well-formed `{ id, digest?, host? }`
/// transcript reference is accepted; anything else degrades the caller.
pub struct HostReference {
    pub id: String,
    pub digest: Option<String>,
    pub host: Option<String>,
}

pub fn host_reference(input: &Value) -> Option<HostReference> {
    let reference = input
        .get("transcriptRef")
        .or_else(|| input.get("transcriptReference"))?;
    let id = reference.get("id")?.as_str()?;
    if id.is_empty() {
        return None;
    }
    Some(HostReference {
        id: id.to_string(),
        digest: reference.get("digest").and_then(Value::as_str).map(str::to_string),
        host: reference.get("host").and_then(Value::as_str).map(str::to_string),
    })
}

/// The Membrane operation service seam: `service(operation, payload) -> { ok, checkpoint?, reason? }`.
pub trait ContinuityService {
    fn call(&self, operation: &str, payload: Value) -> Option<Value>;
}

/// A continuity client bound to a service seam (or none, for typed degradation).
pub struct ContinuityClient<'a, S: ContinuityService> {
    service: Option<&'a S>,
    now: Box<dyn Fn() -> String + 'a>,
}

impl<'a, S: ContinuityService> ContinuityClient<'a, S> {
    pub fn new(service: Option<&'a S>, now: impl Fn() -> String + 'a) -> Self {
        Self { service, now: Box::new(now) }
    }

    fn call(&self, operation: &str, payload: Value) -> ContinuityResult {
        let Some(service) = self.service else {
            return unavailable(operation, "membrane_service_unavailable");
        };
        let Some(result) = service.call(operation, payload) else {
            return unavailable(operation, "membrane_service_unavailable");
        };
        let ok = result.get("ok").and_then(Value::as_bool).unwrap_or(false);
        let checkpoint = result.get("checkpoint").cloned();
        if !ok || checkpoint.is_none() {
            let reason = result
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("continuity_unavailable");
            return unavailable(operation, reason);
        }
        ContinuityResult {
            schema: SCHEMA,
            state: "available",
            operation: operation.to_string(),
            reason: "continuity_persisted".to_string(),
            checkpoint,
            verified: true,
        }
    }

    pub fn checkpoint(&self, input: &Value) -> ContinuityResult {
        let Some(reference) = host_reference(input) else {
            return unavailable("checkpoint_save", "transcript_reference_required");
        };
        let session_id = input.get("sessionId").cloned().unwrap_or(Value::Null);
        let transcript_ref = json!({
            "id": reference.id,
            "digest": reference.digest,
            "host": reference.host,
        });
        let id = input
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| id_for(&json!({"sessionId": session_id, "reference": transcript_ref})));
        let checkpoint = json!({
            "schemaVersion": 1,
            "id": id,
            "sessionId": session_id,
            "taskId": input.get("taskId").cloned().unwrap_or(Value::Null),
            "authority": input.get("authority").cloned().unwrap_or(Value::Null),
            "transcriptRef": transcript_ref,
            "trigger": input.get("trigger").and_then(Value::as_str).unwrap_or("unknown"),
            "createdAt": (self.now)(),
        });
        self.call(
            "membrane_checkpoint_save",
            json!({
                "repository": input.get("repository").cloned().unwrap_or(Value::Null),
                "caller": input.get("caller").cloned().unwrap_or(Value::Null),
                "checkpoint": checkpoint,
            }),
        )
    }

    pub fn restore(&self, input: &Value) -> ContinuityResult {
        let Some(id) = input.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) else {
            return unavailable("checkpoint_load", "checkpoint_id_required");
        };
        self.call(
            "membrane_checkpoint_load",
            json!({
                "repository": input.get("repository").cloned().unwrap_or(Value::Null),
                "caller": input.get("caller").cloned().unwrap_or(Value::Null),
                "id": id,
                "asOfMs": input.get("asOfMs").cloned().unwrap_or(Value::Null),
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct RecordingService {
        calls: RefCell<Vec<(String, Value)>>,
    }
    impl ContinuityService for RecordingService {
        fn call(&self, operation: &str, payload: Value) -> Option<Value> {
            self.calls
                .borrow_mut()
                .push((operation.to_string(), payload.clone()));
            Some(json!({"ok": true, "checkpoint": payload.get("checkpoint").cloned().unwrap_or_else(|| json!({"id": payload["id"], "restored": true}))}))
        }
    }

    #[test]
    fn continuity_sends_host_transcript_references_through_current_service() {
        let service = RecordingService { calls: RefCell::new(vec![]) };
        let client = ContinuityClient::new(Some(&service), || "2026-08-20T00:00:00.000Z".to_string());
        let input = json!({
            "sessionId": "s1",
            "transcriptRef": {"id": "host-log-1", "digest": "sha256:x", "host": "codex"},
            "trigger": "PreCompact",
        });
        let saved = client.checkpoint(&input);
        assert_eq!(saved.state, "available");
        let checkpoint = saved.checkpoint.unwrap();
        assert_eq!(checkpoint["transcriptRef"]["id"], "host-log-1");
        assert!(checkpoint["transcriptRef"].get("bytes").is_none());

        let restored = client.restore(&json!({"id": checkpoint["id"]}));
        assert_eq!(restored.state, "available");
        let calls = service.calls.borrow();
        assert_eq!(calls[0].0, "membrane_checkpoint_save");
        assert_eq!(calls[1].0, "membrane_checkpoint_load");
    }

    #[test]
    fn reports_typed_degradation_instead_of_fallback_when_service_is_absent() {
        let client: ContinuityClient<'_, RecordingService> =
            ContinuityClient::new(None, || "now".to_string());
        let result = client.checkpoint(&json!({"transcriptRef": {"id": "host-log-1"}}));
        assert_eq!(result.state, "degraded");
        assert_eq!(result.reason, "membrane_service_unavailable");
    }
}
