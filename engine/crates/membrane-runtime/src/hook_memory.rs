//! Filesystem-only memory hook helpers.  These operations deliberately never
//! spawn a client: absent resident services are a typed, safe degradation.

use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use membrane_protocol::{HookInputEnvelopeV1, HookModuleOutputV1, HookModuleState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{CheckpointV1, MemDb, MemoryStore};

const DAY_MS: u128 = 86_400_000;
const MEMORY_INDEX_FILES: &[&str] = &["MEMORY.md", "MEMORY-archive.md", "MEMORY-cold.md"];

pub(crate) fn rearm(input: &HookInputEnvelopeV1) -> HookModuleOutputV1 {
    if string(input, "source") != Some("compact") || input.session_id.is_none() {
        return skipped("rearm_not_applicable");
    }
    let root = root(input);
    let session = safe_component(input.session_id.as_deref().unwrap_or_default());
    let database = env::var_os("CORTEX_DB").map(PathBuf::from)
        .unwrap_or_else(|| root.join("tools/.cache/memory/cortex-engine.db"));
    let seen = database.parent().unwrap_or(&root).join("recall-seen").join(format!("{session}.json"));
    let _ = fs::remove_file(seen);
    available("recall_rearmed", Value::Null)
}

pub(crate) fn pre_compact(input: &HookInputEnvelopeV1) -> HookModuleOutputV1 {
    let root = root(input);
    let path = pending_path(&root, input.session_id.as_deref());
    let snapshot = json!({
        "schema_version": 1,
        "checkpoint_id": format!("checkpoint/{}/{}", session_digest(input.session_id.as_deref()), now_ms()),
        "client": if input.payload.get("turn_id").is_some() { "codex" } else { "claude" },
        "session_id": input.session_id.as_deref().unwrap_or("missing-session"),
        "scope_id": root.to_string_lossy(),
        "created_at_ms": now_ms(),
        "trigger": string(input, "trigger").unwrap_or("unknown"),
        "transcriptRef": transcript_ref(input),
    });
    let written = path.parent().and_then(|parent| fs::create_dir_all(parent).ok())
        .and_then(|_| serde_json::to_vec(&snapshot).ok())
        .and_then(|bytes| fs::write(&path, [bytes, b"\n".to_vec()].concat()).ok())
        .is_some();
    if written { available("checkpoint_prepared", json!({"redacted": true, "contentFree": true})) }
    else { unavailable("checkpoint_prepare_failed", json!({"redacted": true, "contentFree": true})) }
}

pub(crate) fn post_compact(input: &HookInputEnvelopeV1) -> HookModuleOutputV1 {
    let root = root(input);
    let path = pending_path(&root, input.session_id.as_deref());
    if !path.exists() { return skipped("checkpoint_not_pending"); }
    let Some(mut checkpoint) = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()) else { return unavailable("checkpoint_invalid", json!({"redacted": true})); };
    checkpoint["summary"] = Value::String(redact_summary(string(input, "compact_summary").unwrap_or_default()));
    checkpoint["expires_at_ms"] = json!(now_ms() + DAY_MS);
    if serde_json::to_vec(&checkpoint).ok().and_then(|bytes| fs::write(&path, [bytes, b"\n".to_vec()].concat()).ok()).is_none() {
        return unavailable("checkpoint_save_failed", json!({"redacted": true}));
    }
    let db = env::var_os("CORTEX_DB").map(PathBuf::from)
        .unwrap_or_else(|| root.join("tools/.cache/memory/cortex-engine.db"));
    if let Some(saved) = save_pending_checkpoint(&checkpoint, &root, &db) {
        if saved { let _ = fs::remove_file(&path); return available("checkpoint_captured", json!({"continuity": true})); }
    }
    // Never create or discover a service from hook code: only an already
    // installed local store is eligible for this in-process save path.
    unavailable("continuity_service_unavailable", json!({"continuity": false, "redacted": true, "expiresInMs": DAY_MS}))
}

