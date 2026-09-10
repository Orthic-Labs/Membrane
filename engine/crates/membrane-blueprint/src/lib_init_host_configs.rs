//! Native Rust port of `blueprint/src/lib/init/host-configs.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `HOOK_POLICY`/`policy_behavior`/`recall-before-read` across
//! membrane-blueprint/src and membrane-runtime/src produced no match).
//! Ported behavior: three hook policy modes (advisory,
//! recall-before-read, task-grants), each with an explicit fail-open/closed
//! flag and recovery command; unknown modes fall back to `advisory`.

pub const HOOK_POLICY_MODES: [&str; 3] = ["advisory", "recall-before-read", "task-grants"];

pub struct HookPolicy {
    pub fail_closed: bool,
    pub description: &'static str,
}

pub fn hook_policy(mode: &str) -> HookPolicy {
    match mode {
        "recall-before-read" => HookPolicy {
            fail_closed: true,
            description: "Reads are denied until a recall receipt exists for the session.",
        },
        "task-grants" => HookPolicy {
            fail_closed: true,
            description: "Widened paths require a task-scoped grant.",
        },
        _ => HookPolicy {
            fail_closed: false,
            description: "Hooks only advise; reads are never blocked.",
        },
    }
}

pub struct PolicyBehavior {
    pub mode: String,
    pub fail_closed: bool,
    pub recovery_command: Option<String>,
}

/// Mirrors `policyBehavior(mode)`: unknown modes behave like `advisory`
/// (fail-open, no recovery command).
pub fn policy_behavior(mode: &str) -> PolicyBehavior {
    let policy = hook_policy(mode);
    let recovery_command = match mode {
        "recall-before-read" => Some("blueprint recall --session <id>".to_string()),
        "task-grants" => Some("blueprint grant issue --task <id> --paths <glob>".to_string()),
        _ => None,
    };
    PolicyBehavior {
        mode: mode.to_string(),
        fail_closed: policy.fail_closed,
        recovery_command,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advisory_is_fail_open_with_no_recovery_command() {
        let behavior = policy_behavior("advisory");
        assert!(!behavior.fail_closed);
        assert_eq!(behavior.recovery_command, None);
    }

    #[test]
    fn recall_before_read_is_fail_closed_with_recovery_command() {
        let behavior = policy_behavior("recall-before-read");
        assert!(behavior.fail_closed);
        assert_eq!(
            behavior.recovery_command,
            Some("blueprint recall --session <id>".to_string())
        );
    }

    #[test]
    fn task_grants_is_fail_closed_with_recovery_command() {
        let behavior = policy_behavior("task-grants");
        assert!(behavior.fail_closed);
        assert_eq!(
            behavior.recovery_command,
            Some("blueprint grant issue --task <id> --paths <glob>".to_string())
        );
    }

    #[test]
    fn unknown_mode_falls_back_to_advisory_behavior() {
        let behavior = policy_behavior("bogus");
        assert!(!behavior.fail_closed);
        assert_eq!(behavior.recovery_command, None);
    }
}
