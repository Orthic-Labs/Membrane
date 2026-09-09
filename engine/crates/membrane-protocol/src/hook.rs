//! Native HookHost envelope, execution result, and host-response projection.
//!
//! This module is deliberately transport-free: callers normalize JSON, execute
//! modules, and project a deterministic host response without filesystem,
//! process, or clock access here.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use thiserror::Error;

pub const HOOK_SCHEMA_VERSION: u32 = 1;
pub const HOOK_MODULE_DEADLINE_MS: u64 = 3_000;

/// Every host event supported by shipped Membrane hooks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookEvent {
    SessionStart,
    UserPromptSubmit,
    PreCompact,
    PostCompact,
    PreToolUse,
    PostToolUse,
    Stop,
    PostToolUseFailure,
    TaskCompleted,
    SessionEnd,
    /// Forward-compatible host event retained verbatim for all-skipped dispatch.
    Unknown(String),
}

impl HookEvent {
    pub fn from_host_value(value: String) -> Self {
        match value.as_str() {
            "SessionStart" => Self::SessionStart,
            "UserPromptSubmit" => Self::UserPromptSubmit,
            "PreCompact" => Self::PreCompact,
            "PostCompact" => Self::PostCompact,
            "PreToolUse" => Self::PreToolUse,
            "PostToolUse" => Self::PostToolUse,
            "Stop" => Self::Stop,
            "PostToolUseFailure" => Self::PostToolUseFailure,
            "TaskCompleted" => Self::TaskCompleted,
            "SessionEnd" => Self::SessionEnd,
            _ => Self::Unknown(value),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::PreCompact => "PreCompact",
            Self::PostCompact => "PostCompact",
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::Stop => "Stop",
            Self::PostToolUseFailure => "PostToolUseFailure",
            Self::TaskCompleted => "TaskCompleted",
            Self::SessionEnd => "SessionEnd",
            Self::Unknown(value) => value,
        }
    }

    pub const fn is_deny_boundary(&self) -> bool {
        matches!(self, Self::PreToolUse | Self::Stop)
    }
}

impl Serialize for HookEvent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for HookEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_host_value(String::deserialize(deserializer)?))
    }
}

/// One native replacement for each existing shipped JavaScript hook module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookModuleId {
    #[serde(rename = "membrane.cortex-status")]
    CortexStatus,
    #[serde(rename = "membrane.memory-rearm")]
    MemoryRearm,
    #[serde(rename = "membrane.memory-recall")]
    MemoryRecall,
    #[serde(rename = "membrane.memory-pre-compact")]
    MemoryPreCompact,
    #[serde(rename = "membrane.memory-post-compact")]
    MemoryPostCompact,
    #[serde(rename = "membrane.memory-bump")]
    MemoryBump,
    #[serde(rename = "membrane.diagnostics-fence")]
    DiagnosticsFence,
    #[serde(rename = "membrane.memory-conflict")]
    MemoryConflict,
    #[serde(rename = "membrane.tool-observer")]
    ToolObserver,
    #[serde(rename = "membrane.memory-ingest")]
    MemoryIngest,
    #[serde(rename = "membrane.diagnostics-observe")]
    DiagnosticsObserve,
    #[serde(rename = "membrane.diagnostics-completion-fence")]
    DiagnosticsCompletionFence,
    #[serde(rename = "membrane.memory-nag")]
    MemoryNag,
    #[serde(rename = "membrane.memory-failure")]
    MemoryFailure,
    #[serde(rename = "membrane.memory-episode")]
    MemoryEpisode,
    #[serde(rename = "membrane.memory-session-end")]
    MemorySessionEnd,
}