pub(crate) fn bump(input: &HookInputEnvelopeV1) -> HookModuleOutputV1 {
    if tool(input) != "Read" { return skipped("bump_not_applicable"); }
    let root = root(input);
    let Some(target) = durable_file(&root, tool_file(input)) else { return skipped("bump_not_applicable"); };
    let memory_root = fs::canonicalize(claude_memory_root(&root)).unwrap_or_else(|_| claude_memory_root(&root));
    if !inside(&memory_root, &target) || target.file_name().and_then(|name| name.to_str()).is_some_and(|name| MEMORY_INDEX_FILES.contains(&name)) {
        return skipped("bump_not_applicable");
    }
    let Ok(source) = fs::read_to_string(&target) else { return skipped("bump_unreadable"); };
    let Some((front, rest)) = split_frontmatter(&source) else { return skipped("bump_no_frontmatter"); };
    let today = utc_date();
    if frontmatter(&front, "last_accessed").as_deref() == Some(today.as_str()) { return skipped("bump_current"); }
    let updated = replace_frontmatter(&front, "last_accessed", &today);
    let temporary = target.with_extension("md.bumptmp");
    if fs::write(&temporary, format!("---{updated}{rest}")).is_err() || fs::rename(&temporary, &target).is_err() {
        let _ = fs::remove_file(temporary);
        return unavailable("bump_write_failed", Value::Null);
    }
    available("memory_access_bumped", Value::Null)
}

pub(crate) fn conflict(input: &HookInputEnvelopeV1) -> HookModuleOutputV1 {
    if tool(input) != "Write" { return skipped("conflict_not_applicable"); }
    let root = root(input);
    let Some(target) = durable_file(&root, tool_file(input)) else { return skipped("conflict_not_applicable"); };
    let memory_root = claude_memory_root(&root);
    let Some(content) = input.payload.pointer("/tool_input/content").and_then(Value::as_str) else { return skipped("conflict_not_applicable"); };
    if !inside(&memory_root, &target) { return skipped("conflict_not_applicable"); }
    let domain = frontmatter_from_text(content, "domain");
    let Some(domain) = domain.filter(|value| !value.is_empty()) else { return skipped("conflict_no_domain"); };
    let siblings = fs::read_dir(&memory_root).ok().into_iter().flatten().filter_map(Result::ok)
        .map(|entry| entry.path()).filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("md") && path != &target)
        .filter(|path| fs::read_to_string(path).ok().and_then(|text| frontmatter_from_text(&text, "domain")).as_deref() == Some(domain.as_str()))
        .filter_map(|path| path.file_name().and_then(|name| name.to_str()).map(str::to_owned)).take(8).collect::<Vec<_>>();
    if siblings.is_empty() { return skipped("conflict_none"); }
    let lines = siblings.iter().map(|name| format!("  - {name}")).collect::<Vec<_>>().join("\n");
    available("memory_conflict", json!({"additionalContext": format!("[HOOK:memory-conflict:advisory]\nWhy: new memory in domain '{domain}' has {} same-domain sibling(s)\nRequired: verify this is not a duplicate; consider editing existing memory\n\n{lines}", siblings.len())}))
}

pub(crate) fn ingest(input: &HookInputEnvelopeV1) -> HookModuleOutputV1 {
    let root = root(input);
    if durable_file(&root, tool_file(input)).is_none() { return skipped("ingest_not_applicable"); }
    unavailable("memory_service_unavailable", Value::Null)
}

