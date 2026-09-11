//! Native semantic-edit-fence helpers for HookHost.
//!
//! This module is deliberately dependency-closed: it talks only to the local
//! resident diagnostics HTTP surface & invokes `git` with hard limits.  A
//! missing resident never becomes permission at an opted-in boundary.

use std::{
    collections::BTreeSet,
    env,
    fs,
    io::{Read, Write},
    net::{Shutdown, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use membrane_protocol::{HookInputEnvelopeV1, HookModuleOutputV1, HookModuleState, HOOK_MODULE_DEADLINE_MS};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const STATUS_DEADLINE_MS: u64 = 800;
const WRITE_DEADLINE_MS: u64 = 1_200;
// Must stay strictly above membrane_protocol::hook::HOOK_MODULE_DEADLINE_MS
// (3_000ms). `bounded_git`'s own timeout only calls a plain `Child::kill()`
// with no job-object/process-group containment, so it cannot reap a
// detached descendant holding the piped stdout open. If this inner bound
// were shorter than (or equal to) the outer per-module deadline, the git
// subprocess would always be killed and return first, and the outer
// module-level timeout-and-reap path in membrane-runtime/src/hook.rs would
// never fire for git-based fences -- leaving leaked descendants unreaped.
// Keeping this bound longer lets the outer deadline win the race so the
// Job-tree-wide reap in the outer path remains reachable in production.
const GIT_DEADLINE_MS: u64 = 3_500;
const GIT_OUTPUT_LIMIT_BYTES: usize = 2 * 1024 * 1024;
const _GIT_DEADLINE_STAYS_BELOW_MODULE_DEADLINE_FOR_OUTER_REAP: () =
    assert!(GIT_DEADLINE_MS > HOOK_MODULE_DEADLINE_MS, "GIT_DEADLINE_MS must exceed HOOK_MODULE_DEADLINE_MS so the outer per-module timeout/reap path stays reachable for git-based fences");

pub(crate) fn resident_healthy() -> bool {
    if let Ok(exe) = env::current_exe() {
        if let Ok(runtime) = crate::service::runtime_from_exe(&exe) {
            if runtime.origin == "installed" {
                let Ok(token) = fs::read_to_string(&runtime.token) else {
                    return false;
                };
                let token = token.trim();
                if token.is_empty() {
                    return false;
                }
                return crate::installed_health::probe_installed(
                    runtime.port,
                    token,
                    Duration::from_millis(STATUS_DEADLINE_MS),
                    &runtime.token,
                )
                .is_ok_and(|response| response.status == 200);
            }
        }
    }
    diagnostics_request(None, "GET", "/health", None, STATUS_DEADLINE_MS)
        .and_then(|response| response.get("status").and_then(Value::as_u64))
        .is_some_and(|status| (200..300).contains(&status))
}

/// Mirrors shipped hook opt-in: an explicit environment value wins; otherwise
/// the workspace marker enables enforcement for this invocation.
pub(crate) fn fence_enforcement_enabled(input: &HookInputEnvelopeV1) -> bool {
    match env::var("MEMBRANE_DIAGNOSTICS_ENFORCE").ok().as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ => project_root(input).join(".agent").join("diagnostics-enforce.json").is_file(),
    }
}

/// Budget split for the UserPromptSubmit recall module (bounded by
/// `HOOK_MODULE_DEADLINE_MS`): a short resident probe, then bounded explicit
/// one-shot federation with whatever remains.  Explicit requests must never
/// fail solely because no resident holder exists (execution-lifecycle
/// boundary), so the ambient injection loop works with Hub off.
const RECALL_RESIDENT_BUDGET_MS: u64 = 600;
const RECALL_ONE_SHOT_BUDGET_MS: u64 = HOOK_MODULE_DEADLINE_MS - RECALL_RESIDENT_BUDGET_MS - 200;

/// Installed resident endpoint (port + bearer token) when this binary runs
/// from an installed root; development falls back to `MEMBRANE_API_TOKEN_FILE`
/// or the workspace cache token.  `None` means no resident credential exists,
/// which is not an error: explicit one-shot execution still runs.
fn resident_endpoint(root: &Path) -> Option<(u16, String)> {
    let installed = env::current_exe().ok().and_then(|exe| crate::service::runtime_from_exe(&exe).ok()).filter(|runtime| runtime.origin == "installed");
    let (port, token_path) = match installed {
        Some(runtime) => (runtime.port, runtime.token.clone()),
        None => (
            env::var("MEMBRANE_PORT").ok().and_then(|value| value.parse::<u16>().ok()).filter(|value| *value >= 1024).unwrap_or(47851),
            env::var_os("MEMBRANE_API_TOKEN_FILE").map(PathBuf::from).unwrap_or_else(|| root.join("tools/.cache/memory/api-token")),
        ),
    };
    token_from_file(&token_path).map(|token| (port, token))
}

fn packet_text(packet: &Value) -> Option<String> {
    let blocks = packet.get("blocks").and_then(Value::as_array).into_iter().flatten().filter_map(|block| block.get("text").and_then(Value::as_str)).collect::<Vec<_>>();
    if !blocks.is_empty() { return Some(blocks.join("\n\n")); }
    packet.get("content").and_then(Value::as_str).map(str::to_owned).filter(|value| !value.is_empty())
}

/// Host context window used to turn observed usage into a remaining ceiling.
/// Claude Code does not send capacity in hook input, but its transcript
/// records the API `usage` of every assistant turn, which is a genuine host
/// observation.  The window size is host configuration, not a Membrane
/// estimate: `MEMBRANE_HOST_CONTEXT_WINDOW_TOKENS` overrides the documented
/// Claude default.
const DEFAULT_HOST_CONTEXT_WINDOW_TOKENS: u64 = 200_000;
const TRANSCRIPT_TAIL_BYTES: u64 = 2 * 1024 * 1024;

fn unix_ms_now() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64) }

