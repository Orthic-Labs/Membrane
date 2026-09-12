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
const GIT_DEADLINE_MS: u64 = 8_500;
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
const RECALL_BUDGET_MARGIN_MS: u64 = 200;

fn recall_one_shot_budget_ms(elapsed: Duration) -> u64 {
    HOOK_MODULE_DEADLINE_MS
        .saturating_sub(elapsed.as_millis() as u64)
        .saturating_sub(RECALL_BUDGET_MARGIN_MS)
        .max(10)
}

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

/// Configured attention cap for ambient hook recall (see
/// `pull::federation::hook_mode_federate`); `MEMBRANE_HOOK_RECALL_MAX_TOKENS`
/// overrides the default.
const DEFAULT_HOOK_RECALL_MAX_TOKENS: u64 = 1_024;
const STARTUP_PACKET_MAX_BYTES: usize = 8 * 1024;

pub(crate) fn resident_recall(input: &HookInputEnvelopeV1) -> Option<String> {
    let task = recall_task(input);
    resident_recall_for_task(input, &task)
}

#[derive(Clone, Debug)]
pub struct RecallOutcome {
    pub context: Option<String>,
    pub reason: &'static str,
    pub detail: Value,
    /// Only an affirmative planner sufficiency verdict fences off alternatives.
    pub sufficient: bool,
}

/// Run Membrane once for one concrete host information need.  Resident and
/// bounded one-shot are transport choices inside this attempt; neither is an
/// alternative-retrieval authorization boundary.
pub(crate) fn resident_recall_for_task(input: &HookInputEnvelopeV1, task: &str) -> Option<String> {
    recall_attempt(input, task).context
}

pub(crate) fn recall_attempt(input: &HookInputEnvelopeV1, task: &str) -> RecallOutcome {
    let started = Instant::now();
    let root = project_root(input);
    let session = input.session_id.as_deref().unwrap_or("host-native");
    let client = input.payload.get("client").and_then(Value::as_str).or_else(|| input.payload.get("client_id").and_then(Value::as_str)).unwrap_or_else(|| if input.payload.get("turn_id").is_some() { "codex" } else { "claude" });
    // Host integrations may provide either canonical camelCase or native
    // snake_case envelope fields. Parse only a validated H8 observation;
    // absence remains explicit instead of inventing a context window.
    let observed_ceiling = input.payload.get("remainingContextCeiling")
        .or_else(|| input.payload.get("remaining_context_ceiling"))
        .cloned()
        .and_then(|value| serde_json::from_value::<membrane_protocol::RemainingContextCeilingV1>(value).ok())
        .filter(|ceiling| ceiling.validate().is_ok());
    let max_tokens = env::var("MEMBRANE_HOOK_RECALL_MAX_TOKENS").ok().and_then(|value| value.parse::<u64>().ok()).filter(|value| *value > 0)
        .or_else(|| input.payload.get("max_tokens").and_then(Value::as_u64)).unwrap_or(DEFAULT_HOOK_RECALL_MAX_TOKENS) as usize;
    // Resident holder first: reuse warm services when an authenticated local
    // holder is reachable.  Failure here is not a verdict on Membrane.
    // A resident miss is only a transport miss.  The one-shot path is part of
    // this same Membrane attempt and must always be tried when needed.
    if let Some((port, token)) = resident_endpoint(&root) {
        let mut body = json!({"task": task, "repo": root, "maxTokens": max_tokens, "client": client, "session": session, "budgetPolicy": if observed_ceiling.is_some() { "host_observed" } else { "configured_cap" }, "maxWaitMs": RECALL_RESIDENT_BUDGET_MS});
        if let Some(ceiling) = observed_ceiling.as_ref() {
            body["remainingContextCeiling"] = serde_json::to_value(ceiling).unwrap_or(Value::Null);
        }
        if let Some(response) = authenticated_json_at(port, "/federate", body, &token, RECALL_RESIDENT_BUDGET_MS) {
            if response.get("packet").is_some() {
                let outcome = outcome_from_response(&response, "resident");
                return if is_session_start(input) { startup_outcome(input, outcome, &response) } else { outcome };
            }
        }
    }
    // Bounded ambient federation in this process under the configured cap,
    // with the remaining module budget as its deadline.
    match crate::pull::federation::hook_mode_federate_with_observation(task, &root, max_tokens, client, session, recall_one_shot_budget_ms(started.elapsed()), observed_ceiling) {
        Ok(response) => {
            let outcome = outcome_from_response(&response, "one_shot");
            if is_session_start(input) { startup_outcome(input, outcome, &response) } else { outcome }
        }
        Err(error) => {
            if env::var_os("MEMBRANE_HOOK_DEBUG").is_some() { eprintln!("membrane hook federate: {}", error.chars().take(400).collect::<String>()); }
            RecallOutcome { context: None, sufficient: false, reason: "membrane_retrieval_failed",
                detail: json!({"transport":"one_shot", "error":error.chars().take(200).collect::<String>()}) }
        }
    }
}

