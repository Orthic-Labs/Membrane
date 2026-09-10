//! Parity test for `blueprint/src/lib/operations/repair.mjs`.

use membrane_blueprint::lib_operations_repair::{build_repair_plan, DoctorReason, REPAIR_PLAN_VERSION};

fn blocker(code: &str) -> DoctorReason {
    DoctorReason {
        code: code.to_string(),
        severity: "blocker".to_string(),
    }
}

#[test]
fn schema_version_matches_legacy_constant() {
    assert_eq!(REPAIR_PLAN_VERSION, 1);
}

#[test]
fn nothing_to_repair_yields_a_single_no_op_action() {
    let plan = build_repair_plan("/repo", ".agent", "ready", &[]);
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].id, "no-op");
}

#[test]
fn corrupt_understanding_and_missing_docs_are_ordered_rebuild_then_docs() {
    let plan = build_repair_plan(
        "/repo",
        ".agent",
        "corrupt",
        &[blocker("corrupt_understanding"), blocker("stale_human_docs")],
    );
    let ids: Vec<_> = plan.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec!["rebuild-graph", "regenerate-docs"]);
}

#[test]
fn manifest_generation_mismatch_triggers_rebuild_like_stale_graph() {
    let plan = build_repair_plan(
        "/repo",
        ".agent",
        "stale",
        &[blocker("manifest_generation_mismatch")],
    );
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].id, "rebuild-graph");
}

#[test]
fn non_blocker_reasons_are_ignored() {
    let warning = DoctorReason {
        code: "missing_map".to_string(),
        severity: "warning".to_string(),
    };
    let plan = build_repair_plan("/repo", ".agent", "degraded", &[warning]);
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].id, "no-op");
}
