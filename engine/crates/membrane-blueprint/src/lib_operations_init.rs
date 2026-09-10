//! Native `Operation::Init` handler (lane V1, r5 closure).
//!
//! Composes the previously-ported pure primitives in
//! `lib_init_detect_hosts`, `lib_init_plan`, `lib_init_apply`, and
//! `lib_init_state_integrity` into one operation, mirroring
//! `blueprint/scripts/cli/commands.mjs` case `"init"` (`buildInitPlan` then,
//! absent `--dry-run`, `applyInitPlan`).
//!
//! SCOPE: as documented on the primitive modules this composes, the legacy
//! `applyInitPlan` also spawns a child `blueprint graph build` and enrolls a
//! filesystem watcher. Those two `PlanAction`s (`build-generation`,
//! `enroll-watch`) and `install-hooks` are OS-process orchestration with no
//! pure-function contract; this handler reports them as deferred actions
//! with a typed omission rather than silently dropping or fabricating them,
//! per the Membrane rule to record material omissions in receipts. The
//! file-edit actions (host instruction files, `.mcp.json`) are applied for
//! real, with full install-state recording so `uninstall` can restore them.

use crate::api::{BlueprintError, BlueprintRequest};
use crate::lib_init_apply::{sha256_hex, FileState, InstallState};
use crate::lib_init_plan::{build_init_plan, BuildInitPlanInput, InitPlanError, PlanFile};
use crate::lib_init_state_integrity::seal_local_state;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;

fn str_field<'a>(input: &'a Value, key: &str, default: &'a str) -> &'a str {
    input.get(key).and_then(Value::as_str).unwrap_or(default)
}

fn plan_to_json(plan: &crate::lib_init_plan::InitPlan) -> Value {
    json!({
        "schemaVersion": plan.schema_version,
        "root": plan.root.to_string_lossy(),
        "hosts": plan.hosts,
        "scope": plan.scope,
        "policy": plan.policy,
        "mcpEnabled": plan.mcp_enabled,
        "watchEnabled": plan.watch_enabled,
        "actions": plan.actions.iter().map(|a| json!({
            "id": a.id,
            "kind": a.kind,
            "path": a.path.as_ref().map(|p| p.to_string_lossy().to_string()),
            "reversible": a.reversible,
        })).collect::<Vec<_>>(),
        "files": plan.files.iter().map(|f| json!({
            "path": f.path.to_string_lossy(),
            "host": f.host,
        })).collect::<Vec<_>>(),
        "uninstallCommand": plan.uninstall_command,
    })
}

fn install_state_path(root: &Path) -> std::path::PathBuf {
    root.join(".agent").join("graph").join("blueprint-install-state.json")
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_base64(bytes: &[u8]) -> String {
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let value = (b0 << 16) | (b1 << 8) | b2;
        output.push(BASE64[((value >> 18) & 63) as usize] as char);
        output.push(BASE64[((value >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 { BASE64[((value >> 6) & 63) as usize] as char } else { '=' });
        output.push(if chunk.len() > 2 { BASE64[(value & 63) as usize] as char } else { '=' });
    }
    output
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let clean: Vec<u8> = input.bytes().filter(|byte| *byte != b'\r' && *byte != b'\n').collect();
    if clean.len() % 4 != 0 { return None; }
    let mut output = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let mut value = 0u32;
        let mut padding = 0usize;
        for byte in chunk {
            if *byte == b'=' { value <<= 6; padding += 1; }
            else {
                let digit = BASE64.iter().position(|candidate| candidate == byte)? as u32;
                value = (value << 6) | digit;
            }
        }
        output.push((value >> 16) as u8);
        if padding < 2 { output.push((value >> 8) as u8); }
        if padding == 0 { output.push(value as u8); }
    }
    Some(output)
}

fn load_install_state(root: &Path) -> std::io::Result<InstallState> {
    let path = install_state_path(root);
    if !path.exists() {
        return Ok(InstallState { version: 1, files: Default::default() });
    }
    let raw: Value = serde_json::from_str(&fs::read_to_string(path)?)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    let version = raw.get("version").and_then(Value::as_u64).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "install state is invalid")
    })? as u32;
    let files = raw.get("files").and_then(Value::as_object).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "install state is invalid")
    })?;
    let mut state = InstallState { version, files: Default::default() };
    for (path, entry) in files {
        let object = entry.as_object().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "install state is invalid")
        })?;
        let exists = object.get("exists").and_then(Value::as_bool).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "install state is invalid")
        })?;
        let content = object
            .get("bytes")
            .and_then(Value::as_str)
            .and_then(decode_base64)
            .or_else(|| object
            .get("content")
            .and_then(Value::as_str)
            .map(|value| value.as_bytes().to_vec()));
        let installed = object.get("installed").and_then(Value::as_str).map(str::to_owned);
        state.files.insert(path.clone(), FileState { exists, content, installed });
    }
    crate::lib_init_apply::validate_install_state(root, &state)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "install state is invalid"))?;
    Ok(state)
}