/// Derives an authentic H8 ceiling from the last main-chain assistant usage
/// record in the Claude Code transcript.  Returns `None` when there is no
/// transcript or no usage yet (a fresh session): the capability gap is then
/// reported through `memory_unavailable`, never papered over with a guess.
fn transcript_context_ceiling(input: &HookInputEnvelopeV1, session: &str, task_id: &str) -> Option<Value> {
    let path = input.payload.get("transcript_path").and_then(Value::as_str).filter(|value| !value.trim().is_empty())?;
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len > TRANSCRIPT_TAIL_BYTES { use std::io::Seek; file.seek(std::io::SeekFrom::Start(len - TRANSCRIPT_TAIL_BYTES)).ok()?; }
    let mut tail = String::new();
    file.read_to_string(&mut tail).ok()?;
    let usage_line = tail.lines().rev().find(|line| {
        line.contains("\"type\":\"assistant\"") && line.contains("\"usage\"")
            && serde_json::from_str::<Value>(line).ok().is_some_and(|record| {
                record.get("isSidechain").and_then(Value::as_bool) != Some(true)
                    && record.get("sessionId").and_then(Value::as_str).map_or(true, |id| id == session)
                    && record.pointer("/message/usage").is_some()
            })
    })?;
    let record: Value = serde_json::from_str(usage_line).ok()?;
    let usage = record.pointer("/message/usage")?;
    let field = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
    let used = field("input_tokens") + field("cache_creation_input_tokens") + field("cache_read_input_tokens") + field("output_tokens");
    let window = env::var("MEMBRANE_HOST_CONTEXT_WINDOW_TOKENS").ok().and_then(|value| value.parse::<u64>().ok()).filter(|value| *value > 0).unwrap_or(DEFAULT_HOST_CONTEXT_WINDOW_TOKENS);
    let remaining = window.saturating_sub(used);
    let observed_at = file.metadata().ok().and_then(|meta| meta.modified().ok()).and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map_or_else(unix_ms_now, |d| (d.as_millis() as u64).max(1));
    let receipt_digest = crate::digest::digest_str(usage_line);
    let now = unix_ms_now();
    Some(json!({
        "schemaVersion": membrane_protocol::host_observation::REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
        "ceilingId": format!("claude_code:{}", &receipt_digest[7..39]),
        "sessionId": session,
        "taskId": {"coverage": "complete", "value": task_id},
        "requestedAtUnixMs": now,
        "remainingTokens": {"basis": {"id": "anthropic_api_usage", "version": "1"}, "estimate": {"coverage": "complete", "value": remaining}},
        "provenanceReceipt": {"schemaVersion": membrane_protocol::host_observation::HOST_OBSERVATION_PROVENANCE_SCHEMA_VERSION, "receiptId": path, "source": "claude_code", "observedAtUnixMs": observed_at, "receiptDigest": receipt_digest}
    }))
}

pub(crate) fn resident_recall(input: &HookInputEnvelopeV1) -> Option<String> {
    let root = project_root(input);
    let task = input.payload.get("prompt").or_else(|| input.payload.get("user_prompt")).or_else(|| input.payload.get("task")).and_then(Value::as_str).unwrap_or("orient current task");
    let session = input.session_id.as_deref().unwrap_or("host-native");
    let client = input.payload.get("client").and_then(Value::as_str).unwrap_or("codex");
    let mut body = json!({"task": task, "repo": root, "maxTokens": input.payload.get("max_tokens").and_then(Value::as_u64).unwrap_or(6420), "client": client, "session": session, "anchors": input.payload.get("anchors").and_then(Value::as_str).unwrap_or("")});
    // The route binds H8 to the request task id; give the prompt a stable one.
    let task_id = format!("prompt:{}", &crate::digest::digest_str(task)[7..39]);
    body["taskId"] = json!(task_id);
    // A host-supplied ceiling wins; otherwise derive one from the host's own
    // transcript usage.  Without either, the H8 gate refuses and recall stays
    // honestly unavailable.
    match input.payload.get("remainingContextCeiling").filter(|value| !value.is_null()).cloned().or_else(|| transcript_context_ceiling(input, session, &task_id)) {
        Some(ceiling) => body["remainingContextCeiling"] = ceiling,
        None => return None,
    }
    // Resident holder first: reuse warm services when an authenticated local
    // holder is reachable.  Failure here is not a verdict on Membrane.
    if let Some((port, token)) = resident_endpoint(&root) {
        if let Some(response) = authenticated_json_at(port, "/federate", body.clone(), &token, RECALL_RESIDENT_BUDGET_MS) {
            if let Some(text) = response.get("packet").and_then(packet_text) { return Some(text); }
        }
    }
    // Bounded explicit one-shot federation in this process: same native route
    // the resident serves, with the remaining module budget as its deadline.
    body["maxWaitMs"] = json!(RECALL_ONE_SHOT_BUDGET_MS);
    let (status, response) = crate::pull::federation::native_route_response(&body.to_string());
    if env::var_os("MEMBRANE_HOOK_DEBUG").is_some() { eprintln!("membrane hook one-shot federate: status={status} body={}", &response[..response.len().min(600)]); }
    if !(200..300).contains(&status) { return None; }
    let response: Value = serde_json::from_str(&response).ok()?;
    response.get("packet").and_then(packet_text)
}