fn outcome_from_response(response: &Value, transport: &str) -> RecallOutcome {
    let packet = response.get("packet").unwrap_or(&Value::Null);
    let context = packet_text(packet).filter(|text| !text.trim().is_empty());
    let sufficiency = response.pointer("/correctiveRetrieval/sufficiency").cloned().unwrap_or(Value::Null);
    let sufficient = context.is_some() && sufficiency.get("state").and_then(Value::as_str) == Some("sufficient")
        && response.get("insufficientConfidence").is_none();
    let reason = if context.is_none() { "membrane_no_matches" }
        else if sufficient { "memory_recalled" } else { "membrane_retrieval_insufficient" };
    RecallOutcome { context, sufficient, reason, detail: json!({
        "transport": transport,
        "packetId": packet.get("id"),
        "freshness": packet.get("freshness").or_else(|| response.get("freshness")),
        "sufficiency": sufficiency,
        "requirementEvidenceMap": response.get("requirementEvidenceMap"),
        "omissions": packet.get("omissions"),
        "insufficientConfidence": response.get("insufficientConfidence"),
        "budgetPolicy": response.get("budgetPolicy"),
        "sources": packet.get("blocks").and_then(Value::as_array).into_iter().flatten()
            .map(|block| json!({"id":block.get("id"),"sourceRef":block.get("sourceRef"),"sourceHash":block.get("sourceHash")})).collect::<Vec<_>>(),
    }) }
}

fn is_session_start(input: &HookInputEnvelopeV1) -> bool {
    serde_json::to_value(&input.event).ok().and_then(|value| value.as_str().map(str::to_owned)).as_deref() == Some("SessionStart")
}

/// Convert one ordinary planner packet into the bounded startup orientation
/// surface.  The planner remains the sole source of candidates, freshness,
/// provider accounting, omissions, and Adapt output; this function only
/// selects a compact host representation and never reads or builds a graph.
fn startup_outcome(input: &HookInputEnvelopeV1, mut outcome: RecallOutcome, response: &Value) -> RecallOutcome {
    let root = project_root(input);
    let generation = response.pointer("/freshness/revision").and_then(Value::as_str).unwrap_or("unknown");
    let stale = response.pointer("/freshness/stale").and_then(Value::as_bool).unwrap_or(false);
    let indexed_at = response.pointer("/freshness/indexedAt").and_then(Value::as_str).unwrap_or("unknown");
    let providers = response.get("providerDiagnostics").and_then(Value::as_array).map(|items| {
        items.iter().filter_map(|item| {
            let name = item.get("provider").and_then(Value::as_str).or_else(|| item.get("name").and_then(Value::as_str))?;
            let status = item.get("status").and_then(Value::as_str).unwrap_or("observed");
            Some(format!("{name}={status}"))
        }).collect::<Vec<_>>().join(", ")
    }).filter(|value| !value.is_empty()).unwrap_or_else(|| "unavailable".to_owned());
    let cortex = if providers.contains("cortex=") { "available" } else if providers.contains("cortex") { "observed" } else { "unavailable" };
    let ledger = if providers.contains("ledger=") || providers.contains("rules=") || providers.contains("documents=") { "available" } else { "unavailable" };
    let adapt = response.get("adaptDelivery").map_or("unavailable", |_| "observed");
    let omissions = response.get("omissions").and_then(Value::as_array).map(|items| {
        items.iter().filter_map(|item| {
            item.get("id").and_then(Value::as_str).or_else(|| item.get("detailId").and_then(Value::as_str))
        }).take(12).collect::<Vec<_>>().join(", ")
    }).filter(|value| !value.is_empty()).unwrap_or_else(|| "none".to_owned());
    let blocks = outcome.context.take().unwrap_or_default();
    let evidence_count = response.pointer("/packet/blocks").and_then(Value::as_array).map_or(0, Vec::len);
    let freshness = if stale { "stale (admitted with age recorded)" } else { "current" };
    let text = format!(
        "Membrane startup orientation\nrepository: {}\nBlueprint: generation={} freshness={} indexed={}\nCortex: {}\nLedger: {}\nAdapt/taste: {}\nproviders: {}\nevidence blocks: {}\nomissions: {}\n{}",
        root.display(), generation, freshness, indexed_at, cortex, ledger, adapt, providers, evidence_count, omissions, blocks
    );
    let bounded = if text.len() > STARTUP_PACKET_MAX_BYTES {
        let marker = format!("\n[Membrane startup packet truncated at {} bytes]", STARTUP_PACKET_MAX_BYTES);
        let mut end = STARTUP_PACKET_MAX_BYTES.saturating_sub(marker.len());
        while end > 0 && !text.is_char_boundary(end) { end -= 1; }
        format!("{}{}", &text[..end], marker)
    } else { text };
    let has_content = !bounded.trim().is_empty();
    outcome.context = has_content.then_some(bounded.clone());
    outcome.sufficient = has_content;
    outcome.reason = if has_content { "startup_orientation_delivered" } else { "membrane_no_matches" };
    outcome.detail["startup"] = json!({
        "repository": root,
        "blueprint": {"generation": generation, "stale": stale, "indexedAt": indexed_at},
        "providers": providers,
        "cortex": cortex,
        "ledger": ledger,
        "adapt": adapt,
        "omissions": omissions,
        "byteBudget": STARTUP_PACKET_MAX_BYTES,
        "bytes": bounded.len(),
    });
    outcome
}

