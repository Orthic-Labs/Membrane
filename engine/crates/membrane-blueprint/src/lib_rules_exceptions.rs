//! Native Rust port of `blueprint/src/lib/rules/exceptions.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `suppressedByException`/`isExceptionValid` across membrane-blueprint/src
//! and membrane-runtime/src produced no match). Ported behavior: an
//! exception is valid only with owner/rationale/expires/ruleId/scope all
//! present and a non-expired ISO-parseable `expires`; expired or malformed
//! exceptions never suppress findings.

pub struct RuleException {
    pub owner: Option<String>,
    pub rationale: Option<String>,
    /// Milliseconds since epoch, matching `Date.parse`.
    pub expires_ms: Option<i64>,
    pub rule_id: Option<String>,
    pub scope: Option<String>,
    pub path: Option<String>,
}

pub struct FindingRef<'a> {
    pub rule_id: &'a str,
    pub source: &'a str,
}

/// Mirrors `isExceptionValid(exception, { now })`.
pub fn is_exception_valid(exception: &RuleException, now_ms: i64) -> bool {
    let owner = exception.owner.as_deref().unwrap_or("");
    let rationale = exception.rationale.as_deref().unwrap_or("");
    let rule_id = exception.rule_id.as_deref().unwrap_or("");
    let scope = exception.scope.as_deref().unwrap_or("");
    if owner.is_empty() || rationale.is_empty() || rule_id.is_empty() || scope.is_empty() {
        return false;
    }
    match exception.expires_ms {
        Some(expiry) => expiry > now_ms,
        None => false,
    }
}

/// Mirrors `suppressedByException(finding, exceptions, { now })`.
pub fn suppressed_by_exception(
    finding: &FindingRef,
    exceptions: &[RuleException],
    now_ms: i64,
) -> bool {
    for exception in exceptions {
        if !is_exception_valid(exception, now_ms) {
            continue;
        }
        if exception.rule_id.as_deref() != Some(finding.rule_id) {
            continue;
        }
        match exception.scope.as_deref() {
            Some("global") => return true,
            Some("path") => {
                if let Some(path) = &exception.path {
                    if finding.source.starts_with(path.as_str()) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_exception(scope: &str, expires_ms: i64) -> RuleException {
        RuleException {
            owner: Some("owner".to_string()),
            rationale: Some("rationale".to_string()),
            expires_ms: Some(expires_ms),
            rule_id: Some("rule-1".to_string()),
            scope: Some(scope.to_string()),
            path: Some("src/legacy/".to_string()),
        }
    }

    #[test]
    fn missing_required_field_is_invalid() {
        let mut exception = full_exception("global", 1_000_000);
        exception.owner = None;
        assert!(!is_exception_valid(&exception, 0));
    }

    #[test]
    fn expired_exception_is_invalid() {
        let exception = full_exception("global", 100);
        assert!(!is_exception_valid(&exception, 200));
    }

    #[test]
    fn valid_exception_suppresses_global_scope() {
        let exceptions = vec![full_exception("global", 1_000_000)];
        let finding = FindingRef {
            rule_id: "rule-1",
            source: "anywhere.rs",
        };
        assert!(suppressed_by_exception(&finding, &exceptions, 0));
    }

    #[test]
    fn path_scope_only_suppresses_matching_prefix() {
        let exceptions = vec![full_exception("path", 1_000_000)];
        let matching = FindingRef {
            rule_id: "rule-1",
            source: "src/legacy/old.rs",
        };
        let non_matching = FindingRef {
            rule_id: "rule-1",
            source: "src/new/fresh.rs",
        };
        assert!(suppressed_by_exception(&matching, &exceptions, 0));
        assert!(!suppressed_by_exception(&non_matching, &exceptions, 0));
    }

    #[test]
    fn expired_exception_never_suppresses() {
        let exceptions = vec![full_exception("global", 100)];
        let finding = FindingRef {
            rule_id: "rule-1",
            source: "anywhere.rs",
        };
        assert!(!suppressed_by_exception(&finding, &exceptions, 200));
    }
}
