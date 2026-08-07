//! Canonical Membrane MCP prompts.
//!
//! Every prompt definition lives in `operations/prompts/*.json` so the native
//! (Rust) MCP server and the legacy JS MCP server serve the exact same payload.
//! This module embeds those JSON files at compile time via `include_str!` and
//! exposes a tiny façade so `prompts/list` and `prompts/get` JSON-RPC methods
//! can answer from a single canonical registry.

use serde_json::{json, Value};

const RECAP: &str = include_str!("../../../../operations/prompts/recap.v1.json");
const PLAN: &str = include_str!("../../../../operations/prompts/plan.v1.json");
const SUMMARIZE: &str = include_str!("../../../../operations/prompts/summarize.v1.json");
const CHECKPOINT: &str = include_str!("../../../../operations/prompts/checkpoint.v1.json");

pub(crate) fn definitions() -> Value {
    Value::Array(
        [RECAP, PLAN, SUMMARIZE, CHECKPOINT]
            .iter()
            .map(|raw| serde_json::from_str::<Value>(raw).expect("canonical prompt must parse"))
            .collect(),
    )
}

/// Return the canonical `prompts/list` payload. Mirrors the MCP wire shape and
/// embeds Membrane-specific metadata (`operations`, `authorityScope`,
/// `authorityEscalation`, `authorityNotes`) so a client can prove the prompt
/// is bounded before invoking any tool.
pub fn list_payload() -> Value {
    let prompts: Vec<Value> = definitions()
        .as_array()
        .expect("prompt definitions are an array")
        .iter()
        .map(|prompt| {
            json!({
                "name": prompt["name"],
                "description": prompt["description"],
                "arguments": prompt.get("arguments").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
                "operations": prompt.get("operations").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
                "authorityScope": prompt.get("authorityScope").cloned().unwrap_or(Value::Null),
                "authorityEscalation": prompt.get("authorityEscalation").cloned().unwrap_or(Value::Bool(false)),
                "authorityNotes": prompt.get("authorityNotes").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    json!({ "prompts": prompts })
}

/// Return the canonical `prompts/get` payload for a single prompt. The
/// arguments block is included so a client can verify the schema before
/// substituting placeholders; messages are returned verbatim.
pub fn get_payload(name: &str) -> Option<Value> {
    for raw in [RECAP, PLAN, SUMMARIZE, CHECKPOINT] {
        let prompt: Value = serde_json::from_str(raw).expect("canonical prompt must parse");
        if prompt["name"].as_str() == Some(name) {
            return Some(json!({
                "name": prompt["name"],
                "description": prompt["description"],
                "arguments": prompt.get("arguments").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
                "operations": prompt.get("operations").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
                "authorityScope": prompt.get("authorityScope").cloned().unwrap_or(Value::Null),
                "authorityEscalation": prompt.get("authorityEscalation").cloned().unwrap_or(Value::Bool(false)),
                "authorityNotes": prompt.get("authorityNotes").cloned().unwrap_or(Value::Null),
                "messages": prompt.get("messages").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
            }));
        }
    }
    None
}

/// Names of every canonical prompt. Exposed so callers (tests, the discovery
/// surface, and the JSON-RPC dispatch table) can iterate without re-parsing.
pub const NAMES: &[&str] = &["recap", "plan", "summarize", "checkpoint"];
