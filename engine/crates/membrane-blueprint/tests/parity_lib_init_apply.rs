//! Parity test for `blueprint/src/lib/init/apply.mjs` (scoped to its
//! deterministic core -- see lib_init_apply.rs module docs).

use membrane_blueprint::lib_init_apply::{
    is_allowed_target, merge_block, remove_block, restore, validate_install_state, FileState,
    InstallState, InstallStateError,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[test]
fn allowlist_matches_the_legacy_fixed_target_set() {
    let root = Path::new("/repo");
    for name in ["CLAUDE.md", "AGENTS.md", "BLUEPRINT-AGENT.md", ".mcp.json"] {
        assert!(is_allowed_target(root, &root.join(name)));
    }
    assert!(is_allowed_target(
        root,
        &root.join(".cursor").join("rules").join("blueprint.mdc")
    ));
    assert!(is_allowed_target(
        root,
        &root.join(".claude").join("settings.json")
    ));
    assert!(!is_allowed_target(root, &root.join("package.json")));
}

#[test]
fn validate_install_state_rejects_target_outside_allowlist() {
    let root = Path::new("/repo");
    let mut files = BTreeMap::new();
    files.insert(
        root.join("evil.sh").to_string_lossy().to_string(),
        FileState {
            exists: true,
            content: Some(b"x".to_vec()),
            installed: Some("a".repeat(64)),
        },
    );
    let state = InstallState { version: 1, files };
    assert_eq!(
        validate_install_state(root, &state).unwrap_err(),
        InstallStateError::Invalid
    );
}

#[test]
fn validate_install_state_accepts_a_well_formed_state() {
    let root = Path::new("/repo");
    let mut files = BTreeMap::new();
    files.insert(
        root.join("CLAUDE.md").to_string_lossy().to_string(),
        FileState {
            exists: true,
            content: Some(b"content".to_vec()),
            installed: Some("f".repeat(64)),
        },
    );
    let state = InstallState { version: 1, files };
    assert!(validate_install_state(root, &state).is_ok());
}

#[test]
fn merge_block_is_idempotent_and_never_duplicates_the_marker() {
    let once = merge_block("# My CLAUDE.md\n");
    let twice = merge_block(&once);
    assert_eq!(once, twice);
    assert_eq!(twice.matches("<!-- blueprint:start -->").count(), 1);
}

#[test]
fn remove_block_leaves_surrounding_content_untouched() {
    let content = "before\n\n<!-- blueprint:start -->\nblock\n<!-- blueprint:end -->\n\nafter\n";
    let cleaned = remove_block(content);
    assert!(cleaned.contains("before"));
    assert!(cleaned.contains("after"));
    assert!(!cleaned.contains("block"));
}

#[test]
fn restore_writes_back_a_captured_before_state_and_deletes_new_files() {
    let dir = tempfile::tempdir().unwrap();
    let claude_md = dir.path().join("CLAUDE.md");
    let mcp_json = dir.path().join(".mcp.json");
    fs::write(&mcp_json, b"{\"installed\": true}").unwrap();

    let mut files = BTreeMap::new();
    files.insert(
        claude_md.to_string_lossy().to_string(),
        FileState {
            exists: true,
            content: Some(b"# pre-existing content\n".to_vec()),
            installed: None,
        },
    );
    files.insert(
        mcp_json.to_string_lossy().to_string(),
        FileState {
            exists: false,
            content: None,
            installed: None,
        },
    );
    let state = InstallState { version: 1, files };
    restore(&state).unwrap();

    assert_eq!(fs::read(&claude_md).unwrap(), b"# pre-existing content\n");
    assert!(!mcp_json.exists());
}
