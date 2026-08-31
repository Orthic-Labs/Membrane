//! Host adapters: Claude Code, Codex & frozen generic JSONL transcripts.
//!
//! Each adapter takes a single JSONL row (`serde_json::Value`) and returns the
//! normalized [`RawEvent`]s found inside it. All hosts share one shape so
//! downstream projections ingest them identically.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::canonical::py_json_dumps;

/// Frozen already-normalized hosts served by the generic adapter.
pub const GENERIC_HOSTS: &[&str] = &[
    "cline",
    "command_code",
    "opencode",
    "qwen",
    "pi",
    "gemini",
    "grok_build",
    "roo_cline",
    "cursor",
    "copilot",
    "antigravity",
];

static CODEX_CONTROL_ENVELOPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?s)<(?:subagent_notification|codex_internal_context|codex_delegation|turn_aborted|heartbeat)(?:\s[^>]*)?>.*?</(?:subagent_notification|codex_internal_context|codex_delegation|turn_aborted|heartbeat)>",
    )
    .expect("valid Codex control-envelope regex")
});
static CODEX_IMAGE_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"</?image\b[^>]*>?").expect("valid Codex image-marker regex")
});
static ANTIGRAVITY_USER_REQUEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<USER_REQUEST>\s*(.*?)\s*</USER_REQUEST>")
        .expect("valid Antigravity user-request regex")
});

const INJECTED_CONTEXT_PREFIXES: &[&str] = &[
    "# AGENTS.md instructions",
    "# CLAUDE.md instructions",
    "# Context from my IDE setup:",
    "# Files mentioned by the user",
    "<environment_context",
    "<in-app-browser-context",
    "<permissions instructions",
    "<recommended_plugins",
    "<summary>",
    "<command-name",
    "<command-message",
    "<local-command-stdout",
    "<task-notification",
    "<launch-selected-element",
    "[Request interrupted",
];

fn injected_context(text: &str) -> bool {
    let text = text.trim_start();
    INJECTED_CONTEXT_PREFIXES
        .iter()
        .any(|prefix| text.starts_with(prefix))
}

fn codex_user_text(text: String) -> String {
    let text = CODEX_CONTROL_ENVELOPE.replace_all(&text, "");
    CODEX_IMAGE_MARKER.replace_all(&text, "").trim().to_owned()
}

/// A normalized pre-canonicalization event emitted by a host adapter.
#[derive(Debug, Clone, Default)]
pub struct RawEvent {
    pub kind: String,
    pub role: Option<String>,
    pub timestamp: Option<String>,
    pub text: String,
    pub tool: Option<String>,
    pub call_id: Option<String>,
    pub is_error: bool,
    pub synthetic: bool,
    pub meta: bool,
    pub private_reasoning_omitted: bool,
    /// Explicit redaction override; `None` derives from redaction markers.
    pub redacted: Option<bool>,
    pub is_sidechain: Option<bool>,
    // Optional thread/scope passthrough (generic frozen snapshots).
    pub agent_role: Option<String>,
    pub thread_source: Option<String>,
    pub parent_thread_id: Option<String>,
    pub cwd: Option<String>,
    pub repo: Option<String>,
    pub session_id: Option<String>,
}

impl RawEvent {
    fn new(kind: &str) -> Self {
        Self {
            kind: kind.to_string(),
            ..Default::default()
        }
    }
}

fn as_str(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_string)
}

fn value_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(value) => py_json_dumps(value),
    }
}