impl HookModuleId {
    pub const ORDERED: [Self; 16] = [
        Self::CortexStatus,
        Self::MemoryRearm,
        Self::MemoryRecall,
        Self::MemoryPreCompact,
        Self::MemoryPostCompact,
        Self::MemoryBump,
        Self::DiagnosticsFence,
        Self::MemoryConflict,
        Self::ToolObserver,
        Self::MemoryIngest,
        Self::DiagnosticsObserve,
        Self::DiagnosticsCompletionFence,
        Self::MemoryNag,
        Self::MemoryFailure,
        Self::MemoryEpisode,
        Self::MemorySessionEnd,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookModuleState {
    Available,
    Unavailable,
    Skipped,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookInvocationStatus {
    Ok,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookExecutionMode {
    Serial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookDispatchMetadataV1 {
    pub execution_mode: HookExecutionMode,
    pub per_module_deadline_ms: u64,
}

impl Default for HookDispatchMetadataV1 {
    fn default() -> Self {
        Self {
            execution_mode: HookExecutionMode::Serial,
            per_module_deadline_ms: HOOK_MODULE_DEADLINE_MS,
        }
    }
}

/// Normalized fields and an unmodified copy of host JSON for module consumers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookInputEnvelopeV1 {
    pub schema_version: u32,
    pub event: HookEvent,
    pub session_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_use_id: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HookNormalizationError {
    #[error("HookHost payload must be an object")]
    PayloadNotObject,
    #[error("HookHost event is required")]
    EventRequired,
}

/// Normalizes host aliases while retaining every original payload field.
pub fn normalize_hook_payload(payload: Value) -> Result<HookInputEnvelopeV1, HookNormalizationError> {
    let object = payload.as_object().ok_or(HookNormalizationError::PayloadNotObject)?;
    let event = required_string(object, &["hook_event_name", "hookEventName", "event"])
        .ok_or(HookNormalizationError::EventRequired)?;
    let event = HookEvent::from_host_value(event);
    Ok(HookInputEnvelopeV1 {
        schema_version: HOOK_SCHEMA_VERSION,
        event,
        session_id: required_string(object, &["thread_id", "session_id", "sessionId"]),
        tool_name: required_string(object, &["tool_name", "toolName"]),
        tool_use_id: required_string(object, &["tool_use_id", "toolUseId"]),
        payload,
    })
}

fn required_string(object: &serde_json::Map<String, Value>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        object.get(*name).and_then(Value::as_str).and_then(|value| {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_owned())
        })
    })
}

/// Typed module status, matching shipped `membrane.hook.status` JSON exactly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookModuleOutputV1 {
    pub schema_version: u32,
    pub kind: String,
    pub state: HookModuleState,
    pub reason: String,
    pub detail: Value,
}

impl HookModuleOutputV1 {
    pub fn status(state: HookModuleState, reason: impl Into<String>, detail: Value) -> Self {
        Self {
            schema_version: HOOK_SCHEMA_VERSION,
            kind: "membrane.hook.status".to_owned(),
            state,
            reason: reason.into(),
            detail,
        }
    }

