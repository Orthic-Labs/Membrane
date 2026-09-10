//! Native Rust port of `blueprint/src/lib/init/plan.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `buildInitPlan`/`build_init_plan`/`INIT_PLAN_VERSION` across
//! membrane-blueprint/src and membrane-runtime/src produced no match).
//! Ported behavior: pure planner that returns planned file edits, MCP
//! entry, hooks, watcher enrollment, first-build command, and uninstall
//! instructions without writing anything.

use crate::lib_init_detect_hosts::detect_hosts;
use std::path::{Path, PathBuf};

pub const INIT_PLAN_VERSION: u32 = 1;
const SCOPES: [&str; 2] = ["project", "user"];

#[derive(Debug, thiserror::Error)]
pub enum InitPlanError {
    #[error("--scope must be project or user")]
    InvalidScope,
    #[error(transparent)]
    DetectHosts(#[from] crate::lib_init_detect_hosts::DetectHostsError),
}

#[derive(Debug, Clone)]
pub struct PlanAction {
    pub id: String,
    pub kind: String,
    pub path: Option<PathBuf>,
    pub reversible: bool,
}

#[derive(Debug, Clone)]
pub struct PlanFile {
    pub path: PathBuf,
    pub host: String,
}

#[derive(Debug, Clone)]
pub struct InitPlan {
    pub schema_version: u32,
    pub root: PathBuf,
    pub hosts: Vec<String>,
    pub scope: String,
    pub policy: String,
    pub mcp_enabled: bool,
    pub watch_enabled: bool,
    pub actions: Vec<PlanAction>,
    pub files: Vec<PlanFile>,
    pub uninstall_command: String,
}

fn instruction_path(root: &Path, host: &str) -> PathBuf {
    match host {
        "claude-code" => root.join("CLAUDE.md"),
        "codex" => root.join("AGENTS.md"),
        "cursor" => root.join(".cursor").join("rules").join("blueprint.mdc"),
        _ => root.join("BLUEPRINT-AGENT.md"),
    }
}

pub struct BuildInitPlanInput<'a> {
    pub root: &'a Path,
    pub host: &'a str,
    pub scope: &'a str,
    pub mcp: &'a str,
    pub watch: &'a str,
    pub hooks: &'a str,
    pub policy: &'a str,
}

impl<'a> Default for BuildInitPlanInput<'a> {
    fn default() -> Self {
        Self {
            root: Path::new("."),
            host: "auto",
            scope: "project",
            mcp: "auto",
            watch: "auto",
            hooks: "none",
            policy: "advisory",
        }
    }
}

/// Mirrors `buildInitPlan(...)`: pure — writes nothing, only computes actions.
pub fn build_init_plan(
    input: BuildInitPlanInput,
    exists: impl Fn(&Path) -> bool,
    command_probe: impl Fn(&str) -> bool,
) -> Result<InitPlan, InitPlanError> {
    let hosts = detect_hosts(
        if input.host == "auto" { None } else { Some(input.host) },
        input.root,
        exists,
        command_probe,
    )?;
    if !SCOPES.contains(&input.scope) {
        return Err(InitPlanError::InvalidScope);
    }
    let mut actions = Vec::new();
    let mut files = Vec::new();
    for h in &hosts {
        let path = instruction_path(input.root, h);
        files.push(PlanFile {
            path: path.clone(),
            host: h.clone(),
        });
        actions.push(PlanAction {
            id: format!("write-host-instructions-{h}"),
            kind: "file-edit".to_string(),
            path: Some(path),
            reversible: true,
        });
    }
    let mcp_enabled = input.mcp == "on" || (input.mcp == "auto" && hosts.iter().any(|h| h == "claude-code"));
    if mcp_enabled {
        let path = input.root.join(".mcp.json");
        files.push(PlanFile {
            path: path.clone(),
            host: "claude-code".to_string(),
        });
        actions.push(PlanAction {
            id: "install-mcp".to_string(),
            kind: "file-edit".to_string(),
            path: Some(path),
            reversible: true,
        });
    }
    let watch_enabled = input.watch == "on" || (input.watch == "auto" && input.scope == "project");
    if input.hooks != "none" {
        actions.push(PlanAction {
            id: "install-hooks".to_string(),
            kind: "hooks".to_string(),
            path: None,
            reversible: true,
        });
    }
    actions.push(PlanAction {
        id: "build-generation".to_string(),
        kind: "command".to_string(),
        path: None,
        reversible: false,
    });
    if watch_enabled {
        actions.push(PlanAction {
            id: "enroll-watch".to_string(),
            kind: "service".to_string(),
            path: None,
            reversible: true,
        });
    }
    Ok(InitPlan {
        schema_version: INIT_PLAN_VERSION,
        root: input.root.to_path_buf(),
        hosts,
        scope: input.scope.to_string(),
        policy: input.policy.to_string(),
        mcp_enabled,
        watch_enabled,
        actions,
        files,
        uninstall_command: format!("blueprint uninstall --root {}", input.root.display()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_for_claude_code_host_enables_mcp_and_watch() {
        let root = Path::new("/repo");
        let plan = build_init_plan(
            BuildInitPlanInput {
                root,
                host: "claude-code",
                ..Default::default()
            },
            |_| false,
            |_| false,
        )
        .unwrap();
        assert_eq!(plan.hosts, vec!["claude-code".to_string()]);
        assert!(plan.mcp_enabled);
        assert!(plan.watch_enabled);
        let ids: Vec<_> = plan.actions.iter().map(|a| a.id.as_str()).collect();
        assert!(ids.contains(&"write-host-instructions-claude-code"));
        assert!(ids.contains(&"install-mcp"));
        assert!(ids.contains(&"build-generation"));
        assert!(ids.contains(&"enroll-watch"));
        assert_eq!(plan.uninstall_command, "blueprint uninstall --root /repo");
    }

    #[test]
    fn plan_for_generic_host_skips_mcp() {
        let root = Path::new("/repo");
        let plan = build_init_plan(
            BuildInitPlanInput {
                root,
                host: "generic",
                ..Default::default()
            },
            |_| false,
            |_| false,
        )
        .unwrap();
        assert!(!plan.mcp_enabled);
        assert!(!plan.actions.iter().any(|a| a.id == "install-mcp"));
    }

    #[test]
    fn invalid_scope_errors() {
        let root = Path::new("/repo");
        let err = build_init_plan(
            BuildInitPlanInput {
                root,
                scope: "bogus",
                ..Default::default()
            },
            |_| false,
            |_| false,
        )
        .unwrap_err();
        assert!(matches!(err, InitPlanError::InvalidScope));
    }

    #[test]
    fn hooks_none_skips_install_hooks_action() {
        let root = Path::new("/repo");
        let plan = build_init_plan(
            BuildInitPlanInput {
                root,
                host: "generic",
                ..Default::default()
            },
            |_| false,
            |_| false,
        )
        .unwrap();
        assert!(!plan.actions.iter().any(|a| a.id == "install-hooks"));
    }
}
