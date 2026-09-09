//! Native HookHost runtime. Mutating modules run in a hidden native child so a
//! deadline kills and reaps its process tree before serial dispatch advances.

use std::{io::{Read, Write}, process::{Command, Stdio}, sync::{mpsc, Arc}, thread, time::{Duration, Instant}};

use membrane_protocol::{
    normalize_hook_payload, project_hook_host_response, HookDispatchResultV1, HookEvent,
    HookHostResponseV1, HookInputEnvelopeV1, HookModuleId, HookModuleOutputV1,
    HookModuleResultV1, HookModuleState, HOOK_MODULE_DEADLINE_MS,
};
use serde_json::{json, Value};

pub const HEALTH_DEADLINE_MS: u64 = 800;
pub const TELEMETRY_DEADLINE_MS: u64 = 1_000;
pub const DIAGNOSTICS_READ_DEADLINE_MS: u64 = 800;
pub const DIAGNOSTICS_WRITE_DEADLINE_MS: u64 = 1_200;
pub const GIT_DEADLINE_MS: u64 = 1_500;
pub const GIT_OUTPUT_LIMIT_BYTES: usize = 2 * 1024 * 1024;
const MODULE_IPC_LIMIT_BYTES: usize = 2 * 1024 * 1024;

/// Native service boundary.  Implementations are supplied by the installed
/// controller; `NoNativeHookService` gives deterministic safe degradation.
pub trait NativeHookService: Send + Sync + 'static {
    fn healthy(&self, _input: &HookInputEnvelopeV1, _deadline: Duration) -> Result<bool, String> { Ok(false) }
    fn recall(&self, _input: &HookInputEnvelopeV1, _deadline: Duration) -> Result<Option<String>, String> { Ok(None) }
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
}

pub struct NativeHookRuntime<S = NoNativeHookService> { service: Arc<S>, enforcement_enabled: bool, process_containment: bool }

impl Default for NativeHookRuntime<NoNativeHookService> {
    fn default() -> Self { Self::new(NoNativeHookService, false) }
}

impl<S: NativeHookService> NativeHookRuntime<S> {
    /// Test/default supervisor: deterministic, in-process execution.
    pub fn new(service: S, enforcement_enabled: bool) -> Self { Self { service: Arc::new(service), enforcement_enabled, process_containment: false } }
    /// Production supervisor: isolate each mutating module in a killable native child.
    pub fn with_process_containment(mut self) -> Self { self.process_containment = true; self }

    /// Serial, fixed-order execution. Every module receives full raw host input
    /// through `HookInputEnvelopeV1::payload`; no secret-bearing error escapes.
    pub fn dispatch(&self, input: &HookInputEnvelopeV1) -> HookDispatchResultV1 {
        let results = HookModuleId::ORDERED.into_iter().map(|id| self.invoke(id, input)).collect();
        HookDispatchResultV1::new(input.event.clone(), results)
    }

