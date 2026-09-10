//! Parity test for `blueprint/src/lib/init/host-configs.mjs`.

use membrane_blueprint::lib_init_host_configs::{policy_behavior, HOOK_POLICY_MODES};

#[test]
fn hook_policy_modes_match_legacy_fixed_set() {
    assert_eq!(
        HOOK_POLICY_MODES,
        ["advisory", "recall-before-read", "task-grants"]
    );
}

#[test]
fn advisory_is_fail_open_with_no_recovery_command() {
    let behavior = policy_behavior("advisory");
    assert!(!behavior.fail_closed);
    assert_eq!(behavior.recovery_command, None);
}

#[test]
fn recall_before_read_is_fail_closed_with_its_recovery_command() {
    let behavior = policy_behavior("recall-before-read");
    assert!(behavior.fail_closed);
    assert_eq!(
        behavior.recovery_command.as_deref(),
        Some("blueprint recall --session <id>")
    );
}

#[test]
fn task_grants_is_fail_closed_with_its_recovery_command() {
    let behavior = policy_behavior("task-grants");
    assert!(behavior.fail_closed);
    assert_eq!(
        behavior.recovery_command.as_deref(),
        Some("blueprint grant issue --task <id> --paths <glob>")
    );
}

#[test]
fn unknown_mode_behaves_like_advisory() {
    let behavior = policy_behavior("nonexistent-mode");
    assert!(!behavior.fail_closed);
    assert_eq!(behavior.recovery_command, None);
}