pub(crate) fn fence(input: &HookInputEnvelopeV1, completion: bool, enforcement_enabled: bool) -> HookModuleOutputV1 {
    if !enforcement_enabled {
        return status(HookModuleState::Skipped, "fence_enforcement_not_enabled", Value::Null);
    }
    if !completion && !is_fence_relevant(input) {
        return status(HookModuleState::Skipped, "fence_not_applicable", Value::Null);
    }
    let root = project_root(input);
    let (repo_id, worktree_id) = diagnostics_identity(&root);
    let boundary = if completion { "completion" } else { "test_build_boundary" };
    let blocked = |detail: String| status(HookModuleState::Blocked, "fence_not_cleared", json!({
        "repoId": repo_id, "worktreeId": worktree_id, "boundary": boundary, "detail": detail,
        "deadlineMs": STATUS_DEADLINE_MS,
    }));
    let status_response = match diagnostics_request(Some(&root), "GET", &workspace_status_path(&repo_id, &worktree_id), None, STATUS_DEADLINE_MS) {
        Some(value) => value,
        None => return blocked("semantic edit fence not cleared: workspace not open or diagnostics unavailable".into()),
    };
    let body = match response_body_ok(&status_response) {
        Some(body) => body,
        None => return blocked("semantic edit fence not cleared: workspace not open or diagnostics unavailable".into()),
    };
    let bound = match body.get("projectRoot").and_then(Value::as_str).filter(|value| !value.is_empty()) {
        Some(value) => canonical_path(Path::new(value)),
        None => return blocked("semantic edit fence not cleared: bound project root missing".into()),
    };
    if root != bound {
        return blocked(format!("semantic edit fence not cleared: project root mismatch (expected {}, got {})", bound.display(), root.display()));
    }
    let paths = match changed_paths_from_git(&root) {
        Some(paths) => paths,
        None => return blocked("current worktree manifest unavailable: fail-closed before verification or completion".into()),
    };
    let manifest = mutation_manifest(&root, paths);
    let reconcile = diagnostics_request(Some(&root), "POST", "/diagnostics/reconcile", Some(json!({
        "repoId": repo_id, "worktreeId": worktree_id,
        "manifestDigest": manifest.source_manifest_digest, "hashes": manifest.changed_file_hashes,
    })), WRITE_DEADLINE_MS);
    let classification = reconcile.as_ref().and_then(response_body_ok).and_then(|value| value.get("classification")).and_then(Value::as_str);
    if classification != Some("cleared") {
        return blocked(format!("semantic edit fence not cleared at {boundary}: current worktree reconciliation is {}", classification.unwrap_or("unavailable")));
    }
    if body.get("fenceCleared") != Some(&Value::Bool(true)) {
        return blocked("semantic edit fence not cleared: run diagnostics snapshot.await and repair before tests/builds/completion".into());
    }
    status(HookModuleState::Available, "fence_cleared", json!({"repoId": repo_id, "worktreeId": worktree_id, "boundary": boundary, "deadlineMs": STATUS_DEADLINE_MS}))
}