fn text_values(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Shared raw message shape used by CommandCode, Pi, Qwen, Cline, Gemini,
/// Grok Build, & Roo-Cline.
fn message_events(message: &Value, fallback_timestamp: Option<String>) -> Vec<RawEvent> {
    let role = message.get("role").and_then(Value::as_str).unwrap_or("");
    let timestamp = as_str(message.get("timestamp"))
        .or_else(|| as_str(message.get("ts")))
        .or(fallback_timestamp);
    let content = message.get("content");
    if role == "toolResult" || role == "tool_result" {
        let mut event = RawEvent::new("tool_result");
        event.role = Some("user".into());
        event.timestamp = timestamp;
        event.tool = as_str(message.get("toolName")).or_else(|| as_str(message.get("tool")));
        event.call_id =
            as_str(message.get("toolCallId")).or_else(|| as_str(message.get("call_id")));
        event.text = text_values(content);
        event.is_error = message
            .get("isError")
            .or_else(|| message.get("is_error"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        return vec![event];
    }
    let role = match role {
        "user" | "human" => "user",
        "assistant" | "model" | "gemini" => "assistant",
        _ => return Vec::new(),
    };
    if let Some(text) = content.and_then(Value::as_str) {
        let mut event = RawEvent::new(&format!("{role}_message"));
        event.role = Some(role.into());
        event.timestamp = timestamp;
        event.text = text.into();
        return vec![event];
    }
    let Some(blocks) = content.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut events = Vec::new();
    for block in blocks.iter().filter_map(Value::as_object) {
        match block.get("type").and_then(Value::as_str).unwrap_or("") {
            "text" | "input_text" | "output_text" => {
                let mut event = RawEvent::new(&format!("{role}_message"));
                event.role = Some(role.into());
                event.timestamp = timestamp.clone();
                event.text = block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into();
                events.push(event);
            }
            "thinking" | "reasoning" => {
                let mut event = RawEvent::new("thinking");
                event.role = Some("assistant".into());
                event.timestamp = timestamp.clone();
                event.text = "private reasoning omitted".into();
                event.private_reasoning_omitted = true;
                event.meta = true;
                events.push(event);
            }
            "tool_use" | "toolCall" | "tool_call" => {
                let mut event = RawEvent::new("tool_call");
                event.role = Some("assistant".into());
                event.timestamp = timestamp.clone();
                event.tool = as_str(block.get("name"))
                    .or_else(|| as_str(block.get("toolName")))
                    .or(Some("unknown".into()));
                event.call_id = as_str(block.get("id"))
                    .or_else(|| as_str(block.get("callId")))
                    .or_else(|| as_str(block.get("call_id")));
                event.text = value_text(block.get("input").or_else(|| block.get("arguments")));
                events.push(event);
            }
            "tool_result" | "toolResult" => {
                let mut event = RawEvent::new("tool_result");
                event.role = Some("user".into());
                event.timestamp = timestamp.clone();
                event.tool = as_str(block.get("name")).or_else(|| as_str(block.get("toolName")));
                event.call_id = as_str(block.get("tool_use_id"))
                    .or_else(|| as_str(block.get("toolCallId")))
                    .or_else(|| as_str(block.get("call_id")));
                event.text = value_text(block.get("content").or_else(|| block.get("output")));
                event.is_error = block
                    .get("is_error")
                    .or_else(|| block.get("isError"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                events.push(event);
            }
            _ => {}
        }
    }
    events
}

pub fn command_family_events(obj: &Value) -> Vec<RawEvent> {
    if obj.get("type").and_then(Value::as_str) == Some("message") {
        return obj
            .get("message")
            .map(|message| message_events(message, as_str(obj.get("timestamp"))))
            .unwrap_or_default();
    }
    message_events(obj, as_str(obj.get("timestamp")))
}

pub fn cline_events(obj: &Value) -> Vec<RawEvent> {
    obj.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|message| message_events(message, None))
        .collect()
}

pub fn gemini_events(obj: &Value) -> Vec<RawEvent> {
    if let Some(rows) = obj
        .get("messages")
        .or_else(|| obj.get("history"))
        .and_then(Value::as_array)
    {
        return rows
            .iter()
            .flat_map(|row| message_events(row, as_str(row.get("timestamp"))))
            .collect();
    }
    let mut row = obj.clone();
    if row.get("role").is_none() {
        if let Some(kind) = row.get("type").and_then(Value::as_str).map(str::to_string) {
            if matches!(
                kind.as_str(),
                "user" | "assistant" | "human" | "model" | "gemini"
            ) {
                let role = if kind == "gemini" { "assistant" } else { &kind };
                row.as_object_mut()
                    .unwrap()
                    .insert("role".into(), Value::String(role.into()));
            }
        }
    }
    message_events(&row, as_str(obj.get("timestamp")))
}

pub fn grok_events(obj: &Value) -> Vec<RawEvent> {
    if obj
        .get("synthetic_reason")
        .is_some_and(|value| !value.is_null())
    {
        return Vec::new();
    }
    gemini_events(obj)
}

pub fn roo_events(obj: &Value) -> Vec<RawEvent> {
    obj.as_array()
        .into_iter()
        .flatten()
        .flat_map(|row| message_events(row, as_str(row.get("ts"))))
        .collect()
}

fn parse_embedded_json(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or(Value::Null),
        Some(value) => value.clone(),
        None => Value::Null,
    }
}

pub fn opencode_events(obj: &Value) -> Vec<RawEvent> {
    let mut events = Vec::new();
    for session in obj
        .get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let session_id = as_str(session.get("id"));
        let cwd = as_str(session.get("cwd"));
        let agent = as_str(session.get("agent")).or_else(|| as_str(session.get("model")));
        for record in session
            .get("records")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let message = parse_embedded_json(record.get("message"));
            let part = parse_embedded_json(record.get("part"));
            if part.is_null() {
                continue;
            }
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("assistant");
            let timestamp = message
                .get("time")
                .and_then(|time| time.get("created"))
                .map(|value| value.to_string())
                .or_else(|| record.get("message_time").map(|value| value.to_string()));
            let mut emitted = match part.get("type").and_then(Value::as_str).unwrap_or("") {
                "text" => {
                    let mut event = RawEvent::new(&format!("{role}_message"));
                    event.role = Some(role.into());
                    event.timestamp = timestamp;
                    event.text = value_text(part.get("text"));
                    vec![event]
                }
                "reasoning" => {
                    let mut event = RawEvent::new("thinking");
                    event.role = Some("assistant".into());
                    event.timestamp = timestamp;
                    event.text = "private reasoning omitted".into();
                    event.private_reasoning_omitted = true;
                    event.meta = true;
                    vec![event]
                }
                "tool" => {
                    let state = part.get("state").unwrap_or(&Value::Null);
                    let tool = as_str(part.get("tool")).or(Some("unknown".into()));
                    let call_id = as_str(part.get("callID")).or_else(|| as_str(part.get("callId")));
                    let mut call = RawEvent::new("tool_call");
                    call.role = Some("assistant".into());
                    call.timestamp = timestamp.clone();
                    call.tool = tool.clone();
                    call.call_id = call_id.clone();
                    call.text = value_text(state.get("input"));
                    let mut pair = vec![call];
                    if matches!(
                        state.get("status").and_then(Value::as_str),
                        Some("completed" | "error")
                    ) {
                        let mut result = RawEvent::new("tool_result");
                        result.role = Some("user".into());
                        result.timestamp = timestamp;
                        result.tool = tool;
                        result.call_id = call_id;
                        result.text =
                            value_text(state.get("output").or_else(|| state.get("error")));
                        result.is_error =
                            state.get("status").and_then(Value::as_str) == Some("error");
                        pair.push(result);
                    }
                    pair
                }
                _ => Vec::new(),
            };
            for event in &mut emitted {
                event.session_id = session_id.clone();
                event.cwd = cwd.clone();
                event.agent_role = agent.clone();
            }
            events.extend(emitted);
        }
    }
    events
}

pub fn cursor_events(obj: &Value) -> Vec<RawEvent> {
    let mut events = Vec::new();
    for session in obj
        .get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let session_id = as_str(session.get("id"));
        let cwd = as_str(session.get("workspace_id"));
        for bubble in session
            .get("bubbles")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let value = parse_embedded_json(bubble.get("value"));
            let bubble_type = value
                .get("type")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let mut emitted = if bubble_type == 1 {
                let text = value
                    .get("richText")
                    .and_then(|rich| rich.get("text"))
                    .or_else(|| value.get("text"));
                let mut event = RawEvent::new("user_message");
                event.role = Some("user".into());
                event.text = value_text(text);
                vec![event]
            } else if bubble_type == 2 {
                if let Some(tool) = value.get("toolFormerData") {
                    let mut call = RawEvent::new("tool_call");
                    call.role = Some("assistant".into());
                    call.tool = as_str(tool.get("name"))
                        .or_else(|| as_str(tool.get("toolName")))
                        .or(Some("unknown".into()));
                    call.call_id = as_str(tool.get("toolCallId"));
                    call.text = value_text(tool.get("params").or_else(|| tool.get("rawArgs")));
                    let mut pair = vec![call];
                    if tool.get("result").is_some() {
                        let mut result = RawEvent::new("tool_result");
                        result.role = Some("user".into());
                        result.tool = pair[0].tool.clone();
                        result.call_id = pair[0].call_id.clone();
                        result.text = value_text(tool.get("result"));
                        result.is_error = tool
                            .get("status")
                            .and_then(Value::as_str)
                            .is_some_and(|status| status.eq_ignore_ascii_case("error"));
                        pair.push(result);
                    }
                    pair
                } else if value.get("thinking").is_some() {
                    let mut event = RawEvent::new("thinking");
                    event.role = Some("assistant".into());
                    event.text = "private reasoning omitted".into();
                    event.private_reasoning_omitted = true;
                    event.meta = true;
                    vec![event]
                } else {
                    let mut event = RawEvent::new("assistant_message");
                    event.role = Some("assistant".into());
                    event.text = value_text(value.get("text"));
                    vec![event]
                }
            } else {
                Vec::new()
            };
            for event in &mut emitted {
                event.session_id = session_id.clone();
                event.cwd = cwd.clone();
            }
            events.extend(emitted);
        }
    }
    events
}

