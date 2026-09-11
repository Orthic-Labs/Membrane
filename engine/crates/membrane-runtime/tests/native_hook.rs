use std::{thread, time::Duration};

use membrane_protocol::{
    normalize_hook_payload, project_hook_host_response, HookModuleId, HookModuleState, HookEvent,
    HOOK_MODULE_DEADLINE_MS,
};
use membrane_runtime::hook::{NativeHookRuntime, NativeHookService};
use serde_json::json;

struct SlowRecall;
impl NativeHookService for SlowRecall {
    fn recall(&self, _: &membrane_protocol::HookInputEnvelopeV1, _: Duration) -> Result<Option<String>, String> {
        thread::sleep(Duration::from_millis(HOOK_MODULE_DEADLINE_MS + 100));
        Ok(Some("must not reach host".into()))
    }
}

#[test]
fn native_dispatch_is_fixed_order_with_typed_skips_for_unknown_event() {
    let input = normalize_hook_payload(json!({"event": "FutureHostEvent", "opaque": true}))
        .expect("unknown host event normalizes");
    let result = NativeHookRuntime::default().dispatch(&input);
    assert_eq!(result.results.len(), HookModuleId::ORDERED.len());
    assert_eq!(result.results.iter().map(|entry| entry.id).collect::<Vec<_>>(), HookModuleId::ORDERED);
    assert!(result.results.iter().all(|entry| entry.output.as_ref()
        .is_some_and(|output| output.state == HookModuleState::Skipped)));
}

#[test]
fn native_recall_deadline_returns_content_free_typed_error() {
    let input = normalize_hook_payload(json!({"event": "UserPromptSubmit", "session_id": "s", "prompt": "secret input"}))
        .expect("payload normalizes");
    let result = NativeHookRuntime::new(SlowRecall, false).dispatch(&input);
    let recall = result.results.iter().find(|entry| entry.id == HookModuleId::MemoryRecall).expect("recall result");
    assert_eq!(recall.error.as_deref(), Some("module_deadline_exceeded"));
    let encoded = serde_json::to_string(recall).expect("result serializes");
    assert!(!encoded.contains("secret input"));
    assert!(!encoded.contains("must not reach host"));
}

#[test]
fn failure_and_episode_are_content_free_with_sha256_episode_digest() {
    let failure = normalize_hook_payload(json!({"event": "PostToolUseFailure", "error": "secret=never-expose"})).expect("failure normalizes");
    let failure_result = NativeHookRuntime::default().dispatch(&failure);
    let failure_output = failure_result.results.iter().find(|entry| entry.id == HookModuleId::MemoryFailure)
        .and_then(|entry| entry.output.as_ref()).expect("failure output");
    assert_eq!(failure_output.detail["contentFree"], true);
    assert!(!serde_json::to_string(failure_output).expect("serializes").contains("never-expose"));

    let episode = normalize_hook_payload(json!({"event": "TaskCompleted", "session_id": "s", "outcomes": ["secret outcome"]})).expect("episode normalizes");
    let episode_result = NativeHookRuntime::default().dispatch(&episode);
    let episode_output = episode_result.results.iter().find(|entry| entry.id == HookModuleId::MemoryEpisode)
        .and_then(|entry| entry.output.as_ref()).expect("episode output");
    assert_eq!(episode_output.detail["contentFree"], true);
    assert!(episode_output.detail["outcomeDigest"].as_str().is_some_and(|digest| digest.starts_with("sha256:")));
    assert!(!serde_json::to_string(episode_output).expect("serializes").contains("secret outcome"));
}

#[test]
fn native_hook_source_never_spawns_node_or_python() {
    let source = include_str!("../src/hook.rs").to_ascii_lowercase();
    assert!(!source.contains("command::new(\"node"));
    assert!(!source.contains("command::new(\"python"));
    assert!(!source.contains("node.exe"));
    assert!(!source.contains("python.exe"));
}

#[test]
fn ambient_hook_budget_fits_codex_ten_second_timeout() {
    assert!(HOOK_MODULE_DEADLINE_MS < 10_000);
    assert!(HOOK_MODULE_DEADLINE_MS >= 8_000);
}

#[test]
fn session_end_is_typed_without_invalid_host_projection() {
    let input = normalize_hook_payload(json!({"event":"SessionEnd", "session_id":"resume-1"})).unwrap();
    let response = project_hook_host_response(NativeHookRuntime::default().dispatch(&input));
    assert_eq!(input.event, HookEvent::SessionEnd);
    let encoded = serde_json::to_value(response).unwrap();
    assert!(encoded.get("hookSpecificOutput").is_none());
    assert_eq!(encoded["membraneHook"]["event"], "SessionEnd");
}