pub(crate) fn nag(input: &HookInputEnvelopeV1) -> HookModuleOutputV1 {
    let Some(path) = string(input, "transcript_path").or_else(|| string(input, "transcriptPath")) else { return skipped("nag_not_applicable"); };
    let Ok(text) = fs::read_to_string(path) else { return skipped("nag_not_applicable"); };
    let root = root(input);
    let memory_root = fs::canonicalize(claude_memory_root(&root)).unwrap_or_else(|_| claude_memory_root(&root));
    let mut tail = text.lines().rev().take(100).collect::<Vec<_>>();
    tail.reverse();
    let entries = tail.into_iter().filter_map(|line| serde_json::from_str::<Value>(line).ok()).collect::<Vec<_>>();
    let writes = entries.iter().filter(|entry| memory_write(entry, &root, &memory_root)).count();
    if writes != 0 { return skipped("nag_not_applicable"); }
    let messages = entries.iter().filter_map(user_message).filter(|message| !trivial_reply(message) && !loaded_marker(message)).collect::<Vec<_>>();
    let signals = messages.iter().rev().take(15).filter(|message| durable_signal(message)).count();
    if signals == 0 { return skipped("nag_not_applicable"); }
    available("memory_nag", json!({"additionalContext": format!("[HOOK:memory-nag:advisory]\nWhy: {signals} durable correction/confirmation signal(s) this session, no memory written\nRequired: save a memory file before ending if this should persist")}))
}

pub(crate) fn failure(input: &HookInputEnvelopeV1) -> HookModuleOutputV1 {
    let Some(reason) = input.payload.get("error").or_else(|| input.payload.get("reason")) else { return skipped("failure_not_applicable"); };
    let summary = redact_summary(&reason.to_string());
    available("failure_observed", json!({"summaryLength": summary.len(), "contentFree": true}))
}

pub(crate) fn episode(input: &HookInputEnvelopeV1) -> HookModuleOutputV1 {
    if input.session_id.is_none() { return skipped("episode_not_applicable"); }
    let outcomes = input.payload.get("outcomes").and_then(Value::as_array).map(|items| items.iter().take(64).cloned().collect::<Vec<_>>()).unwrap_or_default();
    let digest = hex::encode(Sha256::digest(serde_json::to_vec(&outcomes).unwrap_or_default()));
    available("episode_captured", json!({"outcomeDigest": format!("sha256:{digest}"), "contentFree": true}))
}

