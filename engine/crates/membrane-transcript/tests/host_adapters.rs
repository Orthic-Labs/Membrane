use std::io::Write;

use membrane_transcript::adapters::{
    antigravity_events, claude_events, codex_events, copilot_events, detect_host,
    opencode_events,
};
use membrane_transcript::parse_source_events;
use serde_json::json;

#[test]
fn named_hosts_filter_non_user_origins() {
    let claude_sdk = json!({
        "type":"user", "promptSource":"sdk", "entrypoint":"sdk-cli",
        "message":{"role":"user","content":"injected"}
    });
    assert!(claude_events(&claude_sdk).is_empty());
    let claude_human = json!({
        "type":"user", "origin":{"kind":"human"},
        "message":{"role":"user","content":"typed"}
    });
    assert_eq!(claude_events(&claude_human)[0].text, "typed");

    let codex_control = json!({"type":"response_item","payload":{
        "type":"message","role":"user","content":[{"type":"input_text","text":
        "<heartbeat>injected</heartbeat>"}]}});
    assert!(codex_events(&codex_control).is_empty());

    let copilot_system = json!({
        "type":"user.message","data":{"source":"system","content":"steering"}
    });
    assert!(copilot_events(&copilot_system).is_empty());

    let antigravity_harness = json!({
        "type":"USER_INPUT","source":"HARNESS","content":"not typed"
    });
    assert!(antigravity_events(&antigravity_harness).is_empty());
}

#[test]
fn opencode_adapter_emits_typed_user_event() {
    let row = json!({"sessions":[{"id":"s1","records":[{
        "message":{"role":"user","time":{"created":1}},
        "part":{"type":"text","text":"typed in OpenCode"}
    }]}]});
    let events = opencode_events(&row);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "user_message");
    assert_eq!(events[0].session_id.as_deref(), Some("s1"));
}

#[test]
fn copilot_parser_redacts_secrets_and_dedupes_long_reinjected_turns() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let repeated = format!("{} password: hunter2hunter2", "typed specification ".repeat(20));
    let mut file = std::fs::File::create(&path).unwrap();
    for row in [
        json!({"type":"user.message","data":{"source":"system","content":"steering"}}),
        json!({"type":"user.message","timestamp":"t1","data":{"source":"user","content":repeated.clone()}}),
        json!({"type":"user.message","timestamp":"t2","data":{"source":"user","content":repeated}}),
    ] {
        writeln!(file, "{}", serde_json::to_string(&row).unwrap()).unwrap();
    }
    let events = parse_source_events(&path, Some("copilot")).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].host, "copilot");
    assert!(events[0].redacted);
    assert!(events[0].text.contains("[REDACTED]"));
    assert!(!events[0].text.contains("hunter2"));
}

#[test]
fn antigravity_keeps_only_explicit_wrapped_request() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("transcript.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"type\":\"USER_INPUT\",\"source\":\"HARNESS\",\"content\":\"ignore\"}\n",
            "{\"type\":\"USER_INPUT\",\"source\":\"USER_EXPLICIT\",\"created_at\":\"t\",",
            "\"content\":\"<USER_REQUEST>keep this</USER_REQUEST><ADDITIONAL_METADATA>drop this</ADDITIONAL_METADATA>\"}\n"
        ),
    )
    .unwrap();
    let events = parse_source_events(&path, Some("antigravity")).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].text, "keep this");
}

#[test]
fn antigravity_path_wins_over_generic_gemini_detection() {
    let path = std::path::Path::new(
        "C:/Users/test/.gemini/antigravity/brain/session/.system_generated/logs/events.jsonl",
    );
    assert_eq!(detect_host(path).unwrap(), "antigravity");
}
