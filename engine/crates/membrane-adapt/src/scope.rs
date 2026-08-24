//! Fail-closed scope normalization and matching (canon §5.4).
//!
//! The Python oracle dropped unknown scope-dimension keys and degraded to
//! "unqualified", which could WIDEN applicability. That is a known defect the
//! native port intentionally corrects: malformed or unknown narrowing
//! dimensions quarantine the candidate; they are never silently dropped.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Canonical scope dimension keys. Adding a key is a schema change.
pub const SCOPE_DIMENSION_KEYS: &[&str] = &[
    "user",
    "org",
    "repo",
    "path_prefix",
    "package",
    "language",
    "framework",
    "task_family",
    "artifact_type",
    "model",
    "client",
    "environment",
    "risk_class",
    "branch",
];

pub const MAX_DIMENSION_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeError {
    UnknownDimension { key: String },
    EmptyDimensionValue { key: String },
    DimensionTooLong { key: String, len: usize },
    EmptyKey,
}

impl std::fmt::Display for ScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScopeError::UnknownDimension { key } => write!(f, "unknown scope dimension: {key}"),
            ScopeError::EmptyDimensionValue { key } => write!(f, "empty value for scope dimension: {key}"),
            ScopeError::DimensionTooLong { key, len } => {
                write!(f, "scope dimension {key} exceeds {MAX_DIMENSION_CHARS} chars ({len})")
            }
            ScopeError::EmptyKey => write!(f, "empty scope dimension key"),
        }
    }
}

impl std::error::Error for ScopeError {}

/// Normalized, ordered (by key) set of declared narrowing dimensions.
/// Absent/empty means unqualified — matches every context. A record only ever
/// gets NARROWER by declaring dimensions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeDimensions(BTreeMap<String, String>);

impl ScopeDimensions {
    /// Fail-closed normalization. Unknown keys, empty keys, empty values, and
    /// over-long values are ERRORS, not silent drops.
    pub fn normalize(raw: &BTreeMap<String, String>) -> Result<Self, ScopeError> {
        let mut out = BTreeMap::new();
        for (key, value) in raw {
            let key_norm = key.trim().to_lowercase();
            if key_norm.is_empty() {
                return Err(ScopeError::EmptyKey);
            }
            if !SCOPE_DIMENSION_KEYS.contains(&key_norm.as_str()) {
                return Err(ScopeError::UnknownDimension { key: key_norm });
            }
            let value_norm = value.trim().to_string();
            if value_norm.is_empty() {
                return Err(ScopeError::EmptyDimensionValue { key: key_norm });
            }
            if value_norm.chars().count() > MAX_DIMENSION_CHARS {
                return Err(ScopeError::DimensionTooLong {
                    len: value_norm.chars().count(),
                    key: key_norm,
                });
            }
            out.insert(key_norm, value_norm);
        }
        Ok(ScopeDimensions(out))
    }

    pub fn is_unqualified(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.0.iter()
    }

    /// Fail-closed matching against a context.
    ///
    /// * Unqualified records match everything (historical corpus semantics).
    /// * Every declared dimension must be satisfied. A dimension the context
    ///   cannot speak to (missing in context) is a NON-match — silently
    ///   applying a narrowed rule is exactly the failure this prevents.
    /// * `path_prefix` matches by normalized prefix; everything else matches
    ///   case-insensitively and exactly.
    pub fn matches(&self, context: &ScopeDimensions) -> bool {
        if self.is_unqualified() {
            return true;
        }
        for (key, wanted) in &self.0 {
            let Some(actual) = context.get(key) else {
                return false;
            };
            let norm = |s: &str| s.replace('\\', "/").to_lowercase();
            if key == "path_prefix" {
                let w = norm(wanted);
                let a = norm(actual);
                if !a.starts_with(&w) {
                    return false;
                }
            } else if !actual.eq_ignore_ascii_case(wanted) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dims(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn unknown_dimension_is_an_error_not_a_drop() {
        let err = ScopeDimensions::normalize(&dims(&[("colour", "red")])).unwrap_err();
        assert_eq!(
            err,
            ScopeError::UnknownDimension { key: "colour".into() }
        );
    }

    #[test]
    fn empty_value_is_an_error() {
        let err = ScopeDimensions::normalize(&dims(&[("repo", "")])).unwrap_err();
        assert!(matches!(err, ScopeError::EmptyDimensionValue { .. }));
    }

    #[test]
    fn normalization_is_order_independent_and_lowercased_in_key() {
        let a = ScopeDimensions::normalize(&dims(&[("Repo", "x"), ("language", "rust")])).unwrap();
        let b = ScopeDimensions::normalize(&dims(&[("language", "rust"), ("repo", "x")])).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn narrowed_record_requires_context_dimension_fail_closed() {
        let record =
            ScopeDimensions::normalize(&dims(&[("language", "rust")])).unwrap();
        // Context that cannot speak to language does NOT match.
        let empty_ctx = ScopeDimensions::default();
        assert!(!record.matches(&empty_ctx));
        let rust_ctx = ScopeDimensions::normalize(&dims(&[("language", "Rust")])).unwrap();
        assert!(record.matches(&rust_ctx));
        let py_ctx = ScopeDimensions::normalize(&dims(&[("language", "python")])).unwrap();
        assert!(!record.matches(&py_ctx));
    }

    #[test]
    fn path_prefix_matches_normalized_prefixes() {
        let record = ScopeDimensions::normalize(&dims(&[("path_prefix", "Engine\\Src")])).unwrap();
        let ctx = ScopeDimensions::normalize(&dims(&[("path_prefix", "engine/src/adapters")])).unwrap();
        assert!(record.matches(&ctx));
        let outside = ScopeDimensions::normalize(&dims(&[("path_prefix", "docs")])).unwrap();
        assert!(!record.matches(&outside));
    }

    #[test]
    fn unqualified_matches_everything() {
        let record = ScopeDimensions::default();
        let ctx = ScopeDimensions::normalize(&dims(&[("repo", "anything")])).unwrap();
        assert!(record.matches(&ctx));
    }
}
