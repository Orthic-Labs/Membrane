//! Bounded, non-searchable session/task scratchpad. Never persisted or promoted.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
const MAX_BYTES: usize = 64 * 1024;
const MAX_ID: usize = 160;
const MAX_ENTRIES: usize = 256;
const TTL_MS: u64 = 86_400_000;
static PAD: OnceLock<Mutex<HashMap<(String, String), (Value, u64)>>> = OnceLock::new();
fn pad() -> &'static Mutex<HashMap<(String, String), (Value, u64)>> {
    PAD.get_or_init(|| Mutex::new(HashMap::new()))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
fn purge(state: &mut HashMap<(String, String), (Value, u64)>, at: u64) {
    state.retain(|_, (_, expiry)| *expiry > at);
}
fn err(code: &str, message: &str) -> Value {
    json!({"schemaVersion":1,"operation":"membrane_scratchpad","errorVersion":1,"result":{"kind":"error","code":code,"message":message,"retryable":false}})
}
/// Execute save/load/clear with exact session+task binding.
pub fn handle(request: &Value) -> Value {
    let op = request
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !matches!(op, "save" | "load" | "clear") {
        return err(
            "scratchpad_operation_invalid",
            "operation must be one of save, load, clear",
        );
    }
    let session = request
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let task = request
        .get("taskId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if session.is_empty() || task.is_empty() || session.len() > MAX_ID || task.len() > MAX_ID {
        return err(
            "scratchpad_scope_required",
            "sessionId and taskId are required",
        );
    }
    let key = (session.to_owned(), task.to_owned());
    let mut state = match pad().lock() {
        Ok(state) => state,
        Err(_) => return err("scratchpad_unavailable", "scratchpad state unavailable"),
    };
    purge(&mut state, now());
    let data = match op {
        "save" => {
            let Some(value) = request.get("scratchpad").cloned() else {
                return err(
                    "scratchpad_envelope_invalid",
                    "scratchpad payload is required",
                );
            };
            if serde_json::to_vec(&value)
                .map(|v| v.len())
                .unwrap_or(usize::MAX)
                > MAX_BYTES
            {
                return err(
                    "scratchpad_payload_too_large",
                    "scratchpad exceeds bounded payload",
                );
            }
            if !state.contains_key(&key) && state.len() >= MAX_ENTRIES {
                if let Some(oldest) = state.keys().min().cloned() {
                    state.remove(&oldest);
                }
            }
            state.insert(key, (value.clone(), now().saturating_add(TTL_MS)));
            json!({"status":"saved_ephemeral","operation":"save","scratchpad":value})
        }
        "load" => {
            json!({"status":"loaded","operation":"load","scratchpad":state.get(&key).map(|(value, _)| value.clone())})
        }
        _ => {
            let cleared = state.remove(&key).is_some() as usize;
            json!({"status":"cleared","operation":"clear","cleared":cleared})
        }
    };
    json!({"schemaVersion":1,"operation":"membrane_scratchpad","errorVersion":1,"result":{"kind":"success","data":data}})
}
/// Drop all state for one authoritative session teardown.
pub fn clear_session(session: &str) -> Result<usize, String> {
    if session.trim().is_empty() || session.len() > MAX_ID {
        return Err("scratchpad_scope_required".into());
    }
    let mut state = pad()
        .lock()
        .map_err(|_| "scratchpad_unavailable".to_string())?;
    let before = state.len();
    state.retain(|(s, _), _| s != session);
    Ok(before - state.len())
}
pub fn hub_summary(session: &str) -> Result<Value, String> {
    if session.trim().is_empty() || session.len() > MAX_ID {
        return Err("scratchpad_scope_required".into());
    }
    let mut state = pad()
        .lock()
        .map_err(|_| "scratchpad_unavailable".to_string())?;
    purge(&mut state, now());
    Ok(json!({"sessionId":session,"entries":state.keys().filter(|(s, _)| s == session).count()}))
}
pub fn close_response(session: &str) -> (u16, Value) {
    match clear_session(session) {
        Ok(cleared) => (
            200,
            json!({"status":"cleared","sessionId":session,"cleared":cleared}),
        ),
        Err(code) => (400, err(&code, "invalid session scope")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_load_clear_is_ephemeral_and_scoped() {
        clear_session("s").unwrap();
        let saved =
            handle(&json!({"operation":"save","sessionId":"s","taskId":"t","scratchpad":{"x":1}}));
        assert_eq!(saved["result"]["data"]["status"], "saved_ephemeral");
        assert_eq!(
            handle(&json!({"operation":"load","sessionId":"s","taskId":"t"}))["result"]["data"]
                ["scratchpad"]["x"],
            1
        );
        assert!(
            handle(&json!({"operation":"load","sessionId":"other","taskId":"t"}))["result"]["data"]
                ["scratchpad"]
                .is_null()
        );
        assert_eq!(
            handle(&json!({"operation":"clear","sessionId":"s","taskId":"t"}))["result"]["data"]
                ["cleared"],
            1
        );
        clear_session("s").unwrap();
    }
    #[test]
    fn rejects_scope_operation_and_size() {
        assert_eq!(
            handle(&json!({"operation":"save","taskId":"t"}))["result"]["code"],
            "scratchpad_scope_required"
        );
        assert_eq!(
            handle(&json!({"operation":"archive","sessionId":"s","taskId":"t"}))["result"]["code"],
            "scratchpad_operation_invalid"
        );
        assert_eq!(
            handle(
                &json!({"operation":"save","sessionId":"s","taskId":"t","scratchpad":"x".repeat(MAX_BYTES+1)})
            )["result"]["code"],
            "scratchpad_payload_too_large"
        );
        assert_eq!(
            handle(
                &json!({"operation":"save","sessionId":"x".repeat(MAX_ID+1),"taskId":"t","scratchpad":{}})
            )["result"]["code"],
            "scratchpad_scope_required"
        );
        assert_eq!(
            handle(&json!({"operation":"save","sessionId":"s","taskId":"t"}))["result"]["code"],
            "scratchpad_envelope_invalid"
        );
    }
    #[test]
    fn purge_and_cap_are_deterministic() {
        let mut m = HashMap::new();
        m.insert(("s".into(), "t".into()), (json!({}), 1));
        purge(&mut m, 2);
        assert!(m.is_empty());
        clear_session("cap").unwrap();
        for i in 0..=MAX_ENTRIES {
            handle(
                &json!({"operation":"save","sessionId":"cap","taskId":format!("{i}"),"scratchpad":{}}),
            );
        }
        assert_eq!(hub_summary("cap").unwrap()["entries"], MAX_ENTRIES);
        clear_session("cap").unwrap();
    }
    #[test]
    fn close_and_hub_validation_are_typed() {
        let (status, body) = close_response("");
        assert_eq!(status, 400);
        assert_eq!(body["result"]["code"], "scratchpad_scope_required");
        assert!(hub_summary("").is_err());
        let (status, body) = close_response("ok");
        assert_eq!(status, 200);
        assert_eq!(body["cleared"], 0);
        assert_eq!(hub_summary("ok").unwrap()["entries"], 0);
    }
}