// ---- Claude Code adapter ----

/// Claude Code rows: `{"type":"user"|"assistant","message":{...}}` with
/// `message.content` either a string or a block list (`text`, `thinking`,
/// `tool_use` on assistant, `tool_result` on user rows).
pub fn claude_events(obj: &Value) -> Vec<RawEvent> {
    let row_type = obj.get("type").and_then(Value::as_str).unwrap_or("");
    let timestamp = as_str(obj.get("timestamp"));

    if row_type != "user" && row_type != "assistant" {
        if matches!(
            row_type,
            "queue-operation" | "file-history-snapshot" | "summary"
        ) {
            let mut ev = RawEvent::new("meta");
            ev.timestamp = timestamp;
            ev.text = obj
                .get("content")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or(row_type)
                .to_string();
            ev.meta = true;
            ev.synthetic = true;
            return vec![ev];
        }
        return Vec::new();
    }

    if obj
        .get("isSidechain")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Vec::new();
    }
    if row_type == "user"
        && (obj.get("isMeta").and_then(Value::as_bool).unwrap_or(false)
            || obj
                .get("isCompactSummary")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            || obj.get("toolUseResult").is_some()
            || obj
                .get("origin")
                .and_then(Value::as_object)
                .is_some_and(|origin| origin.get("kind").and_then(Value::as_str) != Some("human"))
            || (obj.get("promptSource").and_then(Value::as_str) == Some("sdk")
                && obj.get("entrypoint").and_then(Value::as_str) == Some("sdk-cli")))
    {
        return Vec::new();
    }

    let Some(message) = obj.get("message").filter(|m| m.is_object()) else {
        return Vec::new();
    };
    let Some(content) = message.get("content") else {
        return Vec::new();
    };

    let mut events = Vec::new();

    if let Some(text) = content.as_str() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            let mut ev = RawEvent::new(&format!("{row_type}_message"));
            ev.role = Some(row_type.to_string());
            ev.timestamp = timestamp;
            ev.text = trimmed.to_string();
            if row_type == "user" && injected_context(&ev.text) {
                return Vec::new();
            }
            events.push(ev);
        }
        return events;
    }

    let Some(blocks) = content.as_array() else {
        return events;
    };

    for block in blocks {
        let Some(block) = block.as_object() else {
            continue;
        };
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
        match block_type {
            "text" => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    let mut ev = RawEvent::new(&format!("{row_type}_message"));
                    ev.role = Some(row_type.to_string());
                    ev.timestamp = timestamp.clone();
                    ev.text = text.to_string();
                    if row_type != "user" || !injected_context(&ev.text) {
                        events.push(ev);
                    }
                }
            }
            "thinking" => {
                let mut ev = RawEvent::new("thinking");
                ev.role = Some(row_type.to_string());
                ev.timestamp = timestamp.clone();
                ev.private_reasoning_omitted = true;
                ev.meta = true;
                events.push(ev);
            }
            "tool_use" if row_type == "assistant" => {
                let mut ev = RawEvent::new("tool_call");
                ev.role = Some("assistant".to_string());
                ev.timestamp = timestamp.clone();
                ev.tool = Some(
                    block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string(),
                );
                ev.call_id = as_str(block.get("id"));
                let empty_input: Value = Value::Object(serde_json::Map::new());
                ev.text = py_json_dumps(block.get("input").unwrap_or(&empty_input));
                events.push(ev);
            }
            "tool_result" if row_type == "user" => {
                let content_value = match block.get("content") {
                    None | Some(Value::Null) => Value::String(String::new()),
                    Some(v @ Value::String(_)) => v.clone(),
                    Some(other) => Value::String(py_json_dumps(other)),
                };
                let mut ev = RawEvent::new("tool_result");
                ev.role = Some("user".to_string());
                ev.timestamp = timestamp.clone();
                ev.call_id = as_str(block.get("tool_use_id"));
                ev.text = content_value.as_str().unwrap_or_default().to_string();
                ev.is_error = block
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                events.push(ev);
            }
            _ => {}
        }
    }
    events
}

