//! Parity test for `blueprint/scripts/cli/commands.mjs` case `"init"`
//! (lines ~199-213), exercised end-to-end through
//! `Operation::Init` -> `engine::native_blueprint_operation`.

use membrane_blueprint::api::{BlueprintApi, BlueprintRequest, CancellationToken};
use membrane_blueprint::engine::native_blueprint_operation;
use membrane_blueprint::model::Operation;
use std::fs;
use tempfile::tempdir;

fn run(root: &std::path::Path, mut input_patch: impl FnMut(&mut serde_json::Value)) -> serde_json::Value {
    let mut request = BlueprintRequest::new("cli-init-test", Operation::Init, root.to_string_lossy());
    input_patch(&mut request.input);
    let response = native_blueprint_operation().dispatch(request, CancellationToken::new());
    assert!(response.ok, "init failed: {:?}", response.error);
    response.result.unwrap()
}

/// `--dry-run` mirrors `commands.mjs`: `buildInitPlan` is called and its
/// plan is returned without writing anything to disk.
#[test]
fn dry_run_returns_plan_without_writing_files() {
    let dir = tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let result = run(&root, |input| {
        input["host"] = "generic".into();
        input["dryRun"] = true.into();
    });
    assert_eq!(result["ok"], true);
    assert_eq!(result["dryRun"], true);
    assert_eq!(result["plan"]["hosts"], serde_json::json!(["generic"]));
    assert!(!root.join("BLUEPRINT-AGENT.md").exists());
}

/// Absent `--dry-run`, the deterministic file-edit actions (host
/// instruction file) are actually applied, and the install state is
/// recorded so `uninstall` could later restore it.
#[test]
fn apply_writes_host_instruction_file_and_install_state() {
    let dir = tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let result = run(&root, |input| {
        input["host"] = "generic".into();
    });
    assert_eq!(result["ok"], true);
    assert_eq!(result["dryRun"], false);
    let instructions = root.join("BLUEPRINT-AGENT.md");
    assert!(instructions.exists());
    let content = fs::read_to_string(&instructions).unwrap();
    assert!(content.contains("blueprint:start"));
    assert!(content.contains("blueprint:end"));

    let state_path = root.join(".agent").join("graph").join("blueprint-install-state.json");
    assert!(state_path.exists());
    let state: serde_json::Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(state.get("integrity").is_some(), "install state must be sealed");

    let files_written = result["filesWritten"].as_array().unwrap();
    assert!(files_written.iter().any(|v| v.as_str().unwrap().ends_with("BLUEPRINT-AGENT.md")));
}

/// `claude-code` host enables `.mcp.json`, which is not a marker-block
/// merge target -- it should be created fresh with an empty server map.
#[test]
fn claude_code_host_creates_mcp_json() {
    let dir = tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let result = run(&root, |input| {
        input["host"] = "claude-code".into();
    });
    assert_eq!(result["ok"], true);
    let mcp_path = root.join(".mcp.json");
    assert!(mcp_path.exists());
    let mcp: serde_json::Value = serde_json::from_str(&fs::read_to_string(&mcp_path).unwrap()).unwrap();
    assert!(mcp.get("mcpServers").is_some());
}

/// Re-applying init on an existing instruction file replaces the marker
/// block rather than duplicating it (mirrors `mergeBlock`'s idempotency).
#[test]
fn reapplying_init_does_not_duplicate_marker_block() {
    let dir = tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    run(&root, |input| { input["host"] = "generic".into(); });
    run(&root, |input| { input["host"] = "generic".into(); });
    let content = fs::read_to_string(root.join("BLUEPRINT-AGENT.md")).unwrap();
    assert_eq!(content.matches("<!-- blueprint:start -->").count(), 1);
}

/// An invalid `--scope` is rejected the same way the legacy planner rejects
/// it, before any file is touched.
#[test]
fn invalid_scope_is_rejected() {
    let dir = tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let mut request = BlueprintRequest::new("cli-init-invalid-scope", Operation::Init, root.to_string_lossy());
    request.input["scope"] = "bogus".into();
    let response = native_blueprint_operation().dispatch(request, CancellationToken::new());
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "invalid_scope");
}

/// The `build-generation` / `enroll-watch` OS-orchestration plan actions are
/// reported as a typed omission rather than silently dropped or falsely
/// claimed as completed.
#[test]
fn os_orchestration_actions_are_reported_as_omissions() {
    let dir = tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let result = run(&root, |input| { input["host"] = "generic".into(); input["watch"] = "on".into(); });
    let omissions = result["omissions"].as_array().unwrap();
    assert!(!omissions.is_empty());
    assert_eq!(omissions[0]["code"], "orchestration_out_of_scope");
}
