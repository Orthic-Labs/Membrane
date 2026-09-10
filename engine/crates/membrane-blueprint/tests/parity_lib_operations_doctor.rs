//! Parity test for `blueprint/src/lib/operations/doctor.mjs` (scoped to its
//! deterministic map/stale diagnostics core and MCP-config launchability
//! shape checks -- see lib_operations_doctor.rs module docs).

use membrane_blueprint::lib_operations_doctor::{collect_map_diagnostics, mcp_config_launchable, McpLaunchable};
use serde_json::json;
use std::fs;

#[test]
fn missing_map_json_reports_missing_state_and_blocker_reason() {
    let dir = tempfile::tempdir().unwrap();
    let diag = collect_map_diagnostics(dir.path(), ".agent");
    assert_eq!(diag.state, "missing");
    assert_eq!(diag.reasons.len(), 1);
    assert_eq!(diag.reasons[0].code, "missing_map");
    assert_eq!(diag.reasons[0].severity, "blocker");
}

#[test]
fn unparseable_map_json_reports_corrupt_state() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent")).unwrap();
    fs::write(dir.path().join(".agent/map.json"), "{{{not json").unwrap();
    let diag = collect_map_diagnostics(dir.path(), ".agent");
    assert_eq!(diag.state, "corrupt");
    assert_eq!(diag.reasons[0].code, "corrupt_map");
}

#[test]
fn duplicate_ids_and_dangling_edges_produce_broken_state_with_errors() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent")).unwrap();
    let map = json!({
        "nodes": [{"id": "n1"}, {"id": "n1"}, {"id": "n2"}],
        "edges": [{"from": "n1", "to": "ghost"}],
    });
    fs::write(dir.path().join(".agent/map.json"), map.to_string()).unwrap();
    let diag = collect_map_diagnostics(dir.path(), ".agent");
    assert_eq!(diag.state, "broken");
    assert!(diag.errors.iter().any(|e| e.starts_with("duplicate node id")));
    assert!(diag.errors.iter().any(|e| e.contains("ghost")));
    assert!(diag.reasons.iter().any(|r| r.code == "duplicate_node_ids"));
}

#[test]
fn missing_references_from_stale_json_become_warnings_capped_at_ten() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent")).unwrap();
    fs::write(
        dir.path().join(".agent/map.json"),
        json!({ "nodes": [], "edges": [] }).to_string(),
    )
    .unwrap();
    let missing: Vec<_> = (0..15)
        .map(|i| json!({ "source": format!("doc{i}.md"), "path": format!("gone{i}.rs") }))
        .collect();
    fs::write(
        dir.path().join(".agent/stale.json"),
        json!({ "missingReferences": missing }).to_string(),
    )
    .unwrap();
    let diag = collect_map_diagnostics(dir.path(), ".agent");
    assert_eq!(diag.warnings.len(), 10);
    assert!(diag.reasons.iter().any(|r| r.code == "missing_references"));
}

#[test]
fn no_mcp_config_file_means_no_launchability_result() {
    let dir = tempfile::tempdir().unwrap();
    assert!(mcp_config_launchable(dir.path()).is_none());
}

#[test]
fn mcp_config_without_blueprint_server_entry_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(".mcp.json"),
        json!({ "mcpServers": { "other": {} } }).to_string(),
    )
    .unwrap();
    assert!(mcp_config_launchable(dir.path()).is_none());
}

#[test]
fn mcp_config_with_non_string_command_warns_without_spawning() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(".mcp.json"),
        json!({ "mcpServers": { "blueprint": { "command": 5 } } }).to_string(),
    )
    .unwrap();
    match mcp_config_launchable(dir.path()) {
        Some(McpLaunchable::Warning { message }) => assert!(message.contains("command is missing")),
        other => panic!("expected a Warning result, got something else: {}", other.is_some()),
    }
}
