//! Parity coverage for the native `blueprint mcp [print|install]` verb
//! (`blueprint/scripts/cli/commands.mjs:482` case `"mcp"`). Legacy's only
//! subcommand (`mcp serve`) launches the now-superseded legacy JS stdio
//! server (parity-audit.json classifies the native
//! `engine/crates/membrane-mcp` server as its EXECUTED-NATIVELY
//! replacement); these tests instead cover this crate's native
//! replacement verb, which prints/installs the native stdio server
//! declaration into the allowlisted JSON host config files legacy's
//! `init` flow writes (`.mcp.json`, `.claude/settings.json`).

use membrane_blueprint::cli;
use serde_json::Value;
use std::fs;

#[test]
fn print_reports_the_native_declaration_without_writing_any_file() {
    let dir = tempfile::tempdir().unwrap();
    let payload = cli::mcp(dir.path().to_string_lossy().to_string(), Some("print")).unwrap();
    assert_eq!(payload["declaration"]["command"], "membrane");
    assert_eq!(payload["declaration"]["args"][0], "stdio-mcp");
    assert!(!dir.path().join(".mcp.json").exists());
    assert!(!dir.path().join(".claude").join("settings.json").exists());
}

#[test]
fn mcp_defaults_to_print_when_no_subcommand_given() {
    let dir = tempfile::tempdir().unwrap();
    let payload = cli::mcp(dir.path().to_string_lossy().to_string(), None).unwrap();
    assert_eq!(payload["action"], "print");
}

#[test]
fn install_merges_blueprint_server_into_existing_mcp_json_preserving_siblings() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".mcp.json"), r#"{"mcpServers":{"other-tool":{"command":"other"}}}"#).unwrap();
    let payload = cli::mcp(dir.path().to_string_lossy().to_string(), Some("install")).unwrap();
    assert_eq!(payload["written"].as_array().unwrap().len(), 1);
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(dir.path().join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(on_disk["mcpServers"]["other-tool"]["command"], "other");
    assert_eq!(on_disk["mcpServers"]["blueprint"]["command"], "membrane");
    assert_eq!(on_disk["mcpServers"]["blueprint"]["args"][0], "stdio-mcp");
}

#[test]
fn install_never_creates_a_host_config_file_that_did_not_already_exist() {
    let dir = tempfile::tempdir().unwrap();
    let payload = cli::mcp(dir.path().to_string_lossy().to_string(), Some("install")).unwrap();
    assert_eq!(payload["written"].as_array().unwrap().len(), 0);
    assert!(!dir.path().join(".mcp.json").exists());
}

#[test]
fn unknown_subcommand_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = cli::mcp(dir.path().to_string_lossy().to_string(), Some("serve")).unwrap_err();
    assert_eq!(err.code, "usage");
}