    fn invoke(&self, id: HookModuleId, input: &HookInputEnvelopeV1) -> HookModuleResultV1 {
        if !module_event_matches(id, input) { return HookModuleResultV1::skipped(id); }
        // Only resident health/recall can block outside this process and both
        // are read-only.  Mutating modules execute synchronously below, where
        // their own socket/git limits bound every effect.  Detaching a generic
        // worker after timeout would permit a late filesystem/diagnostics write
        // and break HookHost's serial isolation invariant.
        if !matches!(id, HookModuleId::CortexStatus | HookModuleId::MemoryRecall) {
            return if self.process_containment { self.contained_mutation(id, input) } else { HookModuleResultV1::ok(id, Self::execute(self.service.as_ref(), self.enforcement_enabled, id, input)) };
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let service = Arc::clone(&self.service);
        let input = input.clone();
        let enforcement_enabled = self.enforcement_enabled;
        std::thread::spawn(move || {
            // The only detached workers are read-only resident calls; they
            // cannot mutate workspace or diagnostics state after timeout.
            let _ = sender.send(Self::execute(&service, enforcement_enabled, id, &input));
        });
        match receiver.recv_timeout(Duration::from_millis(HOOK_MODULE_DEADLINE_MS)) {
            Ok(output) => HookModuleResultV1::ok(id, output),
            Err(mpsc::RecvTimeoutError::Timeout) => HookModuleResultV1::error(id, "module_deadline_exceeded"),
            Err(mpsc::RecvTimeoutError::Disconnected) => HookModuleResultV1::error(id, "module_execution_failed"),
        }
    }

    fn contained_mutation(&self, id: HookModuleId, input: &HookInputEnvelopeV1) -> HookModuleResultV1 {
        let deadline = Instant::now() + Duration::from_millis(HOOK_MODULE_DEADLINE_MS);
        let Ok(exe) = std::env::current_exe() else { return HookModuleResultV1::error(id, "module_containment_unavailable"); };
        let Ok(payload) = serde_json::to_vec(&input.payload) else { return HookModuleResultV1::error(id, "module_containment_unavailable"); };
        if payload.len() > MODULE_IPC_LIMIT_BYTES { return HookModuleResultV1::error(id, "module_input_too_large"); }
        let command = { let mut command = Command::new(exe); command.arg("hook-module").arg("--id").arg(module_name(id)).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()); command };
        let Ok(mut process) = crate::providers::child_process::spawn_strictly_contained_command(command) else { return HookModuleResultV1::error(id, "module_containment_unavailable"); };
        let Some(mut stdin) = process.child.stdin.take() else { process.kill_tree(); return HookModuleResultV1::error(id, "module_containment_unavailable"); };
        let Some(stdout) = process.child.stdout.take() else { process.kill_tree(); return HookModuleResultV1::error(id, "module_containment_unavailable"); };
        let (written, write_done) = mpsc::sync_channel(1);
        thread::spawn(move || { let _ = written.send(stdin.write_all(&payload).is_ok()); });
        let (read, read_done) = mpsc::sync_channel(1);
        thread::spawn(move || { let mut output = Vec::new(); let outcome = stdout.take((MODULE_IPC_LIMIT_BYTES + 1) as u64).read_to_end(&mut output).ok().filter(|_| output.len() <= MODULE_IPC_LIMIT_BYTES).map(|_| output); let _ = read.send(outcome); });
        loop {
            match process.child.try_wait() {
                Ok(Some(_)) if Instant::now() < deadline => break,
                Ok(Some(_)) => { process.kill_tree(); let _ = write_done.recv_timeout(Duration::from_millis(50)); let _ = read_done.recv_timeout(Duration::from_millis(50)); return HookModuleResultV1::error(id, "module_deadline_exceeded"); },
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                _ => { process.kill_tree(); let _ = write_done.recv_timeout(Duration::from_millis(50)); let _ = read_done.recv_timeout(Duration::from_millis(50)); return HookModuleResultV1::error(id, "module_deadline_exceeded"); }
            }
        }
        // Root exit is not enough: it may have left a descendant holding the
        // stdout pipe. Kill/reap the Job/process group before waiting reader.
        process.kill_tree();
        let remaining = deadline.saturating_duration_since(Instant::now());
        let write_ok = write_done.recv_timeout(remaining).ok() == Some(true);
        let output = read_done.recv_timeout(deadline.saturating_duration_since(Instant::now())).ok().flatten();
        if !write_ok || output.is_none() { return HookModuleResultV1::error(id, if Instant::now() >= deadline { "module_deadline_exceeded" } else { "module_containment_unavailable" }); }
        serde_json::from_slice(&output.unwrap()).unwrap_or_else(|_| HookModuleResultV1::error(id, "module_containment_unavailable"))
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
            HookModuleId::MemoryRecall if is_event(input, "UserPromptSubmit") => match service.recall(input, Duration::from_millis(HOOK_MODULE_DEADLINE_MS)) {
                Ok(Some(context)) if !context.is_empty() => status(HookModuleState::Available, "memory_recalled", json!({"additionalContext": context})),
                _ => status(HookModuleState::Unavailable, "memory_unavailable", Value::Null),
            },
            HookModuleId::MemoryPreCompact if is_event(input, "PreCompact") => crate::hook_memory::pre_compact(input),
            HookModuleId::MemoryPostCompact if is_event(input, "PostCompact") => crate::hook_memory::post_compact(input),
            HookModuleId::MemoryBump if is_event(input, "PreToolUse") => crate::hook_memory::bump(input),
            HookModuleId::DiagnosticsFence if is_event(input, "PreToolUse") => crate::hook_diagnostics::fence(input, false, enforcement_enabled),
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
    NativeHookRuntime::new(NoNativeHookService, enforcement_enabled).with_process_containment().dispatch(input)
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

/// Hidden child-only entry point used by the parent HookHost containment path.
pub fn run_hook_module_payload(id: &str, payload: Value) -> HookModuleResultV1 {
    let Some(id) = module_id(id) else { return HookModuleResultV1::error(HookModuleId::CortexStatus, "invalid_hook_module"); };
    let Ok(input) = normalize_hook_payload(payload) else { return HookModuleResultV1::error(id, "invalid_hook_payload"); };
    let enforce = crate::hook_diagnostics::fence_enforcement_enabled(&input);
    HookModuleResultV1::ok(id, NativeHookRuntime::<NoNativeHookService>::execute(&NoNativeHookService, enforce, id, &input))
}

fn module_name(id: HookModuleId) -> &'static str { match id { HookModuleId::CortexStatus => "membrane.cortex-status", HookModuleId::MemoryRearm => "membrane.memory-rearm", HookModuleId::MemoryRecall => "membrane.memory-recall", HookModuleId::MemoryPreCompact => "membrane.memory-pre-compact", HookModuleId::MemoryPostCompact => "membrane.memory-post-compact", HookModuleId::MemoryBump => "membrane.memory-bump", HookModuleId::DiagnosticsFence => "membrane.diagnostics-fence", HookModuleId::MemoryConflict => "membrane.memory-conflict", HookModuleId::ToolObserver => "membrane.tool-observer", HookModuleId::MemoryIngest => "membrane.memory-ingest", HookModuleId::DiagnosticsObserve => "membrane.diagnostics-observe", HookModuleId::DiagnosticsCompletionFence => "membrane.diagnostics-completion-fence", HookModuleId::MemoryNag => "membrane.memory-nag", HookModuleId::MemoryFailure => "membrane.memory-failure", HookModuleId::MemoryEpisode => "membrane.memory-episode", HookModuleId::MemorySessionEnd => "membrane.memory-session-end" } }
fn module_id(value: &str) -> Option<HookModuleId> { HookModuleId::ORDERED.into_iter().find(|id| module_name(*id) == value) }
fn module_event_matches(id: HookModuleId, input: &HookInputEnvelopeV1) -> bool { match id {
    HookModuleId::CortexStatus | HookModuleId::MemoryRearm => is_event(input, "SessionStart"),
    HookModuleId::MemoryRecall => is_event(input, "UserPromptSubmit"),
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
