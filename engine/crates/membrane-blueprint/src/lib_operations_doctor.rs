//! Native Rust port of `blueprint/src/lib/operations/doctor.mjs`.
//!
//! Lane LIB3 (r5 closure): no exact native equivalent found. A doctor module
//! exists at `membrane-runtime/src/doctor.rs` (`DoctorReportV0`/`run`/
//! `run_with_policy`), but reading it shows a different contract (a flat
//! check-list report with no `missing_map`/`corrupt_map`/`stale_graph`/
//! `mcp_config_launchable` reason-code ladder and no MCP liveness probe), so
//! it is not treated as a match here. Ported behavior: the typed
//! `ready|degraded|stale|broken|corrupt|missing` reason-code ladder driven
//! by `map.json`/`stale.json` presence, duplicate-node detection, dangling
//! edges, stale references, and an MCP-server launchability liveness probe
//! (spawn the configured command, wait a fixed window, alive == pass).

use serde_json::Value;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const MCP_LIVENESS_MS: u64 = 4000;

#[derive(Debug, Clone, PartialEq)]
pub struct DoctorReason {
    pub code: String,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DoctorDiagnostics {
    pub schema_version: u32,
    pub state: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub reasons: Vec<DoctorReason>,
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub enum McpLaunchable {
    Pass { command: String, args: Vec<String> },
    Warning { message: String },
    Fail { command: String, args: Vec<String>, exit_code: Option<i32> },
}

enum Liveness {
    Alive,
    Exited(Option<i32>),
}

/// Spawns `command`/`args` with a live stdin and waits `MCP_LIVENESS_MS`.
/// Mirrors the legacy child-process liveness probe: a config that keeps
/// running (e.g. an MCP stdio server waiting on its transport) is alive
/// through the window; a broken config exits early.
fn probe_liveness(command: &str, args: &[String]) -> Result<Liveness, String> {
    let mut child: Child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_millis(MCP_LIVENESS_MS);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Liveness::Exited(status.code())),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(Liveness::Alive);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Mirrors `mcpConfigLaunchable(root)`: applies only when `.mcp.json` parses
/// and has `mcpServers.blueprint`; `None` otherwise.
pub fn mcp_config_launchable(root: &Path) -> Option<McpLaunchable> {
    let config = read_json(&root.join(".mcp.json"))?;
    let entry = config.get("mcpServers")?.get("blueprint")?;
    if !entry.is_object() {
        return None;
    }
    let command = entry.get("command").and_then(Value::as_str);
    let args: Vec<String> = entry
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let Some(command) = command.filter(|c| !c.is_empty()) else {
        return Some(McpLaunchable::Warning {
            message: "MCP server config for blueprint is invalid: command is missing or not a string, so launchability cannot be checked.".to_string(),
        });
    };
    match probe_liveness(command, &args) {
        Err(spawn_error) => Some(McpLaunchable::Warning {
            message: format!("MCP server config for blueprint could not be spawn-checked: {spawn_error}"),
        }),
        Ok(Liveness::Alive) => Some(McpLaunchable::Pass {
            command: command.to_string(),
            args,
        }),
        Ok(Liveness::Exited(exit_code)) => Some(McpLaunchable::Fail {
            command: command.to_string(),
            args,
            exit_code,
        }),
    }
}

/// Mirrors `collectDoctorDiagnostics(root, outDir, { full })`'s reason-code
/// core (map.json/stale.json presence, duplicate-node/dangling-edge/
/// stale-reference detection). The MCP liveness probe and graph freshness
/// check are intentionally left to their own call sites since they need
/// live process/filesystem context; this function is the deterministic,
/// testable slice of the diagnostics ladder.
pub fn collect_map_diagnostics(root: &Path, out_dir: &str) -> DoctorDiagnostics {
    let map_path = root.join(out_dir).join("map.json");
    if !map_path.exists() {
        return DoctorDiagnostics {
            schema_version: 1,
            state: "missing".to_string(),
            errors: vec![format!("{out_dir}/map.json missing; run build first")],
            warnings: vec![],
            reasons: vec![DoctorReason {
                code: "missing_map".to_string(),
                severity: "blocker".to_string(),
                message: "Blueprint map.json is not present; planner cannot retrieve candidates.".to_string(),
            }],
        };
    }
    let map = match read_json(&map_path) {
        Some(m) => m,
        None => {
            return DoctorDiagnostics {
                schema_version: 1,
                state: "corrupt".to_string(),
                errors: vec!["map.json could not be parsed".to_string()],
                warnings: vec![],
                reasons: vec![DoctorReason {
                    code: "corrupt_map".to_string(),
                    severity: "blocker".to_string(),
                    message: "Blueprint map.json could not be parsed.".to_string(),
                }],
            };
        }
    };
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut reasons = Vec::new();
    let empty = Vec::new();
    let nodes = map.get("nodes").and_then(Value::as_array).unwrap_or(&empty);
    let edges = map.get("edges").and_then(Value::as_array).unwrap_or(&empty);
    let mut ids = std::collections::HashSet::new();
    for node in nodes {
        if let Some(id) = node.get("id").and_then(Value::as_str) {
            if !ids.insert(id.to_string()) {
                errors.push(format!("duplicate node id: {id}"));
            }
        }
    }
    if ids.len() != nodes.len() {
        reasons.push(DoctorReason {
            code: "duplicate_node_ids".to_string(),
            severity: "blocker".to_string(),
            message: format!(
                "graph contains {} duplicate node id(s); discovery must enforce collision-safe IDs.",
                nodes.len() - ids.len()
            ),
        });
    }
    for edge in edges {
        let from = edge.get("from").and_then(Value::as_str).unwrap_or("");
        let to = edge.get("to").and_then(Value::as_str).unwrap_or("");
        if !ids.contains(from) {
            errors.push(format!("edge missing from node: {from}"));
        }
        if !ids.contains(to) {
            errors.push(format!("edge missing to node: {to}"));
        }
    }
    let stale_path = root.join(out_dir).join("stale.json");
    let stale = read_json(&stale_path).unwrap_or_else(|| serde_json::json!({ "missingReferences": [] }));
    let missing_refs = stale
        .get("missingReferences")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for warning in missing_refs.iter().take(10) {
        let source = warning.get("source").and_then(Value::as_str).unwrap_or("");
        let path = warning.get("path").and_then(Value::as_str).unwrap_or("");
        warnings.push(format!("{source} mentions missing {path}"));
    }
    if !missing_refs.is_empty() {
        reasons.push(DoctorReason {
            code: "missing_references".to_string(),
            severity: "warning".to_string(),
            message: "documents reference paths that no longer exist on disk".to_string(),
        });
    }
    DoctorDiagnostics {
        schema_version: 1,
        state: if !errors.is_empty() { "broken" } else { "degraded" }.to_string(),
        errors,
        warnings,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn missing_map_reports_missing_state() {
        let dir = tempfile::tempdir().unwrap();
        let diag = collect_map_diagnostics(dir.path(), ".agent");
        assert_eq!(diag.state, "missing");
        assert_eq!(diag.reasons[0].code, "missing_map");
    }

    #[test]
    fn corrupt_map_reports_corrupt_state() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent")).unwrap();
        fs::write(dir.path().join(".agent/map.json"), "{not json").unwrap();
        let diag = collect_map_diagnostics(dir.path(), ".agent");
        assert_eq!(diag.state, "corrupt");
        assert_eq!(diag.reasons[0].code, "corrupt_map");
    }

    #[test]
    fn duplicate_node_ids_and_dangling_edges_are_flagged() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent")).unwrap();
        let map = json!({
            "nodes": [{"id": "a"}, {"id": "a"}, {"id": "b"}],
            "edges": [{"from": "a", "to": "missing"}],
        });
        fs::write(dir.path().join(".agent/map.json"), map.to_string()).unwrap();
        let diag = collect_map_diagnostics(dir.path(), ".agent");
        assert_eq!(diag.state, "broken");
        assert!(diag.errors.iter().any(|e| e.contains("edge missing to node: missing")));
        assert!(diag.reasons.iter().any(|r| r.code == "duplicate_node_ids"));
    }

    #[test]
    fn valid_map_with_no_issues_is_degraded_not_broken() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent")).unwrap();
        let map = json!({ "nodes": [{"id": "a"}], "edges": [] });
        fs::write(dir.path().join(".agent/map.json"), map.to_string()).unwrap();
        let diag = collect_map_diagnostics(dir.path(), ".agent");
        assert_eq!(diag.state, "degraded");
        assert!(diag.errors.is_empty());
    }

    #[test]
    fn missing_mcp_config_yields_no_launchable_result() {
        let dir = tempfile::tempdir().unwrap();
        assert!(mcp_config_launchable(dir.path()).is_none());
    }

    #[test]
    fn mcp_config_without_command_warns() {
        let dir = tempfile::tempdir().unwrap();
        let config = json!({ "mcpServers": { "blueprint": {} } });
        fs::write(dir.path().join(".mcp.json"), config.to_string()).unwrap();
        match mcp_config_launchable(dir.path()) {
            Some(McpLaunchable::Warning { message }) => {
                assert!(message.contains("command is missing"));
            }
            other => panic!("expected Warning, got {other:?}", other = debug(&other)),
        }
    }

    fn debug(v: &Option<McpLaunchable>) -> String {
        match v {
            Some(McpLaunchable::Pass { .. }) => "Pass".to_string(),
            Some(McpLaunchable::Warning { .. }) => "Warning".to_string(),
            Some(McpLaunchable::Fail { .. }) => "Fail".to_string(),
            None => "None".to_string(),
        }
    }
}
