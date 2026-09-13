//! Native HookHost runtime. Modules execute in-process under a per-module
//! deadline with a bounded leaf-process scope: the only processes a module
//! may start are tracked leaf helpers (today: `git` for the diagnostics
//! fence), which the dispatcher reaps when the deadline fires before serial
//! dispatch advances. No `membrane.exe hook-module` child is ever spawned.

use std::{sync::{mpsc, Arc}, time::Duration};

use crate::providers::child_process::{ContainmentScope, ScopeGuard};

use membrane_protocol::{
    normalize_hook_payload, project_hook_host_response, HookDispatchResultV1, HookEvent,
    HookHostResponseV1, HookInputEnvelopeV1, HookModuleId, HookModuleOutputV1,
    HookModuleResultV1, HookModuleState, HOOK_MODULE_DEADLINE_MS,
};
use serde_json::{json, Value};

pub use crate::hook_diagnostics::RecallOutcome;

pub const HEALTH_DEADLINE_MS: u64 = 800;
pub const TELEMETRY_DEADLINE_MS: u64 = 1_000;
pub const DIAGNOSTICS_READ_DEADLINE_MS: u64 = 800;
pub const DIAGNOSTICS_WRITE_DEADLINE_MS: u64 = 1_200;
pub const GIT_DEADLINE_MS: u64 = 1_500;
pub const GIT_OUTPUT_LIMIT_BYTES: usize = 2 * 1024 * 1024;

/// Native service boundary.  Implementations are supplied by the installed
/// controller; `NoNativeHookService` gives deterministic safe degradation.
pub trait NativeHookService: Send + Sync + 'static {
    fn healthy(&self, _input: &HookInputEnvelopeV1, _deadline: Duration) -> Result<bool, String> { Ok(false) }
    fn recall(&self, _input: &HookInputEnvelopeV1, _deadline: Duration) -> Result<Option<String>, String> { Ok(None) }
    fn recall_detailed(&self, input: &HookInputEnvelopeV1, deadline: Duration) -> Result<crate::hook_diagnostics::RecallOutcome, String> {
        self.recall(input, deadline).map(|context| RecallOutcome { sufficient: context.is_some(), reason: if context.is_some() { "memory_recalled" } else { "membrane_no_matches" }, context, detail: Value::Null })
    }
    fn diagnostics_fence(&self, _input: &HookInputEnvelopeV1, _completion: bool, _deadline: Duration) -> Result<bool, String> { Ok(false) }
}

#[derive(Default)]
pub struct NoNativeHookService;
impl NativeHookService for NoNativeHookService {
    fn healthy(&self, _input: &HookInputEnvelopeV1, _deadline: Duration) -> Result<bool, String> {
        Ok(crate::hook_diagnostics::resident_healthy())
    }
    fn recall(&self, input: &HookInputEnvelopeV1, _deadline: Duration) -> Result<Option<String>, String> {
        Ok(crate::hook_diagnostics::resident_recall(input))
    }
    fn recall_detailed(&self, input: &HookInputEnvelopeV1, _deadline: Duration) -> Result<crate::hook_diagnostics::RecallOutcome, String> {
        Ok(crate::hook_diagnostics::recall_attempt(input, &crate::hook_diagnostics::recall_task_for_service(input)))
    }
}

pub struct NativeHookRuntime<S = NoNativeHookService> { service: Arc<S>, enforcement_enabled: bool }

impl Default for NativeHookRuntime<NoNativeHookService> {
    fn default() -> Self { Self::new(NoNativeHookService, false) }
}

impl<S: NativeHookService> NativeHookRuntime<S> {
    /// Test/default supervisor: deterministic, in-process execution.
    pub fn new(service: S, enforcement_enabled: bool) -> Self { Self { service: Arc::new(service), enforcement_enabled } }

    /// Serial, fixed-order execution. Every module receives full raw host input
    /// through `HookInputEnvelopeV1::payload`; no secret-bearing error escapes.
    pub fn dispatch(&self, input: &HookInputEnvelopeV1) -> HookDispatchResultV1 {
        let results = HookModuleId::ORDERED.into_iter().map(|id| {
            let mut result = self.invoke(id, input);
            if id == HookModuleId::DiagnosticsFence && input.event == HookEvent::PreToolUse
                && crate::hook_diagnostics::requires_retrieval_attempt(input)
                && result.status == membrane_protocol::HookInvocationStatus::Error {
                // Module failure cannot prove an attempt completed, so it grants nothing.
                result.output = Some(status(HookModuleState::Blocked, "membrane_attempt_unconfirmed", json!({
                    "alternativeAllowed":false, "detail":"Membrane attempt did not produce a verified result; retry Membrane before alternate retrieval."
                })));
            }
            result
        }).collect();
        HookDispatchResultV1::new(input.event.clone(), results)
    }

