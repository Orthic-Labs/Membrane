use membrane_protocol::{
    normalize_hook_payload, project_hook_host_response, HookDispatchResultV1, HookEvent,
    HookHostDecision, HookInvocationStatus, HookModuleId, HookModuleOutputV1, HookModuleResultV1,
    HookModuleState, HookPermissionDecision,
};
use serde_json::json;

const SHIPPED_EVENTS: [&str; 10] = [
    "SessionStart", "UserPromptSubmit", "PreCompact", "PostCompact", "PreToolUse",
    "PostToolUse", "Stop", "PostToolUseFailure", "TaskCompleted", "SessionEnd",
];

#[test]
fn normalizes_every_shipped_event_and_echoes_unknown_raw_host_event() {
    for event in SHIPPED_EVENTS {
        let raw = json!({"hookEventName": event, "thread_id": " session ", "toolName": "Bash", "tool_input": {"command": "pnpm test"}});
        let envelope = normalize_hook_payload(raw.clone()).expect("shipped event normalizes");
        assert_eq!(envelope.event.as_str(), event);
        assert_eq!(envelope.session_id.as_deref(), Some("session"));
        assert_eq!(envelope.tool_name.as_deref(), Some("Bash"));
        assert_eq!(envelope.payload, raw, "raw matcher input is preserved for {event}");
    }

    let envelope = normalize_hook_payload(json!({"event": "FutureHostEvent", "opaque": {"match": "raw"}}))
        .expect("forward-compatible event normalizes");
    assert_eq!(envelope.event, HookEvent::Unknown("FutureHostEvent".into()));
    let response = project_hook_host_response(HookDispatchResultV1::new(
        envelope.event,
        HookModuleId::ORDERED.into_iter().map(HookModuleResultV1::skipped).collect(),
    ));
    assert_eq!(response.hook_specific_output.hook_event_name.as_str(), "FutureHostEvent");
    assert_eq!(response.membrane_hook.results.len(), 16);
    assert!(response.membrane_hook.results.iter().all(|result| result.output.as_ref()
        .is_some_and(|output| output.state == HookModuleState::Skipped)));
}

#[test]
fn projects_fixed_order_context_and_deny_only_at_boundaries() {
    let results = vec![
        HookModuleResultV1::ok(HookModuleId::CortexStatus, HookModuleOutputV1::status(
            HookModuleState::Available, "healthy", json!({"additionalContext": "first"}),
        )),
        HookModuleResultV1::ok(HookModuleId::DiagnosticsFence, HookModuleOutputV1::status(
            HookModuleState::Blocked, "fence_not_cleared", json!({"detail": "blocked", "additionalContext": "second"}),
        )),
    ];
    for boundary in [HookEvent::PreToolUse, HookEvent::Stop] {
        let response = project_hook_host_response(HookDispatchResultV1::new(boundary, results.clone()));
        assert_eq!(response.decision, Some(HookHostDecision::Block));
        assert_eq!(response.hook_specific_output.permission_decision, Some(HookPermissionDecision::Deny));
        assert_eq!(response.hook_specific_output.additional_context, "first\n\nsecond");
    }

    for event in [HookEvent::SessionStart, HookEvent::UserPromptSubmit, HookEvent::PostToolUse, HookEvent::PostToolUseFailure, HookEvent::TaskCompleted, HookEvent::SessionEnd] {
        let response = project_hook_host_response(HookDispatchResultV1::new(event, vec![
            HookModuleResultV1::ok(HookModuleId::DiagnosticsFence, HookModuleOutputV1::status(HookModuleState::Blocked, "blocked", json!({"detail": "must_not_deny"}))),
        ]));
        assert_eq!(response.decision, None);
        assert_eq!(response.hook_specific_output.permission_decision, None);
    }
}

#[test]
fn typed_failure_is_content_free_and_aggregates_error_status() {
    let result = HookDispatchResultV1::new(HookEvent::PostToolUseFailure, vec![
        HookModuleResultV1::error(HookModuleId::MemoryFailure, "module_deadline_exceeded"),
    ]);
    assert_eq!(result.status, HookInvocationStatus::Error);
    let encoded = serde_json::to_string(&result).expect("result serializes");
    assert!(!encoded.contains("secret"));
    assert!(encoded.contains("module_deadline_exceeded"));
}