fn recall_task(input: &HookInputEnvelopeV1) -> String {
    let prompt = input.payload.get("prompt").or_else(|| input.payload.get("user_prompt")).or_else(|| input.payload.get("task")).and_then(Value::as_str);
    let prompt = prompt.or_else(|| is_session_start(input).then_some("startup orientation for current repository"));
    match (prompt, crate::hook_memory::pending_recall_task(input)) {
        (Some(prompt), Some(summary)) => format!("{prompt}\nSession continuity (retrieval query only): {summary}"),
        (Some(prompt), None) => prompt.to_owned(),
        (None, Some(summary)) => summary,
        (None, None) => "orient current task".to_owned(),
    }
}
pub(crate) fn recall_task_for_service(input: &HookInputEnvelopeV1) -> String { recall_task(input) }

pub(crate) fn fence(input: &HookInputEnvelopeV1, completion: bool, enforcement_enabled: bool) -> HookModuleOutputV1 {
    fence_with_recall(input, completion, enforcement_enabled, recall_attempt)
}

pub(crate) fn requires_retrieval_attempt(input: &HookInputEnvelopeV1) -> bool {
    alternative_information_need(input).is_some()
}

pub(crate) fn fence_with_recall(
    input: &HookInputEnvelopeV1, completion: bool, enforcement_enabled: bool,
    recall: impl FnOnce(&HookInputEnvelopeV1, &str) -> RecallOutcome,
) -> HookModuleOutputV1 {
    if !completion {
        if let Some(need) = alternative_information_need(input) {
            let args = input.payload.get("tool_input").or_else(|| input.payload.get("toolInput"));
            if input.tool_name.as_deref().is_none_or(str::is_empty) || args.is_none_or(Value::is_null)
                || serde_json::to_vec(args.unwrap()).map_or(true, |value| value.len() > 16_384) {
                return status(HookModuleState::Blocked, "membrane_attempt_not_started", json!({"alternativeAllowed":false,"detail":"Membrane cannot identify this information need from invalid or oversized tool arguments."}));
            }
            let outcome = recall(input, &need);
            let sufficient = outcome.sufficient;
            let reason = if sufficient { "membrane_context_delivered" } else { outcome.reason };
            return status(if sufficient { HookModuleState::Blocked } else { HookModuleState::Available }, reason, json!({
                "additionalContext": outcome.context.unwrap_or_default(),
                "detail": if sufficient { "Membrane supplied this information. Use injected context; alternate retrieval has not been authorized." } else { "Membrane attempted this exact information need & could not establish sufficiency; this operation may retrieve elsewhere." },
                "informationNeedDigest": format!("sha256:{}", sha256(need.as_bytes())),
                "sessionId": input.session_id,
                "turnId": input.payload.get("turn_id"),
                "toolUseId": input.payload.get("tool_use_id"),
                "attempted": true,
                "issuedAtUnixMs": cortex_store::time::now_millis(),
                "alternativeAllowed": !sufficient,
                "recall": outcome.detail,
            }));
        }
    }
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

/// Extract a bounded, deterministic description of a host read/search need.
/// Mutation, build, test, and install commands remain outside this retrieval
/// fence so normal engineering workflows are not mistaken for information
/// retrieval.
fn alternative_information_need(input: &HookInputEnvelopeV1) -> Option<String> {
    let tool = tool_name(input).to_ascii_lowercase();
    let ti = input.payload.get("tool_input").or_else(|| input.payload.get("toolInput")).unwrap_or(&Value::Null);
    if matches!(tool.as_str(), "read" | "grep" | "glob" | "search" | "file_search" | "exec_command" | "shell_command") {
        if matches!(tool.as_str(), "exec_command" | "shell_command") {
            let command = ti.get("command").or_else(|| ti.get("cmd")).and_then(Value::as_str).or_else(|| input.payload.get("command").and_then(Value::as_str)).unwrap_or("");
            if non_retrieval_command(command) { return None; }
            return Some(format!("{tool}: {command}"));
        }
        return Some(format!("{tool}: {}", bounded_tool_input(ti)));
    }
    // Claude sometimes reports Bash while Codex adapters report shell-like
    // names. Unknown tools do not silently bypass when their declared input
    // is recognizably a read command.
    if tool == "bash" {
        let command = command(input);
        if non_retrieval_command(command) { return None; }
        return Some(format!("bash: {command}"));
    }
    if tool.starts_with("mcp__membrane__")
        || ["write", "edit", "multiedit", "apply_patch", "delete", "move", "copy", "mkdir", "task", "build", "test", "install"].contains(&tool.as_str()) {
        return None;
    }
    // Unknown host tools are conservatively treated as information access when
    // they carry an input. They must complete a Membrane attempt before host
    // retrieval can proceed; only explicit mutation/engineering tools bypass.
    Some(format!("{}: {}", if tool.is_empty() { "unknown-tool" } else { tool.as_str() }, bounded_tool_input(ti)))
}

fn bounded_tool_input(value: &Value) -> String {
    let raw = serde_json::to_string(value).unwrap_or_else(|_| "<invalid-json>".to_owned());
    if raw.len() <= 16_384 { raw } else { format!("<sha256:{};bytes:{}>", sha256(raw.as_bytes()), raw.len()) }
}

fn non_retrieval_command(command: &str) -> bool {
    let trimmed = command.trim();
    let lower = trimmed.to_ascii_lowercase();
    let first = lower.split_whitespace().next().unwrap_or("");
    if first.is_empty() || [";", "|", "&", "`", "$(", "\n"].iter().any(|token| trimmed.contains(token)) { return false; }
    ["rm", "mv", "cp", "mkdir", "rmdir", "del", "remove-item", "set-content", "add-content", "touch", "write", "install", "build", "test"].contains(&first)
        || ["git commit", "git checkout", "git switch", "git add", "git reset", "git clean", "git push", "git pull", "git merge", "git rebase", "git restore", "git tag"].iter().any(|word| lower == *word || lower.starts_with(&format!("{word} ")))
        || ["rightkit cargo test", "rightkit cargo build", "rightkit cargo check", "rightkit cargo clippy", "rightkit cargo fmt", "cargo test", "cargo build", "cargo check", "cargo clippy", "cargo fmt", "pnpm test", "pnpm build", "pnpm install", "npm test", "npm run build", "npm install", "yarn test", "yarn build", "yarn install"].iter().any(|word| lower == *word || lower.starts_with(&format!("{word} ")))
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
    let client = match env::var("MEMBRANE_CLIENT").unwrap_or_else(|_| "codex".to_owned()) { value if value == "claude" => "claude".to_owned(), value => value };
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

    #[test]
    fn hub_off_recall_reclaims_unused_resident_budget() {
        assert_eq!(recall_one_shot_budget_ms(Duration::ZERO), HOOK_MODULE_DEADLINE_MS - RECALL_BUDGET_MARGIN_MS);
        assert_eq!(recall_one_shot_budget_ms(Duration::from_millis(RECALL_RESIDENT_BUDGET_MS)), HOOK_MODULE_DEADLINE_MS - RECALL_RESIDENT_BUDGET_MS - RECALL_BUDGET_MARGIN_MS);
    }

    #[test]
    fn retrieval_gate_requires_current_attempt_and_ignores_caller_receipts() {
        for supplied in [json!({"status":"failed"}), json!({"status":"success"}), json!({"issuer":"membrane","issuedAtUnixMs":0})] {
            let input = membrane_protocol::normalize_hook_payload(json!({
                "event":"PreToolUse","session_id":"gate-session","tool_use_id":"gate-tool",
                "tool_name":"Read","tool_input":{"file_path":"src/one.rs"},"membraneFailureReceipt":supplied,
            })).unwrap();
            let denied = fence_with_recall(&input, false, false, |_, need| {
                assert!(need.contains("src/one.rs"));
                RecallOutcome { context:Some("verified source".into()), sufficient:true, reason:"memory_recalled", detail:json!({}) }
            });
            assert_eq!(denied.state, HookModuleState::Blocked);
            assert_eq!(denied.detail["alternativeAllowed"], false);
            let allowed = fence_with_recall(&input, false, false, |_, _| RecallOutcome {
                context:None, sufficient:false, reason:"membrane_retrieval_failed", detail:json!({"reason":"provider_timeout"}),
            });
            assert_eq!(allowed.detail["attempted"], true);
            assert_eq!(allowed.detail["alternativeAllowed"], true);
            assert_eq!(allowed.detail["toolUseId"], "gate-tool");
        }
    }

    #[test]
    fn malformed_or_oversized_retrieval_cannot_authorize_without_attempt() {
        for args in [Value::Null, json!({"file_path":"x".repeat(17_000)})] {
            let input = membrane_protocol::normalize_hook_payload(json!({"event":"PreToolUse","tool_name":"Read","tool_input":args})).unwrap();
            let output = fence_with_recall(&input, false, false, |_, _| panic!("invalid need must not run fabricated query"));
            assert_eq!(output.state, HookModuleState::Blocked);
            assert_eq!(output.reason, "membrane_attempt_not_started");
        }
    }

    #[test]
    fn partial_packet_is_injected_but_not_declared_sufficient() {
        let packet = json!({"packet":{"blocks":[{"id":"source-1","text":"Useful partial context"}],"omissions":[{"reason":"timeout"}]},
            "correctiveRetrieval":{"sufficiency":{"state":"insufficient"}}});
        let outcome = outcome_from_response(&packet, "resident");
        assert_eq!(outcome.context.as_deref(), Some("Useful partial context"));
        assert!(!outcome.sufficient);
        assert_eq!(outcome.reason, "membrane_retrieval_insufficient");
    }

    #[test]
    fn startup_orientation_is_bounded_and_keeps_stale_blueprint_honest() {
        let input = membrane_protocol::normalize_hook_payload(json!({
            "event":"SessionStart", "session_id":"startup", "cwd":"workspace"
        })).unwrap();
        let response = json!({
            "packet":{"blocks":[{"text":"x".repeat(20_000)}]},
            "freshness":{"revision":"sha256:blueprint", "stale":true, "indexedAt":"blueprint:old"},
            "providerDiagnostics":[{"provider":"blueprint","status":"stale"}],
            "omissions":[{"id":"ledger:unavailable"}]
        });
        let outcome = startup_outcome(&input, outcome_from_response(&response, "one_shot"), &response);
        assert!(outcome.context.as_ref().is_some_and(|text| text.len() <= STARTUP_PACKET_MAX_BYTES));
        assert_eq!(outcome.detail["startup"]["blueprint"]["stale"], true);
        assert!(outcome.context.as_deref().is_some_and(|text| text.contains("ledger:unavailable")));
    }

    #[test]
    fn read_only_gate_rejects_mutation_commands() {
        assert!(!non_retrieval_command("rg membrane src"));
        assert!(!non_retrieval_command("git show HEAD:file"));
        assert!(non_retrieval_command("pnpm test"));
        assert!(!non_retrieval_command("cat file > out"));
        assert!(!non_retrieval_command("Get-Content tests.rs | Select-Object -First 5"));
        assert!(!non_retrieval_command("cat file; echo done"));
        assert!(non_retrieval_command("cargo test --package membrane"));
    }

    #[test]
    fn retrieval_need_is_scoped_to_tool_input() {
        let input = membrane_protocol::normalize_hook_payload(json!({
            "event":"PreToolUse", "tool_name":"Read", "tool_input":{"file_path":"src/lib.rs"}
        })).unwrap();
        assert!(alternative_information_need(&input).unwrap().contains("src/lib.rs"));
    }
}
