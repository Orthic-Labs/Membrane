//! Native Rust port of the CLI `uninstall` verb
//! (`blueprint/scripts/cli/commands.mjs:217` case `"uninstall"`, which
//! forwards to `blueprint/src/lib/init/apply.mjs`'s `uninstallInit`).
//!
//! Lane V3 (r5 closure). Reuses the already-ported pure pieces in
//! [`crate::lib_init_apply`] (`InstallState`, `validate_install_state`,
//! `restore`, `sha256_hex`, `allowed_targets`) for the state shape and file
//! replay, and adds the JSON state-file IO and drift-check orchestration
//! `uninstallInit` layers on top: read
//! `<root>/.agent/graph/blueprint-install-state.json`, validate it, verify
//! every recorded `installed` hash still matches the file currently on
//! disk (refusing to uninstall over a conflicting edit — legacy's
//! `state_conflict`), replay the captured before-state, then delete the
//! install-state file. This never touches the installed Membrane product;
//! it only reverses `blueprint init`'s repo-local file edits.
//!
//! Not ported: watcher enrollment/unenrollment (`state.watch`,
//! `unenrollWatch`) and the separate `.agent/install-state` integrity key
//! (`removeInstallStateKey`) — both are resident/Hub-owned concerns outside
//! this bounded one-shot verb's scope, matching the audit note's framing of
//! `uninstall` as a repo-local, non-resident operation.

use crate::lib_init_apply::{restore, sha256_hex, validate_install_state, FileState, InstallState, InstallStateError};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn state_path(root: &Path) -> PathBuf {
    root.join(".agent").join("graph").join("blueprint-install-state.json")
}

#[derive(Debug, Clone, PartialEq)]
pub enum UninstallError {
    /// Mirrors the `state_invalid` outcome (malformed/tampered state file).
    StateInvalid,
    /// Mirrors `state_conflict`: a recorded file's on-disk content no
    /// longer matches the hash captured at install time.
    StateConflict,
    Io(String),
}

impl UninstallError {
    pub fn code(&self) -> &'static str {
        match self {
            UninstallError::StateInvalid => "state_invalid",
            UninstallError::StateConflict => "state_conflict",
            UninstallError::Io(_) => "io_error",
        }
    }
}

fn parse_state(root: &Path, raw: &Value) -> Result<InstallState, UninstallError> {
    let obj = raw.as_object().ok_or(UninstallError::StateInvalid)?;
    let version = obj.get("version").and_then(Value::as_u64).ok_or(UninstallError::StateInvalid)? as u32;
    let files_obj = obj.get("files").and_then(Value::as_object).ok_or(UninstallError::StateInvalid)?;
    let mut files = BTreeMap::new();
    for (path, entry) in files_obj {
        let entry = entry.as_object().ok_or(UninstallError::StateInvalid)?;
        let exists = entry.get("exists").and_then(Value::as_bool).ok_or(UninstallError::StateInvalid)?;
        let content = match entry.get("bytes").and_then(Value::as_str) {
            Some(b64) => Some(
                base64_decode(b64).ok_or(UninstallError::StateInvalid)?,
            ),
            None => entry.get("content").and_then(Value::as_str).map(|s| s.as_bytes().to_vec()),
        };
        let installed = entry.get("installed").and_then(Value::as_str).map(str::to_owned);
        files.insert(
            path.clone(),
            FileState { exists, content, installed },
        );
    }
    let state = InstallState { version, files };
    validate_install_state(root, &state).map_err(|e| match e {
        InstallStateError::Invalid => UninstallError::StateInvalid,
    })?;
    Ok(state)
}

