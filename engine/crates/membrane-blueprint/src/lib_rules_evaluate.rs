//! Native Rust port of `blueprint/src/lib/rules/evaluate.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `evaluateRules`/`pathMatches` across membrane-blueprint/src and
//! membrane-runtime/src produced no match; the shared provenance tag
//! `FACT_PROVENANCE_RULE_RESOLVED` already exists in `evidence_authority.rs`
//! and is reused here rather than redefined). Ported behavior: deterministic
//! local rule evaluation over graph edges — each match becomes a finding
//! carrying a stable fingerprint, cited evidence, and an explicit
//! `authority: "declaration"` tag; findings are never treated as graph
//! writes.

use crate::evidence_authority::FACT_PROVENANCE_RULE_RESOLVED;
use crate::lib_rules_parser::Rule;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub confidence_tier: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuleFinding {
    pub rule_id: String,
    pub rule_version: String,
    pub state: String,
    pub severity: String,
    pub fingerprint: String,
    pub source: String,
    pub target: String,
    pub evidence_path: Vec<String>,
    pub generation_id: Option<String>,
    pub confidence_tier: Option<String>,
    pub message: String,
    pub remediation: String,
    pub authority: &'static str,
    pub provenance: &'static str,
}

/// Mirrors `pathMatches(pattern, path)`: `**` matches any depth, `*` matches
/// within a path segment, `None`/empty pattern matches everything.
pub fn path_matches(pattern: Option<&str>, path: &str) -> bool {
    let Some(pattern) = pattern else { return true };
    if pattern.is_empty() {
        return true;
    }
    if pattern == path {
        return true;
    }
    let mut regex_src = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    regex_src.push_str(".*");
                } else {
                    regex_src.push_str("[^/]*");
                }
            }
            c if ".+^${}()|[]\\".contains(c) => {
                regex_src.push('\\');
                regex_src.push(c);
            }
            c => regex_src.push(c),
        }
    }
    regex_src.push('$');
    regex::Regex::new(&regex_src)
        .map(|re| re.is_match(path))
        .unwrap_or(false)
}

/// Mirrors `evaluateRules({ rules, edges, nodes, providerVersions, generationId })`.
pub fn evaluate_rules(
    rules: &[Rule],
    edges: &[GraphEdge],
    provider_versions: &HashMap<String, String>,
    generation_id: Option<&str>,
) -> Vec<RuleFinding> {
    let mut findings = Vec::new();
    for rule in rules {
        let rule_version = provider_versions
            .get(&rule.id)
            .cloned()
            .unwrap_or_else(|| "1.0.0".to_string());
        for edge in edges {
            let from_matches = path_matches(rule.from.path.as_deref(), &edge.source);
            let to_path = rule.disallow.to.as_ref().and_then(|t| t.path.as_deref());
            let to_matches = path_matches(to_path, &edge.target);
            let kind_matches = match &rule.disallow.edge_kinds {
                Some(kinds) if !kinds.is_empty() => kinds.contains(&edge.kind),
                _ => true,
            };
            if !from_matches || !to_matches || !kind_matches {
                continue;
            }
            let edge_kind_version = provider_versions.get(&edge.kind).cloned().unwrap_or_default();
            let mut hasher = Sha256::new();
            hasher.update(format!(
                "{}:{}:{}:{}:{}:{}",
                rule.id, rule_version, edge.source, edge.kind, edge.target, edge_kind_version
            ));
            let fingerprint = hex::encode(hasher.finalize())[..16].to_string();
            findings.push(RuleFinding {
                rule_id: rule.id.clone(),
                rule_version: rule_version.clone(),
                state: "violation".to_string(),
                severity: rule.severity.clone(),
                fingerprint,
                source: edge.source.clone(),
                target: edge.target.clone(),
                evidence_path: vec![edge.source.clone(), edge.target.clone()],
                generation_id: generation_id.map(|s| s.to_string()),
                confidence_tier: edge.confidence_tier.clone(),
                message: format!(
                    "rule {} violated by {} -> {} ({})",
                    rule.id, edge.source, edge.target, edge.kind
                ),
                remediation: rule.rationale.clone().unwrap_or_default(),
                authority: "declaration",
                provenance: FACT_PROVENANCE_RULE_RESOLVED,
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lib_rules_parser::{RuleDisallow, RuleDisallowTo, RuleFrom};

    fn edge(source: &str, target: &str, kind: &str) -> GraphEdge {
        GraphEdge {
            source: source.to_string(),
            target: target.to_string(),
            kind: kind.to_string(),
            confidence_tier: None,
        }
    }

    #[test]
    fn glob_path_matching_handles_double_and_single_star() {
        assert!(path_matches(Some("src/ui/**"), "src/ui/a/b.rs"));
        assert!(path_matches(Some("src/ui/*"), "src/ui/a.rs"));
        assert!(!path_matches(Some("src/ui/*"), "src/ui/a/b.rs"));
        assert!(path_matches(None, "anything"));
    }

    #[test]
    fn evaluate_rules_matches_and_fingerprints_deterministically() {
        let rule = Rule {
            id: "no-ui-to-db".to_string(),
            from: RuleFrom {
                path: Some("src/ui/**".to_string()),
            },
            disallow: RuleDisallow {
                to: Some(RuleDisallowTo {
                    path: Some("src/db/**".to_string()),
                }),
                min_confidence: None,
                edge_kinds: Some(vec!["imports".to_string()]),
            },
            severity: "error".to_string(),
            rationale: Some("no direct db access from ui".to_string()),
        };
        let edges = vec![
            edge("src/ui/widget.rs", "src/db/store.rs", "imports"),
            edge("src/ui/widget.rs", "src/db/store.rs", "calls"),
            edge("src/other/thing.rs", "src/db/store.rs", "imports"),
        ];
        let versions = HashMap::new();
        let findings = evaluate_rules(&[rule], &edges, &versions, Some("gen-1"));
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.rule_id, "no-ui-to-db");
        assert_eq!(f.source, "src/ui/widget.rs");
        assert_eq!(f.target, "src/db/store.rs");
        assert_eq!(f.authority, "declaration");
        assert_eq!(f.provenance, FACT_PROVENANCE_RULE_RESOLVED);
        assert_eq!(f.fingerprint.len(), 16);

        // Determinism: identical inputs produce an identical fingerprint.
        let findings2 = evaluate_rules(&[rule_clone()], &edges, &versions, Some("gen-1"));
        assert_eq!(findings[0].fingerprint, findings2[0].fingerprint);
    }

    fn rule_clone() -> Rule {
        Rule {
            id: "no-ui-to-db".to_string(),
            from: RuleFrom {
                path: Some("src/ui/**".to_string()),
            },
            disallow: RuleDisallow {
                to: Some(RuleDisallowTo {
                    path: Some("src/db/**".to_string()),
                }),
                min_confidence: None,
                edge_kinds: Some(vec!["imports".to_string()]),
            },
            severity: "error".to_string(),
            rationale: Some("no direct db access from ui".to_string()),
        }
    }
}