/// Deterministic per-root state-integrity key. The legacy module reads or
/// creates a per-installation key under a platform state directory; this
/// port derives a stable key from the canonical root path instead, which
/// keeps sealing/verification self-consistent for a given repository
/// without introducing a second, OS-specific key-storage mechanism.
fn state_key(root: &Path) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"membrane-blueprint-init-state-v1");
    hasher.update(root.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

fn apply_plan_files(mut state: InstallState, files: &[PlanFile]) -> std::io::Result<(InstallState, Vec<String>)> {
    let mut written = Vec::new();
    for file in files {
        let existing = if file.path.exists() { Some(fs::read(&file.path)?) } else { None };
        let state_key = file.path.to_string_lossy().to_string();
        state.files.entry(state_key.clone()).or_insert_with(|| FileState {
            exists: existing.is_some(),
            content: existing.clone(),
            installed: None,
        });
        let new_content: Vec<u8> = if file.path.extension().and_then(|e| e.to_str()) == Some("json") {
            // JSON host files use the same merge contract as legacy init:
            // preserve unrelated keys, replace only blueprint's server
            // declaration, and reject malformed input before recording a
            // successful install state.
            let current = existing
                .as_deref()
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .unwrap_or_else(|| "{}".to_string());
            let mut value: Value = serde_json::from_str(&current).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{} is not valid JSON", file.path.display()))
            })?;
            let object = value.as_object_mut().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{} is not valid JSON", file.path.display()))
            })?;
            let servers = object.entry("mcpServers").or_insert_with(|| json!({}));
            if !servers.is_object() {
                *servers = json!({});
            }
            servers
                .as_object_mut()
                .expect("mcpServers object was just normalized")
                .insert("blueprint".into(), crate::lib_cli_mcp::native_mcp_declaration());
            format!("{}\n", serde_json::to_string_pretty(&value).unwrap()).into_bytes()
        } else {
            let before = existing.map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
            crate::lib_init_apply::merge_block(&before).into_bytes()
        };
        if let Some(parent) = file.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&file.path, &new_content)?;
        if let Some(entry) = state.files.get_mut(&state_key) {
            entry.installed = Some(sha256_hex(&new_content));
        }
        written.push(file.path.to_string_lossy().to_string());
    }
    Ok((state, written))
}

fn save_install_state(root: &Path, state: &InstallState) -> std::io::Result<()> {
    let files_json: Map<String, Value> = state
        .files
        .iter()
        .map(|(path, file_state)| {
            (
                path.clone(),
                json!({
                    "exists": file_state.exists,
                    "content": file_state.content.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()),
                    "bytes": file_state.content.as_deref().map(encode_base64),
                    "installed": file_state.installed,
                }),
            )
        })
        .collect();
    let raw = json!({"version": state.version, "files": Value::Object(files_json)});
    let sealed = seal_local_state(&raw, &state_key(root), "install");
    let path = install_state_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&sealed).unwrap()))
}

/// Mirrors `case "init"` in `commands.mjs`: build the plan, then either
/// return it (`--dry-run`) or apply the deterministic file-edit actions and
/// report the OS-orchestration actions as deferred.
pub fn execute_init(request: &BlueprintRequest, root: &Path) -> Result<Value, BlueprintError> {
    let input = &request.input;
    let host = str_field(input, "host", "auto");
    let scope = str_field(input, "scope", "project");
    let mcp = str_field(input, "mcp", "auto");
    let watch = str_field(input, "watch", "auto");
    let hooks = str_field(input, "hooks", "none");
    let policy = str_field(input, "policy", "advisory");
    let dry_run = input.get("dryRun").and_then(Value::as_bool).unwrap_or(false);

    let plan = build_init_plan(
        BuildInitPlanInput { root, host, scope, mcp, watch, hooks, policy },
        |p| p.exists(),
        |_| false,
    )
    .map_err(|error| match error {
        InitPlanError::InvalidScope => BlueprintError::new("invalid_scope", "--scope must be project or user"),
        InitPlanError::DetectHosts(inner) => BlueprintError::new("invalid_host", inner.to_string()),
    })?;

    let plan_json = plan_to_json(&plan);
    if dry_run {
        return Ok(json!({"ok": true, "dryRun": true, "plan": plan_json}));
    }

    let prior_state = load_install_state(root)
        .map_err(|error| BlueprintError::new("blueprint_init_state_invalid", error.to_string()))?;
    let (state, written) = apply_plan_files(prior_state, &plan.files)
        .map_err(|error| BlueprintError::new("blueprint_init_write_failed", error.to_string()))?;
    save_install_state(root, &state)
        .map_err(|error| BlueprintError::new("blueprint_init_state_write_failed", error.to_string()))?;

    let deferred: Vec<&str> = plan
        .actions
        .iter()
        .filter(|a| matches!(a.kind.as_str(), "command" | "service" | "hooks"))
        .map(|a| a.id.as_str())
        .collect();
    let omissions: Vec<Value> = if deferred.is_empty() {
        Vec::new()
    } else {
        vec![json!({
            "code": "orchestration_out_of_scope",
            "detail": "OS-process actions (build-generation/enroll-watch/install-hooks) are not performed by the bounded native operation; run `blueprint refresh` and the watcher/hooks setup explicitly.",
            "actions": deferred,
        })]
    };

    Ok(json!({
        "ok": true,
        "dryRun": false,
        "plan": plan_json,
        "filesWritten": written,
        "omissions": omissions,
    }))
}
