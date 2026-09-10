//! Parity tests for `blueprint uninstall`
//! (`blueprint/scripts/cli/commands.mjs:217` case `"uninstall"` ->
//! `blueprint/src/lib/init/apply.mjs`'s `uninstallInit`), ported to the
//! native `membrane_blueprint::cli::uninstall` dispatch. Never touches the
//! installed Membrane product -- only reverses `blueprint init`'s
//! repo-local file edits recorded under
//! `.agent/graph/blueprint-install-state.json`.

use membrane_blueprint::cli;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn write_state(root: &std::path::Path, files: serde_json::Value) {
    let dir = root.join(".agent").join("graph");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("blueprint-install-state.json"),
        serde_json::to_string_pretty(&json!({"version": 1, "files": files})).unwrap(),
    )
    .unwrap();
}

#[test]
fn no_install_state_is_reported_as_an_idempotent_success() {
    let dir = tempfile::tempdir().unwrap();
    let payload = cli::uninstall(dir.path().to_string_lossy().to_string());
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["action"], "uninstalled");
    assert_eq!(payload["idempotent"], true);
    assert_eq!(payload["restored"].as_array().unwrap().len(), 0);
}

#[test]
fn uninstall_restores_original_content_and_deletes_files_init_created() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let claude_md = root.join("CLAUDE.md");
    fs::write(&claude_md, "installed marker block").unwrap();
    let mcp_json = root.join(".mcp.json");
    fs::write(&mcp_json, r#"{"mcpServers":{"blueprint":{}}}"#).unwrap();

    write_state(
        root,
        json!({
            claude_md.to_string_lossy(): {
                "exists": true, "content": "# my original notes\n",
                "installed": sha256_hex(b"installed marker block"),
            },
            mcp_json.to_string_lossy(): {
                "exists": false, "content": null,
                "installed": sha256_hex(br#"{"mcpServers":{"blueprint":{}}}"#),
            },
        }),
    );

    let payload = cli::uninstall(root.to_string_lossy().to_string());
    assert_eq!(payload["ok"], true, "uninstall payload: {payload}");
    assert_eq!(payload["idempotent"], false);
    let restored = payload["restored"].as_array().unwrap();
    assert_eq!(restored.len(), 2);

    assert_eq!(fs::read_to_string(&claude_md).unwrap(), "# my original notes\n");
    assert!(!mcp_json.exists());
    assert!(
        !root.join(".agent").join("graph").join("blueprint-install-state.json").exists(),
        "install-state marker must be removed after a successful uninstall"
    );
}

#[test]
fn uninstall_refuses_when_a_recorded_file_was_edited_since_install() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let claude_md = root.join("CLAUDE.md");
    fs::write(&claude_md, "a user edit that happened after blueprint init").unwrap();

    write_state(
        root,
        json!({
            claude_md.to_string_lossy(): {
                "exists": true, "content": "# my original notes\n",
                "installed": sha256_hex(b"installed marker block, not what is on disk now"),
            },
        }),
    );

    let payload = cli::uninstall(root.to_string_lossy().to_string());
    assert_eq!(payload["ok"], false);
    assert_eq!(payload["error"], "state_conflict");
    // The conflicting edit must survive untouched, and the state file must
    // remain so a later, informed uninstall attempt is still possible.
    assert_eq!(
        fs::read_to_string(&claude_md).unwrap(),
        "a user edit that happened after blueprint init"
    );
    assert!(root.join(".agent").join("graph").join("blueprint-install-state.json").exists());
}

#[test]
fn uninstall_rejects_a_tampered_state_file_referencing_an_unallowlisted_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_state(
        root,
        json!({
            root.join("not-an-allowlisted-target.txt").to_string_lossy(): {
                "exists": true, "content": "x", "installed": "a".repeat(64),
            },
        }),
    );
    let payload = cli::uninstall(root.to_string_lossy().to_string());
    assert_eq!(payload["ok"], false);
    assert_eq!(payload["error"], "state_invalid");
}