fn available(reason: &str, detail: Value) -> HookModuleOutputV1 { HookModuleOutputV1::status(HookModuleState::Available, reason, detail) }
fn unavailable(reason: &str, detail: Value) -> HookModuleOutputV1 { HookModuleOutputV1::status(HookModuleState::Unavailable, reason, detail) }
fn skipped(reason: &str) -> HookModuleOutputV1 { HookModuleOutputV1::status(HookModuleState::Skipped, reason, Value::Null) }
fn string<'a>(input: &'a HookInputEnvelopeV1, key: &str) -> Option<&'a str> { input.payload.get(key)?.as_str() }
fn tool(input: &HookInputEnvelopeV1) -> &str { input.tool_name.as_deref().or_else(|| string(input, "tool_name")).or_else(|| string(input, "toolName")).unwrap_or("") }
fn tool_file(input: &HookInputEnvelopeV1) -> Option<&str> { input.payload.pointer("/tool_input/file_path").and_then(Value::as_str).or_else(|| input.payload.pointer("/tool_input/filePath").and_then(Value::as_str)).or_else(|| string(input, "file_path")).or_else(|| string(input, "filePath")) }
fn root(input: &HookInputEnvelopeV1) -> PathBuf { env::var_os("WORKSPACE_ROOT").map(PathBuf::from).or_else(|| string(input, "cwd").map(PathBuf::from)).or_else(|| string(input, "working_directory").map(PathBuf::from)).unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from("."))) }
fn home() -> PathBuf { env::var_os("USERPROFILE").or_else(|| env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")) }
fn claude_memory_root(root: &Path) -> PathBuf { let slug = root.to_string_lossy().replace([':', '\\', '/'], "-"); home().join(".claude/projects").join(slug).join("memory") }
fn pending_path(root: &Path, session: Option<&str>) -> PathBuf {
    let base = env::current_exe().ok().and_then(|exe| crate::service::runtime_from_exe(&exe).ok())
        .filter(|runtime| runtime.origin == "installed")
        .and_then(|runtime| runtime.db.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| root.join("tools/.cache/memory"));
    base.join("checkpoint-pending").join(format!("{}.json", session_digest(session)))
}

/// Reuse this session's redacted, expiring compaction summary as retrieval
/// query context. It is never promoted to durable knowledge by this read.
pub(crate) fn pending_recall_task(input: &HookInputEnvelopeV1) -> Option<String> {
    let session = input.session_id.as_deref()?;
    let root = root(input);
    let path = pending_path(&root, Some(session));
    if fs::metadata(&path).ok()?.len() > 128 * 1024 { return None; }
    let value: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    if value.get("session_id")?.as_str()? != session
        || value.get("scope_id")?.as_str()? != root.to_string_lossy()
        || value.get("expires_at_ms")?.as_u64()? as u128 <= now_ms() { return None; }
    value.get("summary")?.as_str().filter(|summary| !summary.trim().is_empty()).map(str::to_owned)
}
fn session_digest(session: Option<&str>) -> String { hex::encode(Sha256::digest(session.unwrap_or("missing-session").as_bytes()))[..24].to_owned() }
fn safe_component(value: &str) -> String { value.chars().map(|character| if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') { character } else { '_' }).collect() }
fn inside(parent: &Path, candidate: &Path) -> bool { candidate.strip_prefix(parent).ok().is_some_and(|relative| !relative.as_os_str().is_empty()) }
fn durable_file(root: &Path, raw: Option<&str>) -> Option<PathBuf> { let raw = raw?; let requested = Path::new(raw); let candidate = fs::canonicalize(if requested.is_absolute() { requested.to_owned() } else { root.join(requested) }).ok()?; if candidate.extension().and_then(|extension| extension.to_str()) != Some("md") { return None; } let canonical_root = fs::canonicalize(root).ok()?; let allowed = [canonical_root.join("memory"), canonical_root.join(".agent/okf"), claude_memory_root(&canonical_root)]; if allowed.iter().filter_map(|parent| fs::canonicalize(parent).ok()).any(|parent| inside(&parent, &candidate)) || (inside(&canonical_root, &candidate) && candidate.file_name().and_then(|name| name.to_str()) == Some("start-here.md")) { Some(candidate) } else { None } }
fn split_frontmatter(text: &str) -> Option<(String, String)> { let rest = text.strip_prefix("---")?; let end = rest.find("\n---")?; Some((rest[..end].to_owned(), rest[end..].to_owned())) }
fn frontmatter(front: &str, key: &str) -> Option<String> { front.lines().find_map(|line| line.split_once(':').filter(|(name, _)| name.trim() == key).map(|(_, value)| value.trim().to_owned())) }
fn frontmatter_from_text(text: &str, key: &str) -> Option<String> { split_frontmatter(text).and_then(|(front, _)| frontmatter(&front, key)) }
fn replace_frontmatter(front: &str, key: &str, value: &str) -> String { let mut found = false; let mut lines = front.lines().map(|line| if line.split_once(':').is_some_and(|(name, _)| name.trim() == key) { found = true; format!("{key}: {value}") } else { line.to_owned() }).collect::<Vec<_>>(); if !found { lines.push(format!("{key}: {value}")); } format!("\n{}\n", lines.join("\n")) }
fn transcript_ref(input: &HookInputEnvelopeV1) -> Value { input.payload.get("transcript_ref").cloned().or_else(|| input.payload.get("transcriptRef").cloned()).or_else(|| string(input, "transcript_id").map(|id| json!({"id": id, "host": string(input, "client").unwrap_or("host")}))).unwrap_or(Value::Null) }
fn redact_summary(summary: &str) -> String { summary.lines().map(|line| { let lower = line.to_ascii_lowercase(); if ["api_key", "api-key", "password", "secret", "token="].iter().any(|needle| lower.contains(needle)) { "[redacted sensitive summary line]".to_owned() } else { line.to_owned() } }).collect::<Vec<_>>().join("\n") }
fn save_pending_checkpoint(value: &Value, root: &Path, db: &Path) -> Option<bool> { if !db.is_file() { return None; } let created_at_ms = value.get("created_at_ms")?.as_u64()? as i64; let expires_at_ms = value.get("expires_at_ms")?.as_u64()? as i64; let checkpoint = CheckpointV1 { checkpoint_id: value.get("checkpoint_id")?.as_str()?.to_owned(), installation_id: env::var("MEMBRANE_INSTALLATION_ID").ok().filter(|value| !value.trim().is_empty())?, client: value.get("client")?.as_str()?.to_owned(), session_id: value.get("session_id")?.as_str()?.to_owned(), repository_id: root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace").to_owned(), worktree_rev: env::var("MEMBRANE_WORKTREE_REV").ok().filter(|value| !value.trim().is_empty())?, scope_id: root.to_string_lossy().to_string(), summary: value.get("summary")?.as_str()?.to_owned(), goal_snapshot: None, task_snapshot: None, created_at_ms, expires_at_ms, source_refs: vec![] }; MemoryStore::try_open(MemDb::open(db).ok()?).ok()?.save_checkpoint(&checkpoint).ok()?; Some(true) }
fn user_message(entry: &Value) -> Option<String> { if entry.get("type")?.as_str()? != "user" { return None; } match entry.pointer("/message/content")? { Value::String(value) => Some(value.to_owned()), Value::Array(blocks) => Some(blocks.iter().filter(|block| block.get("type").and_then(Value::as_str) == Some("text")).filter_map(|block| block.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(" ")), _ => None } }
fn memory_write(entry: &Value, root: &Path, memory_root: &Path) -> bool { if entry.get("type").and_then(Value::as_str) != Some("assistant") { return false; } entry.pointer("/message/content").and_then(Value::as_array).is_some_and(|blocks| blocks.iter().any(|block| matches!(block.get("name").and_then(Value::as_str), Some("Write") | Some("Edit")) && durable_file(root, block.pointer("/input/file_path").and_then(Value::as_str)).is_some_and(|path| inside(memory_root, &path)))) }
fn trivial_reply(message: &str) -> bool { matches!(message.trim().trim_end_matches(['.', '!']).to_ascii_lowercase().as_str(), "yes" | "ok" | "okay" | "sure" | "cool" | "thanks" | "perfect" | "great" | "nice" | "done") }
fn loaded_marker(message: &str) -> bool { ["<command-name>", "<command-message>", "<system-reminder>", "<command-output>", "Base directory for this skill:", "Contents of D:", "Contents of C:", "# claudeMd", "## Output format"].iter().any(|marker| message.get(..600).unwrap_or(message).contains(marker)) }
fn now_ms() -> u128 { SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis()).unwrap_or_default() }
fn utc_date() -> String { let days = now_ms() / DAY_MS; civil_from_days(days as i64) }
fn civil_from_days(days: i64) -> String { let z = days + 719_468; let era = if z >= 0 { z } else { z - 146_096 } / 146_097; let doe = z - era * 146_097; let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; let y = yoe + era * 400; let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); let mp = (5 * doy + 2) / 153; let d = doy - (153 * mp + 2) / 5 + 1; let m = mp + if mp < 10 { 3 } else { -9 }; format!("{:04}-{:02}-{:02}", y + if m <= 2 { 1 } else { 0 }, m, d) }
fn durable_signal(text: &str) -> bool { let lower = text.to_ascii_lowercase(); ["from now on", "never do", "never use", "never run", "never call", "never trust", "never assume", "never skip", "never revert", "never fabricate", "the right way is", "the right way to", "next time"].iter().any(|needle| lower.contains(needle)) || lower.contains("rule:") }