// Minimal, dependency-free base64 decoder (standard alphabet, matches
// Node's `Buffer.toString("base64")` output used by the legacy installer).
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = input.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
    if bytes.is_empty() { return Some(Vec::new()); }
    if bytes.len() % 4 != 0 { return None; }
    let first_padding = bytes.iter().position(|&b| b == b'=');
    if let Some(index) = first_padding {
        let padding = bytes.len() - index;
        if padding > 2 || bytes[index..].iter().any(|&b| b != b'=') { return None; }
    }
    let stripped: Vec<u8> = bytes.iter().cloned().take_while(|&b| b != b'=').collect();
    let mut out = Vec::with_capacity(stripped.len() * 3 / 4 + 3);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for b in stripped {
        let v = val(b)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

/// Mirrors `uninstallInit({ root })`. `idempotent: true` when no install
/// state file exists (nothing to uninstall, not an error).
pub fn run(root: &Path) -> Value {
    let path = state_path(root);
    if !path.exists() {
        return json!({
            "schemaVersion": 1, "ok": true, "action": "uninstalled",
            "root": root.display().to_string(), "restored": [], "idempotent": true,
        });
    }
    match uninstall_inner(root, &path) {
        Ok(restored) => json!({
            "schemaVersion": 1, "ok": true, "action": "uninstalled",
            "root": root.display().to_string(), "restored": restored, "idempotent": false,
        }),
        Err(error) => json!({
            "schemaVersion": 1, "ok": false, "action": "uninstall_failed",
            "root": root.display().to_string(), "restored": [], "error": error.code(), "idempotent": false,
        }),
    }
}

fn uninstall_inner(root: &Path, path: &Path) -> Result<Vec<String>, UninstallError> {
    let raw_text = fs::read_to_string(path).map_err(|e| UninstallError::Io(e.to_string()))?;
    let raw: Value = serde_json::from_str(&raw_text).map_err(|_| UninstallError::StateInvalid)?;
    let state = parse_state(root, &raw)?;

    for (target, original) in &state.files {
        let Some(installed) = &original.installed else { continue };
        let target_path = Path::new(target);
        if !target_path.exists() {
            return Err(UninstallError::StateConflict);
        }
        let bytes = fs::read(target_path).map_err(|e| UninstallError::Io(e.to_string()))?;
        if &sha256_hex(&bytes) != installed {
            return Err(UninstallError::StateConflict);
        }
    }

    let restored: Vec<String> = state.files.keys().cloned().collect();
    restore(&state).map_err(|e| UninstallError::Io(e.to_string()))?;
    let _ = fs::remove_file(path);
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_state(root: &Path, files: Value) {
        let dir = root.join(".agent").join("graph");
        fs::create_dir_all(&dir).unwrap();
        let state = json!({"version": 1, "files": files});
        fs::write(dir.join("blueprint-install-state.json"), serde_json::to_string_pretty(&state).unwrap()).unwrap();
    }

    #[test]
    fn no_state_file_is_idempotent_success() {
        let dir = tempfile::tempdir().unwrap();
        let payload = run(dir.path());
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["idempotent"], true);
    }

    #[test]
    fn restores_original_content_and_removes_new_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let claude_md = root.join("CLAUDE.md");
        fs::write(&claude_md, "installed content").unwrap();
        let installed_hash = sha256_hex(b"installed content");

        let agents_md = root.join("AGENTS.md");
        fs::write(&agents_md, "created by init").unwrap();
        let agents_hash = sha256_hex(b"created by init");

        write_state(
            root,
            json!({
                claude_md.to_string_lossy(): {"exists": true, "content": "original content", "installed": installed_hash},
                agents_md.to_string_lossy(): {"exists": false, "content": null, "installed": agents_hash},
            }),
        );

        let payload = run(root);
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["idempotent"], false);
        assert_eq!(fs::read_to_string(&claude_md).unwrap(), "original content");
        assert!(!agents_md.exists());
        assert!(!state_path(root).exists());
    }

    #[test]
    fn refuses_to_uninstall_over_a_conflicting_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let claude_md = root.join("CLAUDE.md");
        fs::write(&claude_md, "edited after install, not what we installed").unwrap();

        write_state(
            root,
            json!({
                claude_md.to_string_lossy(): {"exists": true, "content": "original content", "installed": sha256_hex(b"installed content")},
            }),
        );

        let payload = run(root);
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["error"], "state_conflict");
        // The conflicting file must be left untouched.
        assert_eq!(fs::read_to_string(&claude_md).unwrap(), "edited after install, not what we installed");
        assert!(state_path(root).exists());
    }

    #[test]
    fn rejects_a_state_file_referencing_a_non_allowlisted_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_state(
            root,
            json!({
                root.join("random.txt").to_string_lossy(): {"exists": true, "content": "x", "installed": "a".repeat(64)},
            }),
        );
        let payload = run(root);
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["error"], "state_invalid");
    }
}