pub(crate) fn observe_mutation(input: &HookInputEnvelopeV1) -> HookModuleOutputV1 {
    if !mutation_tool(input) { return status(HookModuleState::Skipped, "diagnostics_not_applicable", Value::Null); }
    let root = project_root(input);
    let candidates = mutation_candidates(input, &root);
    if candidates.is_empty() { return status(HookModuleState::Skipped, "diagnostics_not_applicable", Value::Null); }
    let manifest = mutation_manifest(&root, candidates);
    let (repo_id, worktree_id) = diagnostics_identity(&root);
    let workspace_status = diagnostics_request(Some(&root), "GET", &workspace_status_path(&repo_id, &worktree_id), None, STATUS_DEADLINE_MS);
    let latest = workspace_status.as_ref().and_then(response_body_ok).and_then(|body| body.get("latestSealedEpoch")).and_then(Value::as_u64);
    let configured = env::var("MEMBRANE_DIAGNOSTICS_EPOCH").ok().and_then(|value| value.parse::<u64>().ok());
    let status_available = workspace_status.as_ref().and_then(response_body_ok).is_some();
    let (epoch, parent) = if let Some(latest) = latest {
        (latest.saturating_add(1), Some(latest))
    } else if status_available {
        configured.map_or((0, None), |configured| (configured, configured.checked_sub(1)))
    } else {
        fallback_observed_epoch(&root, &repo_id, &worktree_id, configured)
    };
    let payload = json!({
        "schemaVersion": "workspace-epoch.v1", "repoId": repo_id, "worktreeId": worktree_id,
        "epoch": epoch, "parentEpoch": parent, "sourceManifestDigest": manifest.source_manifest_digest,
        "changedPaths": manifest.changed_paths, "changedFileHashes": manifest.changed_file_hashes,
        "projectConfigDigest": digest_files(&root, &["package.json", "pyproject.toml", "Cargo.toml", "pnpm-workspace.yaml", ".agent/config.json", "blueprint.json", "membrane.json"]),
        "toolchainDigest": native_toolchain_digest(),
        "sandboxPolicyDigest": digest_files(&root, &["tools/lib/memory/runtime.json", ".agent/sandbox.json", "sandbox.json"]),
        "origin": "observed_hook",
    });
    let opened = diagnostics_request(Some(&root), "POST", "/diagnostics/workspace/open", Some(json!({"repoId": repo_id, "worktreeId": worktree_id, "projectRoot": root})), WRITE_DEADLINE_MS);
    if opened.as_ref().and_then(response_body_ok).is_none() {
        return status(HookModuleState::Unavailable, "diagnostics_register_failed", json!({"paths": manifest.changed_paths, "hashes": manifest.changed_file_hashes, "deadlineMs": WRITE_DEADLINE_MS, "contentFree": true}));
    }
    let registered = diagnostics_request(Some(&root), "POST", "/diagnostics/mutation/registerObserved", Some(json!({"repoId": repo_id, "worktreeId": worktree_id, "epoch": payload})), WRITE_DEADLINE_MS);
    if registered.as_ref().and_then(response_body_ok).is_none() {
        return status(HookModuleState::Unavailable, "diagnostics_register_failed", json!({"paths": manifest.changed_paths, "hashes": manifest.changed_file_hashes, "deadlineMs": WRITE_DEADLINE_MS, "contentFree": true}));
    }
    let primary = manifest.changed_paths.first().cloned();
    let hash = primary.as_ref().and_then(|path| manifest.changed_file_hashes.iter().find(|item| item["path"] == *path)).and_then(|item| item.get("hash")).cloned();
    status(HookModuleState::Available, "mutation_observed", json!({"path": primary, "hash": hash, "paths": manifest.changed_paths, "hashes": manifest.changed_file_hashes, "epoch": epoch, "deadlineMs": WRITE_DEADLINE_MS, "contentFree": true}))
}

/// Posts a content-free receipt to resident telemetry.  This preserves the
/// old hook's active-trace/token binding without a Node child process.
pub(crate) fn observe_tool(input: &HookInputEnvelopeV1) -> HookModuleOutputV1 {
    if tool_name(input) != "Bash" || input.session_id.is_none() { return status(HookModuleState::Skipped, "observe_not_applicable", Value::Null); }
    let root = project_root(input);
    let session = input.session_id.as_deref().unwrap_or_default();
    let trace_path = env::var_os("MEMBRANE_ACTIVE_TRACE_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("tools/.cache/memory/active-traces")).join(format!("{}.json", sha256(session.as_bytes())));
    let Some(trace) = fs::read_to_string(trace_path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).and_then(|value| value.get("trace_id").and_then(Value::as_str).map(str::to_owned)) else { return status(HookModuleState::Skipped, "observe_no_trace", Value::Null); };
    let token_path = env::var_os("MEMBRANE_API_TOKEN_FILE").map(PathBuf::from).unwrap_or_else(|| root.join("tools/.cache/memory/api-token"));
    let Some(token) = fs::read_to_string(token_path).ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty()) else { return status(HookModuleState::Unavailable, "tool_observe_failed", json!({"contentFree": true, "deadlineMs": 1000})); };
    let response_digest = sha256(serde_json::to_vec(input.payload.get("tool_response").unwrap_or(&Value::Null)).unwrap_or_default().as_slice());
    let policy = fs::read(root.join("tools/.cache/memory/active-policy.json")).unwrap_or_else(|_| b"membrane-tool-observer-v1".to_vec());
    let nonce = format!("{session}:{trace}:{}", cortex_store::time::now_millis());
    let client = match env::var("MEMBRANE_CLIENT").unwrap_or_else(|_| "codex".to_owned()) { value if value == "claude" => "claude_code".to_owned(), value => value };
    let body = json!({"events":[{"schema":"membrane.observable-event.v1","installation_id":"tool-observer","client_id":client,"session_id":session,"task_id":format!("task-{}", &sha256(format!("{session}:{trace}").as_bytes())[..24]),"turn_id":format!("turn-{trace}"),"trace_id":trace,"event_id":format!("evt-{}", &sha256(nonce.as_bytes())[..32]),"event_type":"tool_receipt","origin":"tool","content_ref_or_digest":format!("sha256:{response_digest}"),"timestamp":cortex_store::time::now_iso(),"completeness":{"observed":true,"tool":true},"policy_snapshot_digest":format!("sha256:{}",sha256(&policy))}]});
    if authenticated_json("/v1/telemetry/observable-events:batch", body, &token, 1000).is_some() { status(HookModuleState::Available, "tool_observed", json!({"contentFree": true, "deadlineMs": 1000})) } else { status(HookModuleState::Unavailable, "tool_observe_failed", json!({"contentFree": true, "deadlineMs": 1000})) }
}