    pub fn skipped() -> Self {
        Self::status(HookModuleState::Skipped, "event_not_applicable", Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookModuleResultV1 {
    pub id: HookModuleId,
    pub status: HookInvocationStatus,
    pub output: Option<HookModuleOutputV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl HookModuleResultV1 {
    pub fn ok(id: HookModuleId, output: HookModuleOutputV1) -> Self {
        Self { id, status: HookInvocationStatus::Ok, output: Some(output), error: None }
    }

    pub fn skipped(id: HookModuleId) -> Self {
        Self::ok(id, HookModuleOutputV1::skipped())
    }

    pub fn error(id: HookModuleId, error: impl Into<String>) -> Self {
        Self { id, status: HookInvocationStatus::Error, output: None, error: Some(error.into()) }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookDispatchResultV1 {
    pub schema_version: u32,
    pub event: HookEvent,
    pub status: HookInvocationStatus,
    pub results: Vec<HookModuleResultV1>,
}

impl HookDispatchResultV1 {
    pub fn new(event: HookEvent, results: Vec<HookModuleResultV1>) -> Self {
        let status = aggregate_hook_status(&results);
        Self { schema_version: HOOK_SCHEMA_VERSION, event, status, results }
    }
}

pub fn aggregate_hook_status(results: &[HookModuleResultV1]) -> HookInvocationStatus {
    if results.iter().any(|result| result.status == HookInvocationStatus::Error) {
        HookInvocationStatus::Error
    } else {
        HookInvocationStatus::Ok
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookHostDecision {
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPermissionDecision {
    Deny,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookSpecificOutputV1 {
    pub hook_event_name: HookEvent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<HookPermissionDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision_reason: Option<String>,
    pub additional_context: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookHostResponseV1 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<HookHostDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub hook_specific_output: HookSpecificOutputV1,
    pub membrane_hook: HookDispatchResultV1,
}

/// Projects native module results into HookHost's deterministic JSON response.
pub fn project_hook_host_response(result: HookDispatchResultV1) -> HookHostResponseV1 {
    let additional_context = result.results.iter().filter_map(|entry| {
        entry.output.as_ref().and_then(|output| output.detail.get("additionalContext"))
            .and_then(Value::as_str)
    }).filter(|context| !context.is_empty()).collect::<Vec<_>>().join("\n\n");
    let blocked = result.results.iter().find(|entry| {
        entry.output.as_ref().is_some_and(|output| output.state == HookModuleState::Blocked)
    });
    let deny = result.event.is_deny_boundary() && blocked.is_some();
    let reason = blocked.and_then(|entry| entry.output.as_ref()).and_then(|output| {
        output.detail.get("detail").and_then(Value::as_str).map(str::to_owned)
            .or_else(|| Some(output.reason.clone()))
    }).or_else(|| deny.then(|| "semantic edit fence not cleared".to_owned()));
    HookHostResponseV1 {
        decision: deny.then_some(HookHostDecision::Block),
        reason: deny.then(|| reason.clone()).flatten(),
        hook_specific_output: HookSpecificOutputV1 {
            hook_event_name: result.event.clone(),
            permission_decision: deny.then_some(HookPermissionDecision::Deny),
            permission_decision_reason: deny.then(|| reason.clone()).flatten(),
            additional_context,
        },
        membrane_hook: result,
    }
}

/// Stable canonical id for one host injection point (BM09). Distinct from
/// `HookEvent`: `Resume` and `ExplicitPull` are not host hook callbacks, and
/// `PostEdit`/`PreTool` narrow `PostToolUse`/`PreToolUse` to mutation-shaped
/// invocations so descriptors stay meaningful per injection point rather than
/// per raw host event name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookInjectionPointId {
    SessionStart,
    UserPrompt,
    PreTool,
    PostEdit,
    PostTool,
    PreCompaction,
    Resume,
    ExplicitPull,
}

impl HookInjectionPointId {
    pub const ORDERED: [Self; 8] = [
        Self::SessionStart,
        Self::UserPrompt,
        Self::PreTool,
        Self::PostEdit,
        Self::PostTool,
        Self::PreCompaction,
        Self::Resume,
        Self::ExplicitPull,
    ];

    pub const fn canonical_name(&self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::UserPrompt => "user_prompt",
            Self::PreTool => "pre_tool",
            Self::PostEdit => "post_edit",
            Self::PostTool => "post_tool",
            Self::PreCompaction => "pre_compaction",
            Self::Resume => "resume",
            Self::ExplicitPull => "explicit_pull",
        }
    }
}

/// Shared semantic descriptor for one injection point (BM09). Every field is
/// required so a descriptor missing when-not-to-use, freshness, budget,
/// dedup, suppression, or a receipt kind fails construction-site review
/// rather than shipping silently incomplete (negative controls Z17/Z18).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookInjectionPointDescriptorV1 {
    pub id: HookInjectionPointId,
    pub canonical_name: &'static str,
    /// Every host-side name this point renders as, across supported clients.
    pub host_names: &'static [&'static str],
    pub purpose: &'static str,
    pub when_to_use: &'static str,
    pub when_not_to_use: &'static str,
    pub input: &'static str,
    pub output: &'static str,
    pub cost_bound: &'static str,
    pub freshness_bound: &'static str,
    pub effect_bound: &'static str,
    pub budget: &'static str,
    pub dedup: &'static str,
    pub suppression: &'static str,
    pub receipt_kind: &'static str,
}

/// The eight injection points BM09 requires, one descriptor each, in stable
/// `HookInjectionPointId::ORDERED` order. Delivery adapts host transport only;
/// it never changes point semantics (negative control Z15).
pub fn hook_injection_point_descriptors() -> [HookInjectionPointDescriptorV1; 8] {
    use HookInjectionPointId::*;
    [
        HookInjectionPointDescriptorV1 {
            id: SessionStart, canonical_name: SessionStart.canonical_name(),
            host_names: &["SessionStart"],
            purpose: "Prime a new session with cortex health and durable orientation before the first turn.",
            when_to_use: "Once per fresh session, before any user prompt is normalized.",
            when_not_to_use: "Never on a resumed session with an intact prior context window; use Resume instead so orientation is not repeated at full cost.",
            input: "HookInputEnvelopeV1 with event=SessionStart; no tool or prompt fields populated.",
            output: "HookModuleOutputV1 status (available|unavailable) with no additionalContext beyond health/rearm detail.",
            cost_bound: "One resident health probe (<=800ms) plus rearm; no network beyond local resident.",
            freshness_bound: "Health/rearm state must reflect the resident as of this call; no cached cross-session value.",
            effect_bound: "Read-only against workspace and durable state; rearm writes only session-scoped local markers.",
            budget: "Single attempt per session start; no retry budget beyond the module deadline.",
            dedup: "One invocation per session id; a repeated SessionStart for the same session id is not re-primed.",
            suppression: "Suppressed entirely when cortex resident is unreachable; falls back to unavailable status, never a stale substitute.",
            receipt_kind: "membrane.hook.status (schemaVersion 1) recording available|unavailable and reason.",
        },
        HookInjectionPointDescriptorV1 {
            id: UserPrompt, canonical_name: UserPrompt.canonical_name(),
            host_names: &["UserPromptSubmit"],
            purpose: "Recall durable/task-relevant context before the model sees the user's prompt.",
            input: "HookInputEnvelopeV1 with event=UserPromptSubmit and the raw prompt in payload.",
            output: "HookModuleOutputV1 status with additionalContext carrying recalled text when available.",
            when_to_use: "Every user prompt submission where a resident memory/federation endpoint is configured.",
            when_not_to_use: "Never when no resident API token is installed; skip rather than fabricate recall from an unauthenticated or absent source.",
            cost_bound: "One federation call bounded by HOOK_MODULE_DEADLINE_MS (3000ms); no unbounded retrieval.",
            freshness_bound: "Recall reflects durable/document state as of the resident's current index generation, not a prior session snapshot.",
            effect_bound: "Read-only; no durable-state or workspace mutation.",
            budget: "One recall attempt per prompt submission.",
            dedup: "Not deduplicated across prompts; each submission may recall fresh context.",
            suppression: "Suppressed (unavailable) when the token file is missing/empty or recall returns no non-empty blocks.",
            receipt_kind: "membrane.hook.status carrying reason (memory_recalled|memory_unavailable) and additionalContext detail.",
        },
        HookInjectionPointDescriptorV1 {
            id: PreTool, canonical_name: PreTool.canonical_name(),
            host_names: &["PreToolUse"],
            purpose: "Fence tool invocation on unresolved durable-memory conflicts and, for verification-shaped commands, an unresolved semantic-edit reconciliation.",
            input: "HookInputEnvelopeV1 with event=PreToolUse, tool_name and tool_input populated.",
            output: "HookModuleOutputV1 status; Blocked forces a typed deny via project_hook_host_response.",
            when_to_use: "Every tool invocation that could execute a verification command or touch conflicted durable memory.",
            when_not_to_use: "Never for inspection-only commands (ls/cat/grep/git status/git diff); those are explicitly excluded so the fence does not block passive reads.",
            cost_bound: "One resident status/reconcile round trip bounded by STATUS_DEADLINE_MS/WRITE_DEADLINE_MS plus one bounded git diff (<=1500ms).",
            freshness_bound: "Reconciliation must be evaluated against the current worktree's changed-path manifest, not a cached prior manifest.",
            effect_bound: "Read-only probe plus a reconcile call; no source-file mutation performed by this point itself.",
            budget: "One fence evaluation per PreToolUse event; not retried on block.",
            dedup: "Not deduplicated; every qualifying tool call is re-evaluated since the worktree may have changed.",
            suppression: "Skipped entirely unless fence enforcement is opted in for the workspace (env var or `.agent/diagnostics-enforce.json`).",
            receipt_kind: "membrane.hook.status with boundary/repoId/worktreeId detail; Blocked additionally projects hookSpecificOutput.permissionDecision=deny.",
        },
        HookInjectionPointDescriptorV1 {
            id: PostEdit, canonical_name: PostEdit.canonical_name(),
            host_names: &["PostToolUse (Write|Edit|MultiEdit|apply_patch)"],
            purpose: "Register an observed mutation epoch against durable diagnostics so later fences see the change.",
            input: "HookInputEnvelopeV1 with event=PostToolUse restricted to mutation-shaped tool_name/tool_input.",
            output: "HookModuleOutputV1 status; detail is content-free (paths/hashes only, never file contents).",
            when_to_use: "After a Write/Edit/MultiEdit/apply_patch tool call that changed at least one in-workspace path.",
            when_not_to_use: "Never for non-mutation tools (Read, Bash without file output) and never when no changed-path candidate resolves; skip rather than register an empty epoch.",
            cost_bound: "One workspace-open plus one registerObserved resident call, each bounded by WRITE_DEADLINE_MS (1200ms).",
            freshness_bound: "Epoch parent is the resident's latestSealedEpoch as of this call; falls back to a local monotonic counter only when the resident is unreachable.",
            effect_bound: "Writes only a durable-diagnostics epoch record keyed by content hashes; never writes file contents.",
            budget: "One registration attempt per qualifying PostToolUse event.",
            dedup: "Not deduplicated; each mutation call registers its own epoch even if paths repeat.",
            suppression: "Suppressed (skipped) when the tool is not a mutation tool or no changed-path candidate is extracted.",
            receipt_kind: "membrane.hook.status with reason mutation_observed|diagnostics_register_failed and contentFree=true detail.",
        },
        HookInjectionPointDescriptorV1 {
            id: PostTool, canonical_name: PostTool.canonical_name(),
            host_names: &["PostToolUse", "PostToolUseFailure"],
            purpose: "Ingest durable-memory-relevant outcomes and emit a content-free telemetry receipt for the completed tool call.",
            input: "HookInputEnvelopeV1 with event=PostToolUse or PostToolUseFailure and tool_response present.",
            output: "HookModuleOutputV1 status; telemetry detail carries a response digest, never response content.",
            when_to_use: "Every completed or failed tool call belonging to a tracked active trace.",
            when_not_to_use: "Never for a tool other than the traced boundary (currently Bash) or when no active-trace/session id resolves; skip rather than emit an untraceable receipt.",
            cost_bound: "One authenticated telemetry POST bounded by a 1000ms deadline.",
            freshness_bound: "Receipt reflects this invocation's own response digest only; no aggregation across calls.",
            effect_bound: "Writes one telemetry event; never writes workspace or durable-memory state.",
            budget: "One telemetry attempt per qualifying tool completion.",
            dedup: "Not deduplicated; each tool completion emits its own event id derived from session+trace+time.",
            suppression: "Suppressed when no resident token is installed or no active trace resolves for the session.",
            receipt_kind: "membrane.observable-event.v1 batched telemetry event plus membrane.hook.status tool_observed|tool_observe_failed.",
        },
        HookInjectionPointDescriptorV1 {
            id: PreCompaction, canonical_name: PreCompaction.canonical_name(),
            host_names: &["PreCompact", "PostCompact"],
            purpose: "Preserve durable-memory continuity across a host context-window compaction.",
            input: "HookInputEnvelopeV1 with event=PreCompact or PostCompact.",
            output: "HookModuleOutputV1 status recording what was preserved or restored.",
            when_to_use: "Every host-initiated compaction, both immediately before and immediately after.",
            when_not_to_use: "Never outside an actual host compaction event; this point does not run speculatively on a timer.",
            cost_bound: "Local, resident-scoped read/write bounded by HOOK_MODULE_DEADLINE_MS; no network call required.",
            freshness_bound: "State captured at PreCompact must be the state actually restorable at the paired PostCompact for the same session id.",
            effect_bound: "Writes only session-scoped compaction-continuity state; no cross-session or repository mutation.",
            budget: "One preserve attempt at PreCompact, one restore attempt at PostCompact; not retried.",
            dedup: "Keyed by session id; a repeated PreCompact for the same session overwrites its own prior continuity snapshot.",
            suppression: "Skipped when no session id is present in the envelope.",
            receipt_kind: "membrane.hook.status per phase (PreCompact/PostCompact) with reason and content-free detail.",
        },
        HookInjectionPointDescriptorV1 {
            id: Resume, canonical_name: Resume.canonical_name(),
            host_names: &["SessionStart (resumed session)"],
            purpose: "Reattach an existing session to its prior durable/compaction continuity without repeating full session-start priming.",
            input: "HookInputEnvelopeV1 with event=SessionStart and a session id already known to the resident.",
            output: "HookModuleOutputV1 status indicating reattachment succeeded, or falling back to full SessionStart priming.",
            when_to_use: "A host resume of a prior session id where continuity state exists.",
            when_not_to_use: "Never for a session id the resident has no record of; that case must fall back to the ordinary SessionStart path, not fabricate continuity.",
            cost_bound: "One resident lookup bounded by HEALTH_DEADLINE_MS (800ms).",
            freshness_bound: "Reattached continuity must be the same session id's own last-written state; no cross-session substitution.",
            effect_bound: "Read-only lookup; falls back to SessionStart's own effect bounds when no continuity is found.",
            budget: "One reattachment attempt per resume; falls back to SessionStart on miss rather than retrying.",
            dedup: "Keyed by session id; a resume for an already-active session id is a no-op.",
            suppression: "Suppressed (falls back to SessionStart) when no continuity record matches the session id.",
            receipt_kind: "membrane.hook.status with reason session_resumed|session_start_fallback.",
        },
        HookInjectionPointDescriptorV1 {
            id: ExplicitPull, canonical_name: ExplicitPull.canonical_name(),
            host_names: &["explicit_client (/federate, MCP membrane_context)"],
            purpose: "Serve an explicit, caller-initiated context request outside the implicit hook lifecycle.",
            input: "An explicit request (task, repo, client, session, budget) via MCP tool call or resident /federate endpoint; never a synthesized hook payload.",
            output: "A ContextPacket/receipt pair under the frozen public V1 shapes; never a bare string.",
            when_to_use: "A caller explicitly asks for context outside session_start/user_prompt (e.g. mid-turn re-orientation, tool-initiated pull).",
            when_not_to_use: "Never as a substitute for user_prompt's automatic recall; explicit_pull is caller-initiated and must not be silently invoked by hook dispatch.",
            cost_bound: "Bounded by the caller's own maxTokens/budget parameter; no implicit unbounded retrieval.",
            freshness_bound: "Reflects the planner's current admission/freshness policy at call time, independent of any hook-cycle cache.",
            effect_bound: "Read-only against durable/document/graph state; may emit a receipt but performs no source mutation.",
            budget: "Exactly the caller-supplied budget for this one call; no cross-call accumulation.",
            dedup: "Not deduplicated by this point; the caller controls repetition.",
            suppression: "Refused with a typed rejection when scope/authority checks fail; never silently degrades to empty content.",
            receipt_kind: "membrane.context-receipt.v1 (or the equivalent MCP resource/prompt response) with material omissions recorded.",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hook_injection_point_descriptors_cover_all_eight_points_uniquely() {
        let descriptors = hook_injection_point_descriptors();
        assert_eq!(descriptors.len(), 8);
        let mut ids: Vec<_> = descriptors.iter().map(|d| d.id).collect();
        ids.sort_by_key(|id| id.canonical_name());
        ids.dedup();
        assert_eq!(ids.len(), 8, "every injection point id must be unique");
        for expected in HookInjectionPointId::ORDERED {
            assert!(descriptors.iter().any(|d| d.id == expected), "missing descriptor for {}", expected.canonical_name());
        }
    }

    #[test]
    fn hook_injection_point_descriptors_carry_when_not_to_use() {
        // Negative control Z17: a descriptor lacking when-not-to-use fails.
        for descriptor in hook_injection_point_descriptors() {
            assert!(!descriptor.when_not_to_use.trim().is_empty(), "{} missing when_not_to_use", descriptor.canonical_name);
        }
    }

    #[test]
    fn hook_injection_point_descriptors_carry_per_point_freshness_budget_dedup_suppression_receipt() {
        // Negative control Z18: a point lacking freshness/budget/dedup/suppression/receipt fails.
        for descriptor in hook_injection_point_descriptors() {
            for (field_name, value) in [
                ("freshness_bound", descriptor.freshness_bound),
                ("budget", descriptor.budget),
                ("dedup", descriptor.dedup),
                ("suppression", descriptor.suppression),
                ("receipt_kind", descriptor.receipt_kind),
                ("cost_bound", descriptor.cost_bound),
                ("effect_bound", descriptor.effect_bound),
            ] {
                assert!(!value.trim().is_empty(), "{} missing {field_name}", descriptor.canonical_name);
            }
        }
    }

    #[test]
    fn hook_injection_point_canonical_names_match_bm09_required_ids() {
        let names: Vec<&str> = hook_injection_point_descriptors().iter().map(|d| d.canonical_name).collect();
        for required in ["session_start", "user_prompt", "pre_tool", "post_edit", "post_tool", "pre_compaction", "resume", "explicit_pull"] {
            assert!(names.contains(&required), "BM09 requires coverage of {required}");
        }
    }

    #[test]
    fn normalizes_aliases_and_preserves_raw_payload() {
        let raw = json!({"hookEventName":"PreToolUse", "thread_id":" t ", "toolName":"Bash", "other": true});
        let envelope = normalize_hook_payload(raw.clone()).expect("payload normalizes");
        assert_eq!(envelope.event, HookEvent::PreToolUse);
        assert_eq!(envelope.session_id.as_deref(), Some("t"));
        assert_eq!(envelope.payload, raw);
    }

    #[test]
    fn preserves_unknown_event_for_host_parity() {
        let envelope = normalize_hook_payload(json!({"event":"OtherEvent"})).expect("payload normalizes");
        assert_eq!(envelope.event, HookEvent::Unknown("OtherEvent".into()));
        assert_eq!(serde_json::to_value(&envelope.event).expect("event serializes"), json!("OtherEvent"));
        let response = project_hook_host_response(HookDispatchResultV1::new(
            envelope.event,
            vec![HookModuleResultV1::skipped(HookModuleId::CortexStatus)],
        ));
        let value = serde_json::to_value(response).expect("response serializes");
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "OtherEvent");
        assert_eq!(value["membraneHook"]["event"], "OtherEvent");
    }

    #[test]
    fn boundary_block_projects_deny_and_context_in_order() {
        let result = HookDispatchResultV1::new(HookEvent::PreToolUse, vec![
            HookModuleResultV1::ok(HookModuleId::CortexStatus, HookModuleOutputV1::status(HookModuleState::Available, "ok", json!({"additionalContext":"first"}))),
            HookModuleResultV1::ok(HookModuleId::DiagnosticsFence, HookModuleOutputV1::status(HookModuleState::Blocked, "blocked", json!({"additionalContext":"second"}))),
        ]);
        let response = project_hook_host_response(result);
        assert_eq!(response.decision, Some(HookHostDecision::Block));
        assert_eq!(response.hook_specific_output.permission_decision, Some(HookPermissionDecision::Deny));
        assert_eq!(response.hook_specific_output.additional_context, "first\n\nsecond");
    }

    #[test]
    fn invocation_error_matches_host_status_shape() {
        let result = HookModuleResultV1::error(HookModuleId::MemoryRecall, "deadline exceeded");
        assert_eq!(result.status, HookInvocationStatus::Error);
        assert_eq!(result.output, None);
        assert_eq!(result.error.as_deref(), Some("deadline exceeded"));
        assert_eq!(aggregate_hook_status(&[result]), HookInvocationStatus::Error);
    }

    #[test]
    fn skipped_module_is_a_typed_status_output() {
        let result = HookModuleResultV1::skipped(HookModuleId::MemoryRecall);
        let output = result.output.expect("typed output");
        assert_eq!(output.schema_version, 1);
        assert_eq!(output.kind, "membrane.hook.status");
        assert_eq!(output.state, HookModuleState::Skipped);
        assert_eq!(output.reason, "event_not_applicable");
        assert_eq!(output.detail, Value::Null);
    }

    #[test]
    fn unblocked_projection_omits_deny_only_fields() {
        let response = project_hook_host_response(HookDispatchResultV1::new(
            HookEvent::UserPromptSubmit,
            vec![HookModuleResultV1::skipped(HookModuleId::MemoryRecall)],
        ));
        assert_eq!(serde_json::to_value(response).expect("response serializes"), json!({
            "hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "additionalContext": ""
            },
            "membraneHook": {
                "schemaVersion": 1,
                "event": "UserPromptSubmit",
                "status": "ok",
                "results": [{
                    "id": "membrane.memory-recall",
                    "status": "ok",
                    "output": {
                        "schemaVersion": 1,
                        "kind": "membrane.hook.status",
                        "state": "skipped",
                        "reason": "event_not_applicable",
                        "detail": null
                    }
                }]
            }
        }));
    }
}
