//! Parity test for `blueprint/src/lib/init/plan.mjs`.

use membrane_blueprint::lib_init_plan::{build_init_plan, BuildInitPlanInput, InitPlanError, INIT_PLAN_VERSION};
use std::path::Path;

#[test]
fn schema_version_matches_legacy_constant() {
    assert_eq!(INIT_PLAN_VERSION, 1);
}

#[test]
fn claude_code_host_plan_enables_mcp_and_watch_and_lists_expected_actions() {
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
    assert_eq!(plan.schema_version, 1);
    assert_eq!(plan.hosts, vec!["claude-code".to_string()]);
    assert!(plan.mcp_enabled);
    assert!(plan.watch_enabled);
    let ids: Vec<_> = plan.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "write-host-instructions-claude-code",
            "install-mcp",
            "build-generation",
            "enroll-watch",
        ]
    );
    assert_eq!(plan.uninstall_command, "blueprint uninstall --root /repo");
}

#[test]
fn generic_host_plan_skips_mcp_but_still_builds() {
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
    assert!(plan.actions.iter().any(|a| a.id == "build-generation"));
    assert!(!plan.actions.iter().any(|a| a.id == "install-mcp"));
}

#[test]
fn user_scope_disables_watch_by_default() {
    let root = Path::new("/repo");
    let plan = build_init_plan(
        BuildInitPlanInput {
            root,
            host: "generic",
            scope: "user",
            ..Default::default()
        },
        |_| false,
        |_| false,
    )
    .unwrap();
    assert!(!plan.watch_enabled);
}

#[test]
fn invalid_scope_is_rejected() {
    let root = Path::new("/repo");
    let err = build_init_plan(
        BuildInitPlanInput {
            root,
            scope: "team",
            ..Default::default()
        },
        |_| false,
        |_| false,
    )
    .unwrap_err();
    assert!(matches!(err, InitPlanError::InvalidScope));
}