pub(crate) fn is_fence_relevant(input: &HookInputEnvelopeV1) -> bool {
    let tool = tool_name(input).to_ascii_lowercase();
    if !matches!(tool.as_str(), "bash" | "shell" | "terminal" | "command" | "task") { return false; }
    let command = command(input);
    is_verification_command(command, &tool)
}

fn status(state: HookModuleState, reason: &str, detail: Value) -> HookModuleOutputV1 { HookModuleOutputV1::status(state, reason, detail) }
fn tool_name(input: &HookInputEnvelopeV1) -> &str { input.tool_name.as_deref().or_else(|| input.payload.get("tool_name").and_then(Value::as_str)).or_else(|| input.payload.get("toolName").and_then(Value::as_str)).unwrap_or("") }
fn command(input: &HookInputEnvelopeV1) -> &str { input.payload.pointer("/tool_input/command").and_then(Value::as_str).or_else(|| input.payload.pointer("/tool_input/cmd").and_then(Value::as_str)).or_else(|| input.payload.get("command").and_then(Value::as_str)).unwrap_or("") }
fn mutation_tool(input: &HookInputEnvelopeV1) -> bool { matches!(tool_name(input), "Write" | "Edit" | "MultiEdit" | "apply_patch") }
/// Mirrors shipped hook extraction: direct file fields, edit arrays, then
/// unified-diff/apply-patch headers. Paths are normalized before manifesting.
fn mutation_candidates(input: &HookInputEnvelopeV1, root: &Path) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut push = |value: Option<&str>| if let Some(value) = value.filter(|value| !value.trim().is_empty()) { candidates.push(value.trim().to_owned()); };
    let tool_input = input.payload.get("tool_input").and_then(Value::as_object);
    push(tool_input.and_then(|value| value.get("file_path")).and_then(Value::as_str));
    push(tool_input.and_then(|value| value.get("filePath")).and_then(Value::as_str));
    push(input.payload.get("file_path").and_then(Value::as_str));
    push(input.payload.get("filePath").and_then(Value::as_str));
    if let Some(edits) = tool_input.and_then(|value| value.get("edits")).and_then(Value::as_array) {
        for edit in edits { push(edit.get("file_path").and_then(Value::as_str)); push(edit.get("filePath").and_then(Value::as_str)); }
    }
    let patch = tool_input.and_then(|value| value.get("patch").or_else(|| value.get("content"))).and_then(Value::as_str).unwrap_or("");
    if tool_name(input) == "apply_patch" || ["*** Begin Patch", "diff --git", "*** Update File", "*** Add File", "*** Delete File"].iter().any(|needle| patch.contains(needle)) {
        for line in patch.lines() {
            for prefix in ["*** Update File:", "*** Add File:", "*** Delete File:"] { if let Some(value) = line.strip_prefix(prefix) { push(Some(value)); } }
            if let Some(value) = line.strip_prefix("--- a/") { push(Some(value)); }
            if let Some(value) = line.strip_prefix("+++ b/") { push(Some(value)); }
            if let Some(value) = line.strip_prefix("diff --git a/") { if let Some((left, right)) = value.split_once(" b/") { push(Some(left)); push(Some(right)); } }
        }
    }
    candidates.into_iter().filter_map(|value| normalize_changed_path(root, &value)).collect::<BTreeSet<_>>().into_iter().collect()
}

fn project_root(input: &HookInputEnvelopeV1) -> PathBuf {
    let requested = env::var_os("WORKSPACE_ROOT").map(PathBuf::from).or_else(|| input.payload.get("cwd").and_then(Value::as_str).map(PathBuf::from)).or_else(|| input.payload.get("working_directory").and_then(Value::as_str).map(PathBuf::from)).unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    canonical_path(&requested)
}
fn canonical_path(path: &Path) -> PathBuf { fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()) }
fn diagnostics_identity(root: &Path) -> (String, String) {
    let repo = env::var("MEMBRANE_DIAGNOSTICS_REPO_ID").unwrap_or_else(|_| root.file_name().and_then(|value| value.to_str()).unwrap_or("workspace").to_owned());
    let worktree = env::var("MEMBRANE_DIAGNOSTICS_WORKTREE_ID").unwrap_or_else(|_| sha256(root.to_string_lossy().as_bytes())[..16].to_owned());
    (repo, worktree)
}
fn workspace_status_path(repo_id: &str, worktree_id: &str) -> String { format!("/diagnostics/workspace/status?repoId={}&worktreeId={}", encode_component(repo_id), encode_component(worktree_id)) }
/// JavaScript `encodeURIComponent` compatibility for resident query binding.
fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => encoded.push(byte as char),
            byte => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}
