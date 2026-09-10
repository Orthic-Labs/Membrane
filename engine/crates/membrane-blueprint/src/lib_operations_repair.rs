//! Native Rust port of `blueprint/src/lib/operations/repair.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `buildRepairPlan`/`REPAIR_PLAN_VERSION` across membrane-blueprint/src and
//! membrane-runtime/src produced no match). Ported behavior: ordered,
//! non-destructive repair-plan builder driven by doctor `reasons` blocker
//! codes (rebuild graph before regenerating docs, reconcile watcher gaps
//! before trusting freshness); a no-op action is emitted when nothing needs
//! repair.

pub const REPAIR_PLAN_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct RepairAction {
    pub id: String,
    pub kind: String,
    pub command: Option<String>,
    pub reversible: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepairPlan {
    pub schema_version: u32,
    pub root: String,
    pub graph_state: String,
    pub actions: Vec<RepairAction>,
}

#[derive(Debug, Clone)]
pub struct DoctorReason {
    pub code: String,
    pub severity: String,
}

fn action(id: &str, kind: &str, command: Option<String>, reversible: bool, reason: &str) -> RepairAction {
    RepairAction {
        id: id.to_string(),
        kind: kind.to_string(),
        command,
        reversible,
        reason: reason.to_string(),
    }
}

/// Mirrors `buildRepairPlan({ root, outDir, graphState, reasons })`.
pub fn build_repair_plan(
    root: &str,
    out_dir: &str,
    graph_state: &str,
    reasons: &[DoctorReason],
) -> RepairPlan {
    let mut actions = Vec::new();
    let blockers: Vec<&DoctorReason> = reasons.iter().filter(|r| r.severity == "blocker").collect();
    let missing_map = blockers.iter().any(|r| r.code == "missing_map");
    let corrupt_map = blockers.iter().any(|r| r.code == "corrupt_map");
    let stale_graph = blockers
        .iter()
        .any(|r| r.code == "stale_graph" || r.code == "manifest_generation_mismatch");
    let event_gap = blockers.iter().any(|r| r.code == "event_gap");
    let corrupt_store = blockers
        .iter()
        .any(|r| r.code == "corrupt_understanding" || r.code == "corrupt_portable_manifest");
    let missing_docs = blockers
        .iter()
        .any(|r| r.code == "missing_human_docs" || r.code == "stale_human_docs");

    if missing_map || corrupt_map || corrupt_store {
        actions.push(action(
            "rebuild-graph",
            "command",
            Some(format!("blueprint build --out {out_dir}")),
            false,
            "graph artifacts missing or corrupt",
        ));
    } else if stale_graph {
        actions.push(action(
            "rebuild-graph",
            "command",
            Some(format!("blueprint build --out {out_dir}")),
            false,
            "graph stale relative to source",
        ));
    }
    if event_gap {
        actions.push(action(
            "reconcile-watcher",
            "command",
            Some(format!("blueprint-watch reconcile --root {root}")),
            true,
            "watcher continuity not proven",
        ));
    }
    if missing_docs {
        actions.push(action(
            "regenerate-docs",
            "command",
            Some(format!("blueprint build --out {out_dir} --no-readme-link")),
            true,
            "generated docs missing or stale",
        ));
    }
    if actions.is_empty() {
        actions.push(action(
            "no-op",
            "command",
            None,
            true,
            "no repair actions required",
        ));
    }
    RepairPlan {
        schema_version: REPAIR_PLAN_VERSION,
        root: root.to_string(),
        graph_state: graph_state.to_string(),
        actions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(code: &str) -> DoctorReason {
        DoctorReason {
            code: code.to_string(),
            severity: "blocker".to_string(),
        }
    }

    #[test]
    fn no_blockers_yields_no_op() {
        let plan = build_repair_plan("/repo", ".agent", "ready", &[]);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].id, "no-op");
        assert!(plan.actions[0].command.is_none());
    }

    #[test]
    fn missing_map_triggers_rebuild_before_anything_else() {
        let plan = build_repair_plan(
            "/repo",
            ".agent",
            "broken",
            &[reason("missing_map"), reason("missing_human_docs")],
        );
        let ids: Vec<_> = plan.actions.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["rebuild-graph", "regenerate-docs"]);
        assert_eq!(
            plan.actions[0].command.as_deref(),
            Some("blueprint build --out .agent")
        );
        assert!(!plan.actions[0].reversible);
    }

    #[test]
    fn event_gap_triggers_reversible_reconcile_action() {
        let plan = build_repair_plan("/repo", ".agent", "degraded", &[reason("event_gap")]);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].id, "reconcile-watcher");
        assert!(plan.actions[0].reversible);
        assert_eq!(
            plan.actions[0].command.as_deref(),
            Some("blueprint-watch reconcile --root /repo")
        );
    }

    #[test]
    fn stale_graph_alone_triggers_rebuild() {
        let plan = build_repair_plan("/repo", ".agent", "stale", &[reason("stale_graph")]);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].id, "rebuild-graph");
    }
}
