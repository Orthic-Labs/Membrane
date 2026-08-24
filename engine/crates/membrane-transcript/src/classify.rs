//! Class-priority admission classification (plan 5.2 port).

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Class-priority admission classes. Lower number = higher priority; the cap
/// squeezes only the lowest class (`SuccessfulReadonly`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    UnresolvedFailure,
    Mutation,
    FailedVerification,
    OpenUserRequest,
    DecisionOrConstraint,
    SuccessfulReadonly,
}

impl Classification {
    /// Admission priority: unresolved failures > mutations > failed
    /// verification > open user requests > decisions/constraints >
    /// successful read-only.
    pub fn priority(self) -> u8 {
        match self {
            Classification::UnresolvedFailure => 0,
            Classification::Mutation => 1,
            Classification::FailedVerification => 2,
            Classification::OpenUserRequest => 3,
            Classification::DecisionOrConstraint => 4,
            Classification::SuccessfulReadonly => 5,
        }
    }

    pub const ALL: [Classification; 6] = [
        Classification::UnresolvedFailure,
        Classification::Mutation,
        Classification::FailedVerification,
        Classification::OpenUserRequest,
        Classification::DecisionOrConstraint,
        Classification::SuccessfulReadonly,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Classification::UnresolvedFailure => "unresolved_failure",
            Classification::Mutation => "mutation",
            Classification::FailedVerification => "failed_verification",
            Classification::OpenUserRequest => "open_user_request",
            Classification::DecisionOrConstraint => "decision_or_constraint",
            Classification::SuccessfulReadonly => "successful_readonly",
        }
    }
}

/// Tool name tokens classified read-only (case-insensitive substring match).
pub const READONLY_TOOL_TOKENS: &[&str] = &[
    "read", "glob", "grep", "search", "list", "ls", "view", "fetch", "query", "describe", "get",
    "show", "inspect",
];

/// Tool name tokens that look like mutations.
pub const MUTATION_TOOL_TOKENS: &[&str] = &[
    "write",
    "edit",
    "multiedit",
    "create",
    "delete",
    "remove",
    "patch",
    "move",
    "rename",
    "deploy",
    "publish",
    "install",
    "add",
    "commit",
    "push",
    "merge",
    "applypatch",
];

/// Planning-only tools must NOT be classified as mutations.
pub const PLANNING_TOOL_TOKENS: &[&str] = &[
    "todowrite",
    "updateplan",
    "creategoal",
    "updategoal",
    "createplan",
    "updateplanv2",
    "plan",
    "create_thread",
    "sendmessage",
    "send_message",
    "forge",
    "apply_patch",
];

static FAILURE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?im)\bFAIL(?:ED)?:",
        r"(?im)\bERROR\b",
        r"(?im)\bTraceback\b",
        r"(?im)\bExit code:\s*[1-9]\d*",
        r#"(?im)\breturncode["']?\s*[:=]\s*[1-9]\d*"#,
        r"(?im)\bENOENT\b",
        r"(?im)\bEACCES\b",
        r"(?im)\bpermission denied\b",
        r"(?im)\btimeout\b",
        r"(?im)\b(?:npm|pnpm|yarn)\b.*\bERR!\b",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});

static USER_REQUEST_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?im)^\s*(?:please\s+)?(?:fix|implement|add|build|create|make|ensure)\b",
        r"(?im)^\s*(?:can|could|would)\s+you\b",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});

static DECISION_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?im)^\s*(?:decision|locked|constraint|invariant|rules?):\s*",
        r"(?im)\bnever\b.*\b(?:use|do|call|invoke)\b",
        r"(?im)\balways\b.*\b(?:use|do|call|invoke)\b",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});

static INLINE_MUTATION_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?im)\b(?:git\s+(?:add|commit|push|merge)|",
        r"Set-Content|Add-Content|New-Item|Remove-Item|",
        r"npm\s+install|pnpm\s+add|pip\s+install|deploy|publish)\b",
    ))
    .unwrap()
});

static FAILED_VERIFICATION_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?im)\b(?:verified|validated|fixed|tested)\b.*\b(?:failed|broken|wrong|",
        r"missing|not fixed|still fails)\b",
    ))
    .unwrap()
});

fn matches_any(patterns: &[Regex], text: &str) -> bool {
    patterns.iter().any(|p| p.is_match(text))
}

fn contains_token(tool_folded: &str, tokens: &[&str]) -> bool {
    tokens.iter().any(|t| tool_folded.contains(t))
}