fn fallback_observed_epoch(root: &Path, repo_id: &str, worktree_id: &str, configured: Option<u64>) -> (u64, Option<u64>) {
    let path = root.join("tools/.cache/diagnostics/observed-epoch.json");
    let stored = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).and_then(|value| value.get("epoch").and_then(Value::as_u64));
    let parent = stored.or(configured);
    let epoch = parent.map_or(0, |value| value.saturating_add(1));
    if let Some(directory) = path.parent() { let _ = fs::create_dir_all(directory); }
    let _ = fs::write(path, format!("{}\n", json!({"repoId": repo_id, "worktreeId": worktree_id, "epoch": epoch, "parentEpoch": parent, "updatedAt": cortex_store::time::now_iso()})));
    (epoch, parent)
}

fn is_verification_command(command: &str, tool: &str) -> bool {
    let command = command.trim();
    if command.is_empty() { return false; }
    let (parts, unbalanced) = split_segments(command);
    if unbalanced && (has_keyword(command) || has_keyword(tool)) { return true; }
    parts.iter().any(|part| !is_inspection(part) && has_keyword(part)) || has_keyword(tool)
}
fn has_keyword(value: &str) -> bool { value.split(|char: char| !char.is_ascii_alphanumeric()).any(|word| matches!(word.to_ascii_lowercase().as_str(), "test" | "tests" | "check" | "build" | "compile" | "release" | "publish" | "make" | "gradle" | "mvn")) }
fn is_inspection(value: &str) -> bool { let value = value.trim_start().to_ascii_lowercase(); ["ls", "cat", "grep", "rg"].iter().any(|prefix| value == *prefix || value.starts_with(&format!("{prefix} "))) || value == "git status" || value.starts_with("git status ") || value == "git diff" || value.starts_with("git diff ") }
fn split_segments(command: &str) -> (Vec<String>, bool) {
    let mut parts = Vec::new(); let mut part = String::new(); let mut quote = None; let mut escaped = false; let chars: Vec<char> = command.chars().collect(); let mut index = 0;
    while index < chars.len() { let character = chars[index]; if escaped { part.push(character); escaped = false; index += 1; continue; } if character == '\\' { part.push(character); escaped = true; index += 1; continue; } if let Some(active) = quote { part.push(character); if character == active { quote = None; } index += 1; continue; } if matches!(character, '\'' | '"' | '`') { quote = Some(character); part.push(character); index += 1; continue; } if matches!(character, ';' | '\n' | '|') || (character == '&' && chars.get(index + 1) == Some(&'&')) { if !part.trim().is_empty() { parts.push(part.trim().to_owned()); } part.clear(); if matches!(character, '|' | '&') && chars.get(index + 1) == Some(&character) { index += 1; } index += 1; continue; } part.push(character); index += 1; }
    if !part.trim().is_empty() { parts.push(part.trim().to_owned()); } (parts, quote.is_some() || escaped)
}