    fn invoke(&self, id: HookModuleId, input: &HookInputEnvelopeV1) -> HookModuleResultV1 {
        if !module_event_matches(id, input) { return HookModuleResultV1::skipped(id); }
        // Every module runs on a worker thread under the same deadline, with
        // a leaf-process scope installed: when the deadline fires the scope
        // reaps any leaf helper the module started (today: git) before serial
        // dispatch advances, and the late result is discarded. Leaf helpers
        // carry their own socket/git bounds, so a module that starts no
        // process can only overrun through its own bounded calls; local
        // filesystem effects are millisecond-scale and complete or fail
        // inside the same bounds. No child process is ever spawned: this
        // dispatcher owns no runtime discovery and no storage beyond what the
        // injected service supplies.
        let scope = ContainmentScope::new();
        let (sender, receiver) = mpsc::sync_channel(1);
        let service = Arc::clone(&self.service);
        let input = input.clone();
        let enforcement_enabled = self.enforcement_enabled;
        let worker_scope = Arc::clone(&scope);
        std::thread::spawn(move || {
            let _guard = ScopeGuard::install(&worker_scope);
            // The only detached workers are this dispatch's own modules; the
            // scope above guarantees their leaf processes die with them.
            let _ = sender.send(Self::execute(&service, enforcement_enabled, id, &input));
        });
        match receiver.recv_timeout(Duration::from_millis(HOOK_MODULE_DEADLINE_MS)) {
            Ok(output) => HookModuleResultV1::ok(id, output),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                scope.kill_all();
                // Best-effort serial isolation: give the worker a short grace
                // window to observe the reaped leaves and return, then discard
                // whatever arrives late. The wait is bounded; dispatch always
                // advances.
                let _ = receiver.recv_timeout(Duration::from_millis(500));
                HookModuleResultV1::error(id, "module_deadline_exceeded")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                scope.kill_all();
                HookModuleResultV1::error(id, "module_execution_failed")
            }
        }
    }

    fn execute(service: &S, enforcement_enabled: bool, id: HookModuleId, input: &HookInputEnvelopeV1) -> HookModuleOutputV1 {
        match id {
            HookModuleId::CortexStatus if is_event(input, "SessionStart") => {
                match service.healthy(input, Duration::from_millis(HEALTH_DEADLINE_MS)) {
                    Ok(true) => status(HookModuleState::Available, "cortex_healthy", Value::Null),
                    _ => status(HookModuleState::Unavailable, "cortex_unavailable", Value::Null),
                }
            }
            HookModuleId::MemoryRearm if is_event(input, "SessionStart") => crate::hook_memory::rearm(input),
            HookModuleId::MemoryRecall if is_recall_event(input) => match service.recall_detailed(input, Duration::from_millis(HOOK_MODULE_DEADLINE_MS)) {
                Ok(outcome) => match outcome.context {
                    Some(context) if !context.is_empty() => status(HookModuleState::Available, outcome.reason, json!({"additionalContext": context, "recall": outcome.detail})),
                    _ => status(HookModuleState::Unavailable, outcome.reason, outcome.detail),
                },
                Err(error) => status(HookModuleState::Unavailable, "memory_retrieval_failed", json!({"error": error.chars().take(200).collect::<String>()})),
            },
            HookModuleId::MemoryPreCompact if is_event(input, "PreCompact") => crate::hook_memory::pre_compact(input),
            HookModuleId::MemoryPostCompact if is_event(input, "PostCompact") => crate::hook_memory::post_compact(input),
            HookModuleId::MemoryBump if is_event(input, "PreToolUse") => crate::hook_memory::bump(input),
            HookModuleId::DiagnosticsFence if is_event(input, "PreToolUse") => crate::hook_diagnostics::fence_with_recall(input, false, enforcement_enabled, |input, need| {
                let mut scoped = input.clone();
                scoped.payload["prompt"] = Value::String(need.to_owned());
                service.recall_detailed(&scoped, Duration::from_millis(HOOK_MODULE_DEADLINE_MS))
                    .unwrap_or_else(|_| RecallOutcome { context:None, sufficient:false, reason:"membrane_retrieval_failed", detail:json!({"serviceError":true}) })
            }),
            HookModuleId::MemoryConflict if is_event(input, "PreToolUse") => crate::hook_memory::conflict(input),
            HookModuleId::ToolObserver if is_event(input, "PostToolUse") => crate::hook_diagnostics::observe_tool(input),
            HookModuleId::MemoryIngest if is_event(input, "PostToolUse") => crate::hook_memory::ingest(input),
            HookModuleId::DiagnosticsObserve if is_event(input, "PostToolUse") => crate::hook_diagnostics::observe_mutation(input),
            HookModuleId::DiagnosticsCompletionFence if is_event(input, "Stop") => crate::hook_diagnostics::fence(input, true, enforcement_enabled),
            HookModuleId::MemoryNag if is_event(input, "Stop") => crate::hook_memory::nag(input),
            HookModuleId::MemoryFailure if is_event(input, "PostToolUseFailure") => crate::hook_memory::failure(input),
            HookModuleId::MemoryEpisode if is_event(input, "TaskCompleted") => crate::hook_memory::episode(input),
            HookModuleId::MemorySessionEnd if is_event(input, "SessionEnd") => status(HookModuleState::Available, "session_closed", Value::Null),
            _ => HookModuleOutputV1::skipped(),
        }
    }

}

