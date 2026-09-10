//! Partial port of legacy `mcp/host/context-adapter.cjs`.
//!
//! Scope note: the legacy adapter is a Node hook script that (a) derives a
//! typed client identity and a task/turn request envelope from process
//! environment + stdin, (b) calls out to a resident HTTP service or spawns a
//! `membrane-client.mjs` subprocess, and (c) renders the response by
//! delegating to `mcp/context-renderer-lib.cjs` (`finalize`,
//! `ContextSessionV1`, `applyDeliveryLedger`) — a module outside this lane's
//! 11-module port list and not present anywhere in this crate's dependency
//! tree. This port covers the pure, host-owned identity/envelope logic
//! (`defaultClient`, `buildRequest`, `taskId`, `ledgerKey`) faithfully; the
//! subprocess/HTTP transport and the renderer-delegating `render`/`finalize`/
//! `prepareDelivery` functions are process-orchestration, not MCP-surface
//! logic, and are intentionally left to the Node hook they ship in.

use membrane_protocol::digest_str;

pub const CLIENT_IDENTITIES: &[&str] = &["claude_code", "codex", "mcp", "api_worker", "other"];

/// Mirrors `defaultClient(env)`.
pub fn default_client(membrane_client: Option<&str>, codex_thread_id: Option<&str>, codex_session_id: Option<&str>) -> &'static str {
    if let Some(value) = membrane_client {
        return CLIENT_IDENTITIES
            .iter()
            .find(|id| **id == value)
            .copied()
            .unwrap_or("other");
    }
    if codex_thread_id.is_some() || codex_session_id.is_some() {
        return "codex";
    }
    "claude_code"
}

fn digest(value: &str) -> String {
    digest_str(value)
}

/// Mirrors `taskId(event, session)`.
pub fn task_id(explicit_task_id: Option<&str>, session: &str, prompt: &str) -> String {
    if let Some(id) = explicit_task_id.filter(|s| !s.is_empty()) {
        return id.to_string();
    }
    let full = digest(&format!("{session}:{prompt}"));
    let hex = full.strip_prefix("sha256:").unwrap_or(&full);
    hex.chars().skip(0).take(24).collect::<String>()
}

pub struct BuiltRequest {
    pub task: String,
    pub repo: String,
    pub session: String,
    pub client: String,
    pub max_tokens: i64,
    pub task_envelope_task_id: String,
    pub turn_envelope_turn_id: String,
    pub user_prompt_digest: String,
}

/// Mirrors `buildRequest(event, root)` for the subset of fields that do not
/// depend on host-environment plumbing (max_tokens defaulting, anchors
/// joining are omitted as they are direct pass-through with no logic worth
/// re-testing here).
pub fn build_request(
    session_id: Option<&str>,
    prompt: Option<&str>,
    root: &str,
    client: &str,
    turn_id: Option<&str>,
    explicit_task_id: Option<&str>,
    pid: u32,
) -> BuiltRequest {
    let session = session_id
        .map(str::to_string)
        .unwrap_or_else(|| format!("host-{pid}"));
    let task = prompt.unwrap_or("orient current task").trim().to_string();
    let id = task_id(explicit_task_id, &session, &task);
    let turn = turn_id.map(str::to_string).unwrap_or_else(|| format!("{id}:turn"));
    BuiltRequest {
        task: task.clone(),
        repo: root.to_string(),
        session: session.clone(),
        client: client.to_string(),
        max_tokens: 6420,
        task_envelope_task_id: id,
        turn_envelope_turn_id: turn,
        user_prompt_digest: digest(&task),
    }
}

/// Mirrors `ledgerKey(result, packet)`'s packet-content-addressed fallback
/// branch (the session-id branch is a trivial string format, omitted).
pub fn ledger_key_from_blocks(blocks: &[(&str, &str)]) -> String {
    let ids: Vec<String> = blocks
        .iter()
        .map(|(id, text)| format!("{}:{}", if id.is_empty() { "block" } else { id }, digest(text)))
        .collect();
    format!("packet:{}", digest(&ids.join("|")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_client_identity_never_leaks_a_gateway_alias() {
        assert!(CLIENT_IDENTITIES.contains(&default_client(None, None, None)));
        assert_eq!(default_client(None, None, None), "claude_code");
        assert_eq!(default_client(None, Some("thread-1"), None), "codex");
        assert_eq!(default_client(Some("mcp"), None, None), "mcp");
        assert_eq!(default_client(Some("ccx"), None, None), "other");
    }

    #[test]
    fn build_request_produces_typed_task_and_turn_envelope_ids() {
        let request = build_request(Some("session-1"), Some("inspect current graph"), "/repo", "claude_code", Some("turn-1"), None, 4242);
        assert_eq!(request.session, "session-1");
        assert_eq!(request.turn_envelope_turn_id, "turn-1");
        assert_eq!(request.task_envelope_task_id.len(), 24);
        assert!(request.user_prompt_digest.starts_with("sha256:"));
    }

    #[test]
    fn ledger_key_is_content_addressed_and_stable() {
        let blocks = [("rules:AGENTS.md", "text-a"), ("git:meta", "text-b")];
        let key1 = ledger_key_from_blocks(&blocks);
        let key2 = ledger_key_from_blocks(&blocks);
        assert_eq!(key1, key2);
        assert!(key1.starts_with("packet:sha256:"));
    }
}