struct Manifest { changed_paths: Vec<String>, changed_file_hashes: Vec<Value>, source_manifest_digest: String }
fn changed_paths_from_git(root: &Path) -> Option<Vec<String>> {
    let mut paths = BTreeSet::new();
    for args in [["diff", "--name-only", "-z"].as_slice(), ["diff", "--cached", "--name-only", "-z"].as_slice(), ["ls-files", "--others", "--exclude-standard", "-z"].as_slice()] {
        let output = bounded_git(root, args)?;
        for candidate in output.split(|byte| *byte == 0).filter_map(|item| std::str::from_utf8(item).ok()).filter_map(|item| normalize_changed_path(root, item)) { paths.insert(candidate); }
    }
    Some(paths.into_iter().collect())
}
fn bounded_git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let mut child = Command::new("git").args(args).current_dir(root).stdin(Stdio::null()).stderr(Stdio::null()).stdout(Stdio::piped()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || { let mut bytes = Vec::new(); let mut limited = stdout.by_ref().take((GIT_OUTPUT_LIMIT_BYTES + 1) as u64); limited.read_to_end(&mut bytes).ok().filter(|_| bytes.len() <= GIT_OUTPUT_LIMIT_BYTES).map(|_| bytes) });
    let deadline = Instant::now() + Duration::from_millis(GIT_DEADLINE_MS);
    loop { match child.try_wait() { Ok(Some(status)) if status.success() => break, Ok(Some(_)) | Err(_) => { let _ = child.kill(); let _ = reader.join(); return None; }, Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)), Ok(None) => { let _ = child.kill(); let _ = child.wait(); let _ = reader.join(); return None; } } }
    reader.join().ok().flatten()
}
fn normalize_changed_path(_root: &Path, candidate: &str) -> Option<String> {
    let raw = candidate.trim();
    if raw.is_empty() || Path::new(raw).is_absolute() { return None; }
    let cleaned = raw.replace('\\', "/").trim_start_matches("./").to_owned();
    if cleaned.is_empty() || cleaned.starts_with('/') || cleaned.split('/').any(|part| part == "..") { return None; }
    Some(cleaned)
}
fn mutation_manifest(root: &Path, paths: Vec<String>) -> Manifest { let paths: Vec<String> = paths.into_iter().filter_map(|path| normalize_changed_path(root, &path)).collect::<BTreeSet<_>>().into_iter().collect(); let mut digest = Sha256::new(); let mut hashes = Vec::new(); for path in &paths { let hash = fs::read(root.join(path)).ok().map(|bytes| format!("sha256:{}", sha256(&bytes))); if let Some(hash) = &hash { hashes.push(json!({"path": path, "hash": hash})); } digest.update(path.as_bytes()); digest.update([0]); digest.update(hash.as_deref().unwrap_or("missing").as_bytes()); digest.update([0]); } Manifest { changed_paths: paths, changed_file_hashes: hashes, source_manifest_digest: format!("sha256:{}", hex::encode(digest.finalize())) } }
fn digest_files(root: &Path, files: &[&str]) -> String { let mut digest = Sha256::new(); let mut found = false; for file in files { if let Ok(bytes) = fs::read(root.join(file)) { digest.update(file.as_bytes()); digest.update([0]); digest.update(bytes); digest.update([0]); found = true; } } if !found { digest.update(b"empty"); } format!("sha256:{}", hex::encode(digest.finalize())) }
fn digest_text(value: &str) -> String { format!("sha256:{}", sha256(value.as_bytes())) }
/// Native replacement for legacy `${process.version}\0${process.platform}\0${process.arch}`.
/// Product version is Membrane's installed runtime version, not a host Node version.
fn native_toolchain_digest() -> String { digest_text(&format!("{}\0{}\0{}", env!("CARGO_PKG_VERSION"), env::consts::OS, env::consts::ARCH)) }
fn sha256(value: &[u8]) -> String { hex::encode(Sha256::digest(value)) }