// ---- Codex adapter ----

fn codex_text_blocks(content: &Value, allowed: &[&str]) -> Vec<String> {
    if let Some(s) = content.as_str() {
        return vec![s.to_string()];
    }
    let Some(items) = content.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|b| {
            let b = b.as_object()?;
            if !allowed.contains(&b.get("type").and_then(Value::as_str)?) {
                return None;
            }
            b.get("text").and_then(Value::as_str).map(str::to_string)
        })
        .collect()
}

/// Codex rows: `{"type":"response_item","timestamp":"...","payload":{...}}`
/// with payload types `message`, `function_call`/`custom_tool_call`,
/// `function_call_output`/`custom_tool_call_output`, `reasoning` (private,
/// omitted), and `event_msg`.
pub fn codex_events(obj: &Value) -> Vec<RawEvent> {
    if obj.get("type").and_then(Value::as_str) != Some("response_item") {
        return Vec::new();
    }
    let Some(payload) = obj.get("payload").filter(|p| p.is_object()) else {
        return Vec::new();
    };
    let kind = payload.get("type").and_then(Value::as_str).unwrap_or("");
    let timestamp = as_str(obj.get("timestamp"));

    match kind {
        "message" => {
            let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
            if role != "user" && role != "assistant" {
                return Vec::new();
            }
            let blocks = codex_text_blocks(
                payload.get("content").unwrap_or(&Value::Null),
                &["input_text", "output_text", "text"],
            );
            let text = blocks.join("\n");
            let text = if role == "user" {
                codex_user_text(text)
            } else {
                text
            };
            if text.trim().is_empty() {
                return Vec::new();
            }
            if role == "user" && injected_context(&text) {
                return Vec::new();
            }
            let mut ev = RawEvent::new(&format!("{role}_message"));
            ev.role = Some(role.to_string());
            ev.timestamp = timestamp;
            ev.text = text;
            vec![ev]
        }
        "function_call" | "custom_tool_call" => {
            let name = payload
                .get("name")
                .or_else(|| payload.get("tool_name"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let arguments = payload
                .get("arguments")
                .or_else(|| payload.get("input"))
                .cloned()
                .unwrap_or(Value::String(String::new()));
            let mut ev = RawEvent::new("tool_call");
            ev.role = Some("assistant".to_string());
            ev.timestamp = timestamp;
            ev.tool = Some(name.to_string());
            ev.call_id = as_str(payload.get("call_id"));
            // Python used str(arguments): strings pass through verbatim; any
            // other JSON shape serializes canonically here (deterministic).
            ev.text = match &arguments {
                Value::String(s) => s.clone(),
                other => py_json_dumps(other),
            };
            vec![ev]
        }
        "function_call_output" | "custom_tool_call_output" => {
            let output = payload
                .get("output")
                .or_else(|| payload.get("content"))
                .cloned()
                .unwrap_or(Value::String(String::new()));
            let mut ev = RawEvent::new("tool_result");
            ev.role = Some("user".to_string());
            ev.timestamp = timestamp;
            ev.call_id = as_str(payload.get("call_id"));
            ev.text = match &output {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                // Deterministic canonical serialization (Python used str(),
                // whose dict repr is not reproducible across runtimes).
                other => py_json_dumps(other),
            };
            ev.is_error = payload
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            vec![ev]
        }
        "reasoning" => {
            let mut ev = RawEvent::new("thinking");
            ev.role = Some("assistant".to_string());
            ev.timestamp = timestamp;
            ev.private_reasoning_omitted = true;
            ev.meta = true;
            vec![ev]
        }
        "event_msg" => {
            let mut ev = RawEvent::new("meta");
            ev.timestamp = timestamp;
            ev.text = payload
                .get("type")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("event_msg")
                .to_string();
            ev.meta = true;
            ev.synthetic = true;
            vec![ev]
        }
        _ => Vec::new(),
    }
}

// ---- GitHub Copilot CLI adapter ----

/// Copilot CLI rows. System steering uses the same message shape as typed
/// input, so `data.source=system` is excluded before it can become user
/// evidence.
pub fn copilot_events(obj: &Value) -> Vec<RawEvent> {
    let row_type = obj.get("type").and_then(Value::as_str).unwrap_or("");
    if !matches!(row_type, "user.message" | "assistant.message") {
        return Vec::new();
    }
    let Some(data) = obj.get("data").and_then(Value::as_object) else {
        return Vec::new();
    };
    if row_type == "user.message"
        && data.get("source").and_then(Value::as_str) == Some("system")
    {
        return Vec::new();
    }
    let text = data
        .get("content")
        .or_else(|| data.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if text.is_empty() || (row_type == "user.message" && injected_context(text)) {
        return Vec::new();
    }
    let role = if row_type == "user.message" {
        "user"
    } else {
        "assistant"
    };
    let mut event = RawEvent::new(&format!("{role}_message"));
    event.role = Some(role.into());
    event.timestamp = as_str(obj.get("timestamp"));
    event.text = text.into();
    vec![event]
}

// ---- Google Antigravity adapter ----

/// Antigravity's `USER_INPUT` row is admissible only when its source is
/// explicitly `USER_EXPLICIT`. Harness metadata outside `<USER_REQUEST>` is
/// discarded.
pub fn antigravity_events(obj: &Value) -> Vec<RawEvent> {
    if obj.get("type").and_then(Value::as_str) != Some("USER_INPUT")
        || obj.get("source").and_then(Value::as_str) != Some("USER_EXPLICIT")
    {
        return Vec::new();
    }
    let content = obj.get("content").and_then(Value::as_str).unwrap_or("");
    let texts = ANTIGRAVITY_USER_REQUEST
        .captures_iter(content)
        .filter_map(|capture| capture.get(1).map(|value| value.as_str()))
        .collect::<Vec<_>>();
    let texts = if texts.is_empty() {
        vec![content]
    } else {
        texts
    };
    texts
        .into_iter()
        .map(str::trim)
        .filter(|text| !text.is_empty() && !injected_context(text))
        .map(|text| {
            let mut event = RawEvent::new("user_message");
            event.role = Some("user".into());
            event.timestamp = as_str(obj.get("created_at"));
            event.text = text.into();
            event
        })
        .collect()
}

// ---- Generic frozen adapter ----

const GENERIC_KINDS: &[&str] = &[
    "user_message",
    "assistant_message",
    "tool_call",
    "tool_result",
    "thinking",
    "meta",
];

/// Reads one already-normalized event from a hash-bound frozen snapshot row:
/// `{"type":"adapt_event_v1","host":"...","event":{...}}`.
pub fn generic_events(obj: &Value) -> Vec<RawEvent> {
    if obj.get("type").and_then(Value::as_str) != Some("adapt_event_v1") {
        return Vec::new();
    }
    let host = obj.get("host").and_then(Value::as_str).unwrap_or("");
    let known = host == "claude_code" || host == "codex" || GENERIC_HOSTS.contains(&host);
    if !known {
        return Vec::new();
    }
    let Some(event) = obj.get("event").filter(|e| e.is_object()) else {
        return Vec::new();
    };
    let kind = event.get("kind").and_then(Value::as_str).unwrap_or("");
    if !GENERIC_KINDS.contains(&kind) {
        return Vec::new();
    }

    let mut ev = RawEvent::new(kind);
    ev.role = as_str(event.get("role"));
    ev.timestamp = as_str(event.get("timestamp"));
    ev.text = event
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    ev.tool = as_str(event.get("tool"));
    ev.call_id = as_str(event.get("call_id"));
    ev.is_error = event
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    ev.synthetic = event
        .get("synthetic")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    ev.meta = event.get("meta").and_then(Value::as_bool).unwrap_or(false);
    ev.private_reasoning_omitted = event
        .get("private_reasoning_omitted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    ev.redacted = event.get("redacted").and_then(Value::as_bool);
    ev.is_sidechain = event.get("is_sidechain").and_then(Value::as_bool);
    ev.agent_role = as_str(event.get("agentRole"));
    ev.thread_source = as_str(event.get("threadSource"));
    ev.parent_thread_id = as_str(event.get("parentThreadId"));
    ev.cwd = as_str(event.get("cwd"));
    ev.repo = as_str(event.get("repo"));
    vec![ev]
}

// ---- Host detection ----

/// Detect the host by scanning the first (up to) 20 rows: Codex rows carry
/// `type` in `{session_meta, response_item, event_msg}` with a nested
/// `payload`; Claude Code rows carry `type` in `{user, assistant,
/// queue-operation}`; frozen snapshots declare `adapt_event_v1` plus their own
/// `host`. Falls back to `claude_code`.
pub fn detect_host(path: &Path) -> std::io::Result<&'static str> {
    let path_text = path.to_string_lossy().replace('\\', "/");
    if path.file_name().and_then(|value| value.to_str()) == Some("opencode.db") {
        return Ok("opencode");
    }
    if path.file_name().and_then(|value| value.to_str()) == Some("state.vscdb") {
        return Ok("cursor");
    }
    if path_text.contains("/.commandcode/") {
        return Ok("command_code");
    }
    if path_text.contains("/.pi/") {
        return Ok("pi");
    }
    if path_text.contains("/.qwen") {
        return Ok("qwen");
    }
    if path_text.contains("/.cline/") {
        return Ok("cline");
    }
    if path_text.contains("/antigravity/") || path_text.contains("/.system_generated/logs/") {
        return Ok("antigravity");
    }
    if path_text.contains("/.gemini/") {
        return Ok("gemini");
    }
    if path_text.contains("/.grok/") {
        return Ok("grok_build");
    }
    if path_text.contains("roo-cline") {
        return Ok("roo_cline");
    }
    if path_text.contains("/.copilot/") {
        return Ok("copilot");
    }
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    for line in std::io::BufRead::lines(reader).take(20) {
        let line = line?;
        let Ok(obj) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(map) = obj.as_object() else { continue };
        match map.get("type").and_then(Value::as_str) {
            Some("adapt_event_v1") => {
                if let Some(host) = map.get("host").and_then(Value::as_str) {
                    return Ok(known_host(host));
                }
            }
            Some(t) if matches!(t, "session_meta" | "response_item" | "event_msg") => {
                return Ok("codex");
            }
            Some(t) if matches!(t, "user" | "assistant" | "queue-operation") => {
                return Ok("claude_code");
            }
            Some(t) if matches!(t, "user.message" | "assistant.message") => {
                return Ok("copilot");
            }
            Some("USER_INPUT") => return Ok("antigravity"),
            Some("session") if map.contains_key("cwd") => return Ok("pi"),
            Some("message") if map.get("message").is_some() => return Ok("pi"),
            _ => {}
        }
    }
    Ok("claude_code")
}

fn known_host(host: &str) -> &'static str {
    for candidate in GENERIC_HOSTS {
        if *candidate == host {
            return candidate;
        }
    }
    match host {
        "claude_code" => "claude_code",
        "codex" => "codex",
        "copilot" => "copilot",
        "antigravity" => "antigravity",
        _ => "",
    }
}

/// Public dispatch: route one decoded row through the right adapter.
/// `adapt_event_v1` rows always take the generic path regardless of `host`.
pub fn iter_events_for_host(host: &str, obj: &Value) -> Vec<RawEvent> {
    if obj.get("type").and_then(Value::as_str) == Some("adapt_event_v1") {
        return generic_events(obj);
    }
    match host {
        "claude_code" => claude_events(obj),
        "codex" => codex_events(obj),
        "command_code" | "pi" | "qwen" => command_family_events(obj),
        "cline" => cline_events(obj),
        "gemini" => gemini_events(obj),
        "grok_build" => grok_events(obj),
        "roo_cline" => roo_events(obj),
        "opencode" => opencode_events(obj),
        "cursor" => cursor_events(obj),
        "copilot" => copilot_events(obj),
        "antigravity" => antigravity_events(obj),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_text_and_blocks() {
        let row = json!({
            "type": "assistant",
            "timestamp": "t0",
            "sessionId": "s",
            "message": {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "secret plan"},
                {"type": "text", "text": "hello"},
                {"type": "tool_use", "id": "tu_1", "name": "edit", "input": {"path": "a.rs"}}
            ]}
        });
        let evs = claude_events(&row);
        assert_eq!(evs.len(), 3);
        assert!(evs[0].private_reasoning_omitted && evs[0].meta);
        assert_eq!(evs[1].kind, "assistant_message");
        assert_eq!(evs[2].kind, "tool_call");
        assert_eq!(evs[2].call_id.as_deref(), Some("tu_1"));
        assert_eq!(evs[2].text, r#"{"path": "a.rs"}"#);
    }

    #[test]
    fn claude_sidechain_dropped_and_meta_rows() {
        let side = json!({"type":"user","isSidechain":true,"message":{"content":"x"}});
        assert!(claude_events(&side).is_empty());
        let meta = json!({"type":"summary","content":"did things"});
        let evs = claude_events(&meta);
        assert_eq!(evs.len(), 1);
        assert!(evs[0].meta && evs[0].synthetic);
        assert_eq!(evs[0].text, "did things");
    }

    #[test]
    fn codex_full_flow() {
        let call = json!({"type":"response_item","timestamp":"t","payload":{
            "type":"function_call","name":"shell","call_id":"c1","arguments":"cargo test"}});
        let out = codex_events(&call);
        assert_eq!(out[0].text, "cargo test");
        let reasoning = json!({"type":"response_item","payload":{"type":"reasoning"}});
        assert!(codex_events(&reasoning)[0].private_reasoning_omitted);
        let msg = json!({"type":"response_item","payload":{"type":"message","role":"user",
            "content":[{"type":"input_text","text":"hi"},{"type":"input_text","text":"there"}]}});
        assert_eq!(codex_events(&msg)[0].text, "hi\nthere");
    }

    #[test]
    fn generic_requires_known_host_kind_and_type() {
        let ok = json!({"type":"adapt_event_v1","host":"pi",
            "event":{"kind":"user_message","text":"hello"}});
        assert_eq!(generic_events(&ok)[0].text, "hello");
        let bad_host = json!({"type":"adapt_event_v1","host":"martian",
            "event":{"kind":"user_message","text":"x"}});
        assert!(generic_events(&bad_host).is_empty());
        let bad_kind = json!({"type":"adapt_event_v1","host":"pi",
            "event":{"kind":"telepathy","text":"x"}});
        assert!(generic_events(&bad_kind).is_empty());
    }

    #[test]
    fn dispatch_prefers_generic_path_for_snapshots() {
        let snap = json!({"type":"adapt_event_v1","host":"qwen",
            "event":{"kind":"tool_call","tool":"read","text":"x"}});
        assert_eq!(iter_events_for_host("claude_code", &snap).len(), 1);
    }
}
