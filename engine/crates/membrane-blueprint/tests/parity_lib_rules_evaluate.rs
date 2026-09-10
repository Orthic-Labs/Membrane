//! Parity test for `blueprint/src/lib/rules/evaluate.mjs`.

use membrane_blueprint::evidence_authority::FACT_PROVENANCE_RULE_RESOLVED;
use membrane_blueprint::lib_rules_evaluate::{evaluate_rules, path_matches, GraphEdge};
use membrane_blueprint::lib_rules_parser::{Rule, RuleDisallow, RuleDisallowTo, RuleFrom};
use std::collections::HashMap;

fn rule() -> Rule {
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
        rationale: Some("ui must not import db".to_string()),
    }
}

#[test]
fn path_matches_supports_double_star_single_star_and_exact() {
    assert!(path_matches(Some("a/b"), "a/b"));
    assert!(path_matches(Some("a/**"), "a/b/c/d.rs"));
    assert!(path_matches(Some("a/*"), "a/b.rs"));
    assert!(!path_matches(Some("a/*"), "a/b/c.rs"));
    assert!(path_matches(Some(""), "anything"));
}

#[test]
fn matching_edge_produces_a_declaration_tagged_finding() {
    let edges = vec![GraphEdge {
        source: "src/ui/panel.rs".to_string(),
        target: "src/db/conn.rs".to_string(),
        kind: "imports".to_string(),
        confidence_tier: Some("Exact".to_string()),
    }];
    let findings = evaluate_rules(&[rule()], &edges, &HashMap::new(), Some("gen-9"));
    assert_eq!(findings.len(), 1);
    let f = &findings[0];
    assert_eq!(f.state, "violation");
    assert_eq!(f.authority, "declaration");
    assert_eq!(f.provenance, FACT_PROVENANCE_RULE_RESOLVED);
    assert_eq!(f.generation_id.as_deref(), Some("gen-9"));
    assert_eq!(f.evidence_path, vec!["src/ui/panel.rs".to_string(), "src/db/conn.rs".to_string()]);
}

#[test]
fn edge_kind_not_in_disallow_list_does_not_match() {
    let edges = vec![GraphEdge {
        source: "src/ui/panel.rs".to_string(),
        target: "src/db/conn.rs".to_string(),
        kind: "re-exports".to_string(),
        confidence_tier: None,
    }];
    let findings = evaluate_rules(&[rule()], &edges, &HashMap::new(), None);
    assert!(findings.is_empty());
}

#[test]
fn fingerprint_is_deterministic_given_identical_inputs() {
    let edges = vec![GraphEdge {
        source: "src/ui/panel.rs".to_string(),
        target: "src/db/conn.rs".to_string(),
        kind: "imports".to_string(),
        confidence_tier: None,
    }];
    let a = evaluate_rules(&[rule()], &edges, &HashMap::new(), Some("gen-1"));
    let b = evaluate_rules(&[rule()], &edges, &HashMap::new(), Some("gen-1"));
    assert_eq!(a[0].fingerprint, b[0].fingerprint);
    assert_eq!(a[0].fingerprint.len(), 16);
}

#[test]
fn provider_version_for_the_rule_flows_into_the_fingerprint() {
    let edges = vec![GraphEdge {
        source: "src/ui/panel.rs".to_string(),
        target: "src/db/conn.rs".to_string(),
        kind: "imports".to_string(),
        confidence_tier: None,
    }];
    let mut versions = HashMap::new();
    versions.insert("no-ui-to-db".to_string(), "2.0.0".to_string());
    let default_version = evaluate_rules(&[rule()], &edges, &HashMap::new(), None);
    let bumped_version = evaluate_rules(&[rule()], &edges, &versions, None);
    assert_eq!(default_version[0].rule_version, "1.0.0");
    assert_eq!(bumped_version[0].rule_version, "2.0.0");
    assert_ne!(default_version[0].fingerprint, bumped_version[0].fingerprint);
}