fn diagnostics_request(root: Option<&Path>, method: &str, path: &str, body: Option<Value>, deadline_ms: u64) -> Option<Value> {
    let port = env::var("MEMBRANE_PORT").ok().and_then(|value| value.parse::<u16>().ok()).filter(|value| *value >= 1024).unwrap_or(47851);
    let timeout = Duration::from_millis(deadline_ms);
    let mut stream = TcpStream::connect_timeout(&format!("127.0.0.1:{port}").parse().ok()?, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?; stream.set_write_timeout(Some(timeout)).ok()?;
    let bytes = body.map(|body| serde_json::to_vec(&body).ok()).flatten().unwrap_or_default();
    let bearer = resident_bearer(root)?;
    let authorization = bearer.map(|token| format!("Authorization: Bearer {token}\r\n")).unwrap_or_default();
    let request = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n{authorization}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n", bytes.len());
    stream.write_all(request.as_bytes()).ok()?; if !bytes.is_empty() { stream.write_all(&bytes).ok()?; } let _ = stream.shutdown(Shutdown::Write);
    let mut response = Vec::new(); stream.take(4 * 1024 * 1024).read_to_end(&mut response).ok()?;
    let split = response.windows(4).position(|window| window == b"\r\n\r\n")? + 4;
    let status = std::str::from_utf8(&response[..split]).ok()?.split_whitespace().nth(1)?.parse::<u16>().ok()?;
    Some(json!({"status": status, "body": serde_json::from_slice::<Value>(&response[split..]).ok()?}))
}
/// Installed hook binding: explicit resident token wins; otherwise use its
/// canonical configured token file, then the workspace-local installed token.
fn resident_bearer(root: Option<&Path>) -> Option<Option<String>> {
    if let Ok(token) = env::var("MEMBRANE_RESIDENT_TOKEN") {
        if !token.is_empty() { return header_safe_token(token).map(Some); }
    }
    let Some(root) = root else { return Some(None); };
    match env::var_os("MEMBRANE_API_TOKEN_FILE").map(PathBuf::from) {
        Some(path) => resident_bearer_from_file(&path),
        None => resident_bearer_from_file(&root.join("tools/.cache/memory/api-token")),
    }
}
fn resident_bearer_from_file(path: &Path) -> Option<Option<String>> {
    match fs::read_to_string(path) {
        Ok(token) => header_safe_token(token.trim().to_owned()).map(Some),
        Err(_) => Some(None),
    }
}
fn token_from_file(path: &Path) -> Option<String> { fs::read_to_string(path).ok().and_then(|token| header_safe_token(token.trim().to_owned())) }
fn header_safe_token(token: String) -> Option<String> { (!token.is_empty() && !token.contains('\r') && !token.contains('\n')).then_some(token) }
fn authenticated_json(path: &str, body: Value, token: &str, deadline_ms: u64) -> Option<Value> {
    let port = env::var("MEMBRANE_PORT").ok().and_then(|value| value.parse::<u16>().ok()).filter(|value| *value >= 1024).unwrap_or(47851);
    authenticated_json_at(port, path, body, token, deadline_ms)
}

fn authenticated_json_at(port: u16, path: &str, body: Value, token: &str, deadline_ms: u64) -> Option<Value> {
    if token.contains('\r') || token.contains('\n') { return None; }
    let timeout = Duration::from_millis(deadline_ms); let Ok(address) = format!("127.0.0.1:{port}").parse() else { return None; };
    let Ok(mut stream) = TcpStream::connect_timeout(&address, timeout) else { return None; };
    if stream.set_read_timeout(Some(timeout)).is_err() || stream.set_write_timeout(Some(timeout)).is_err() { return None; }
    let Ok(bytes) = serde_json::to_vec(&body) else { return None; };
    let request = format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", bytes.len());
    if stream.write_all(request.as_bytes()).is_err() || stream.write_all(&bytes).is_err() { return None; }
    let _ = stream.shutdown(Shutdown::Write); let mut response = Vec::new(); stream.take(2 * 1024 * 1024).read_to_end(&mut response).ok()?;
    let split = response.windows(4).position(|window| window == b"\r\n\r\n")? + 4;
    let status = std::str::from_utf8(&response[..split]).ok()?.split_whitespace().nth(1)?.parse::<u16>().ok()?;
    ((200..300).contains(&status)).then(|| serde_json::from_slice(&response[split..]).ok()).flatten()
}
fn response_body_ok(response: &Value) -> Option<&Value> { (response.get("status")?.as_u64()? >= 200 && response.get("status")?.as_u64()? < 300).then(|| response.get("body")).flatten() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_ceiling_uses_last_main_chain_usage_and_binds_task() {
        let dir = std::env::temp_dir().join(format!("membrane-hook-h8-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        fs::write(&path, concat!(
            "{\"type\":\"user\",\"sessionId\":\"s1\"}
",
            "{\"type\":\"assistant\",\"isSidechain\":false,\"sessionId\":\"s1\",\"message\":{\"usage\":{\"input_tokens\":10,\"cache_creation_input_tokens\":20,\"cache_read_input_tokens\":1000,\"output_tokens\":70}}}
",
            "{\"type\":\"assistant\",\"isSidechain\":true,\"sessionId\":\"s1\",\"message\":{\"usage\":{\"input_tokens\":1,\"cache_creation_input_tokens\":1,\"cache_read_input_tokens\":1,\"output_tokens\":1}}}
",
        )).unwrap();
        let input = membrane_protocol::normalize_hook_payload(json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","transcript_path":path.to_string_lossy(),"prompt":"x"})).unwrap();
        let ceiling = transcript_context_ceiling(&input, "s1", "prompt:abc").expect("ceiling");
        let parsed: membrane_protocol::host_observation::RemainingContextCeilingV1 = serde_json::from_value(ceiling.clone()).expect("typed ceiling");
        parsed.validate().expect("valid");
        assert_eq!(parsed.remaining_tokens.estimate.value, Some(DEFAULT_HOST_CONTEXT_WINDOW_TOKENS - 1100));
        assert_eq!(parsed.task_id.value.as_deref(), Some("prompt:abc"));
        assert_eq!(parsed.provenance_receipt.source, "claude_code");
        let no_transcript = membrane_protocol::normalize_hook_payload(json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt":"x"})).unwrap();
        assert!(transcript_context_ceiling(&no_transcript, "s1", "prompt:abc").is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn query_encoding_matches_encode_uri_component_safe_set() {
        assert_eq!(encode_component("a b/?&=✓!~*'()"), "a%20b%2F%3F%26%3D%E2%9C%93!~*'()");
    }

    #[test]
    fn changed_paths_never_escape_workspace() {
        let root = Path::new("workspace");
        assert_eq!(normalize_changed_path(root, "src/lib.rs").as_deref(), Some("src/lib.rs"));
        assert_eq!(normalize_changed_path(root, "../outside.rs"), None);
        assert_eq!(normalize_changed_path(root, "src/../../outside.rs"), None);
        assert_eq!(normalize_changed_path(root, "/outside.rs"), None);
    }

    #[test]
    fn native_toolchain_identity_is_product_version_os_and_arch() {
        assert_eq!(native_toolchain_digest(), digest_text(&format!("{}\0{}\0{}", env!("CARGO_PKG_VERSION"), env::consts::OS, env::consts::ARCH)));
    }

    #[test]
    fn installed_token_file_fallback_is_loaded_without_echo() {
        let root = tempfile::tempdir().expect("temporary root");
        let token = root.path().join("api-token");
        fs::write(&token, "installed-token\n").expect("token file");
        assert!(token_from_file(&token).is_some());
    }
}
