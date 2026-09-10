//! Parity test for `blueprint/src/lib/rules/parser.mjs`.

use membrane_blueprint::lib_rules_parser::{parse_rules, RULES_VERSION};

#[test]
fn version_constant_matches_legacy() {
    assert_eq!(RULES_VERSION, 1);
}

#[test]
fn full_rule_with_all_sections_parses_correctly() {
    let yaml = r#"
version: 1
- id: no-ui-to-db
  from:
    path: "src/ui/**"
  disallow:
    to:
      path: "src/db/**"
    edgeKinds: [imports, calls]
  severity: error
  rationale: "keep ui/db decoupled"
"#;
    let parsed = parse_rules(yaml);
    assert_eq!(parsed.rules.len(), 1);
    let rule = &parsed.rules[0];
    assert_eq!(rule.id, "no-ui-to-db");
    assert_eq!(rule.from.path.as_deref(), Some("src/ui/**"));
    assert_eq!(
        rule.disallow.to.as_ref().unwrap().path.as_deref(),
        Some("src/db/**")
    );
    assert_eq!(
        rule.disallow.edge_kinds.as_ref().unwrap(),
        &vec!["imports".to_string(), "calls".to_string()]
    );
    assert_eq!(rule.rationale.as_deref(), Some("keep ui/db decoupled"));
}

#[test]
fn default_severity_is_error_when_unspecified() {
    let yaml = "- id: a\n";
    let parsed = parse_rules(yaml);
    assert_eq!(parsed.rules[0].severity, "error");
}

#[test]
fn snake_case_edge_kinds_alias_is_supported() {
    let yaml = "- id: a\n  disallow:\n    edge_kinds: [imports]\n";
    let parsed = parse_rules(yaml);
    assert_eq!(
        parsed.rules[0].disallow.edge_kinds,
        Some(vec!["imports".to_string()])
    );
}

#[test]
fn exceptions_section_absorbs_every_remaining_line_in_the_rule() {
    // Once `exceptions:` opens, every following line for that rule is
    // skipped -- including a `severity:` line -- since nothing re-enters a
    // recognized section within the same rule; only starting a new `- id:`
    // rule resets the section. This mirrors the legacy line-based state
    // machine exactly (it has no indentation awareness and no explicit
    // "end of exceptions" marker).
    let yaml = "- id: a\n  exceptions:\n    - owner: x\n  severity: warning\n";
    let parsed = parse_rules(yaml);
    assert_eq!(parsed.rules.len(), 1);
    assert_eq!(parsed.rules[0].severity, "error");
}

#[test]
fn a_new_rule_after_exceptions_gets_a_fresh_section_state() {
    let yaml = "- id: a\n  exceptions:\n    - owner: x\n- id: b\n  severity: warning\n";
    let parsed = parse_rules(yaml);
    assert_eq!(parsed.rules.len(), 2);
    assert_eq!(parsed.rules[0].severity, "error");
    assert_eq!(parsed.rules[1].severity, "warning");
}