/// Inputs needed to classify one normalized event.
#[derive(Debug, Clone, Copy)]
pub struct ClassifyInput<'a> {
    pub kind: &'a str,
    pub tool: Option<&'a str>,
    pub text: &'a str,
    pub is_error: bool,
}

/// Return one admission class for the event.
pub fn classify(input: ClassifyInput<'_>) -> Classification {
    let ClassifyInput {
        kind,
        tool,
        text,
        is_error,
    } = input;

    if kind == "tool_result" && (is_error || matches_any(&FAILURE_PATTERNS, text)) {
        return Classification::UnresolvedFailure;
    }

    if kind == "tool_call" {
        let folded = tool.unwrap_or("").to_lowercase();
        if contains_token(&folded, PLANNING_TOOL_TOKENS) {
            return Classification::DecisionOrConstraint;
        }
        if contains_token(&folded, MUTATION_TOOL_TOKENS) {
            return Classification::Mutation;
        }
        if INLINE_MUTATION_PATTERN.is_match(text) {
            return Classification::Mutation;
        }
    }

    if kind == "assistant_message" && FAILED_VERIFICATION_PATTERN.is_match(text) {
        return Classification::FailedVerification;
    }

    if kind == "user_message" && matches_any(&USER_REQUEST_PATTERNS, text) {
        return Classification::OpenUserRequest;
    }

    if (kind == "user_message" || kind == "assistant_message")
        && matches_any(&DECISION_PATTERNS, text)
    {
        return Classification::DecisionOrConstraint;
    }

    if kind == "tool_call" {
        let folded = tool.unwrap_or("").to_lowercase();
        if contains_token(&folded, READONLY_TOOL_TOKENS) {
            return Classification::SuccessfulReadonly;
        }
        return Classification::SuccessfulReadonly;
    }

    // Default fallback bucket so the cap can squeeze it (intentional).
    Classification::SuccessfulReadonly
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(kind: &str, tool: Option<&str>, text: &str, is_error: bool) -> Classification {
        classify(ClassifyInput {
            kind,
            tool,
            text,
            is_error,
        })
    }

    #[test]
    fn failing_tool_result_is_unresolved_failure() {
        assert_eq!(
            c("tool_result", None, "panic: ERROR boom", false),
            Classification::UnresolvedFailure
        );
        assert_eq!(
            c("tool_result", None, "fine output", true),
            Classification::UnresolvedFailure
        );
    }

    #[test]
    fn mutation_tools_beat_planning_exclusion_order() {
        assert_eq!(
            c("tool_call", Some("edit"), "{}", false),
            Classification::Mutation
        );
        // Planning tools explicitly excluded from mutation per plan 5.2.
        assert_eq!(
            c("tool_call", Some("TodoWrite"), "[]", false),
            Classification::DecisionOrConstraint
        );
        assert_eq!(
            c("tool_call", Some("apply_patch"), "", false),
            Classification::DecisionOrConstraint
        );
    }

    #[test]
    fn inline_shell_mutations_are_detected() {
        assert_eq!(
            c("tool_call", Some("bash"), "git push origin main", false),
            Classification::Mutation
        );
        assert_eq!(
            c("tool_call", Some("shell"), "pnpm add left-pad", false),
            Classification::Mutation
        );
    }

    #[test]
    fn read_only_tools_and_defaults_are_squeezable() {
        assert_eq!(
            c("tool_call", Some("read"), "", false),
            Classification::SuccessfulReadonly
        );
        assert_eq!(
            c("tool_call", Some("bash"), "cargo test --quiet", false),
            Classification::SuccessfulReadonly
        );
        assert_eq!(
            c("thinking", None, "", false),
            Classification::SuccessfulReadonly
        );
    }

    #[test]
    fn user_request_and_decision_patterns() {
        assert_eq!(
            c("user_message", None, "please fix the parser", false),
            Classification::OpenUserRequest
        );
        assert_eq!(
            c("user_message", None, "Never use npm install here", false),
            Classification::DecisionOrConstraint
        );
        assert_eq!(
            c(
                "assistant_message",
                None,
                "constraint: keep ids stable",
                false
            ),
            Classification::DecisionOrConstraint
        );
    }

    #[test]
    fn failed_verification_claim_detected() {
        assert_eq!(
            c(
                "assistant_message",
                None,
                "I verified the fix but tests still fail broken",
                false
            ),
            Classification::FailedVerification
        );
    }
}