pub fn dispatch_native_hook(input: &HookInputEnvelopeV1, enforcement_enabled: bool) -> HookDispatchResultV1 {
    NativeHookRuntime::new(NoNativeHookService, enforcement_enabled).dispatch(input)
}

/// CLI-facing native entry point. Input normalization retains raw JSON exactly;
/// malformed host input becomes a content-free typed result rather than panic.
pub fn run_hook_payload(payload: Value) -> HookHostResponseV1 {
    match normalize_hook_payload(payload) {
        Ok(input) => project_hook_host_response(dispatch_native_hook(
            &input,
            crate::hook_diagnostics::fence_enforcement_enabled(&input),
        )),
        Err(_) => project_hook_host_response(HookDispatchResultV1::new(
            HookEvent::Unknown("invalid_hook_payload".to_owned()),
            HookModuleId::ORDERED.into_iter().map(|id| HookModuleResultV1::error(id, "invalid_hook_payload")).collect(),
        )),
    }
}

fn module_event_matches(id: HookModuleId, input: &HookInputEnvelopeV1) -> bool { match id {
    HookModuleId::CortexStatus | HookModuleId::MemoryRearm => is_event(input, "SessionStart"),
    HookModuleId::MemoryRecall => is_recall_event(input),
    HookModuleId::MemoryPreCompact => is_event(input, "PreCompact"),
    HookModuleId::MemoryPostCompact => is_event(input, "PostCompact"),
    HookModuleId::MemoryBump | HookModuleId::DiagnosticsFence | HookModuleId::MemoryConflict => is_event(input, "PreToolUse"),
    HookModuleId::ToolObserver | HookModuleId::MemoryIngest | HookModuleId::DiagnosticsObserve => is_event(input, "PostToolUse"),
    HookModuleId::DiagnosticsCompletionFence | HookModuleId::MemoryNag => is_event(input, "Stop"),
    HookModuleId::MemoryFailure => is_event(input, "PostToolUseFailure"),
    HookModuleId::MemoryEpisode => is_event(input, "TaskCompleted"),
    HookModuleId::MemorySessionEnd => is_event(input, "SessionEnd"),
} }


fn status(state: HookModuleState, reason: &str, detail: Value) -> HookModuleOutputV1 { HookModuleOutputV1::status(state, reason, detail) }
fn is_event(input: &HookInputEnvelopeV1, expected: &str) -> bool { serde_json::to_value(&input.event).ok().and_then(|value| value.as_str().map(str::to_owned)).as_deref() == Some(expected) }
fn is_recall_event(input: &HookInputEnvelopeV1) -> bool { is_event(input, "SessionStart") || is_event(input, "UserPromptSubmit") }

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::normalize_hook_payload;
    use serde_json::json;
    #[test]
    fn always_emits_fixed_module_order() {
        let input = normalize_hook_payload(json!({"event":"Stop"})).unwrap();
        let result = NativeHookRuntime::new(NoNativeHookService, true).dispatch(&input);
        assert_eq!(result.results.len(), 16);
        assert_eq!(result.results[11].id, HookModuleId::DiagnosticsCompletionFence);
        assert_eq!(result.results[11].output.as_ref().unwrap().state, HookModuleState::Blocked);
    }
    #[test]
    fn episode_is_sha256_and_content_free() {
        let input = normalize_hook_payload(json!({"event":"TaskCompleted", "session_id":"x", "outcomes":["secret"]})).unwrap();
        let detail = NativeHookRuntime::new(NoNativeHookService, false).dispatch(&input).results[14].output.as_ref().unwrap().detail.clone();
        assert!(detail["outcomeDigest"].as_str().unwrap().starts_with("sha256:"));
        assert_eq!(detail["contentFree"], true);
    }
}
