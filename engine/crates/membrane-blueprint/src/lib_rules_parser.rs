//! Native Rust port of `blueprint/src/lib/rules/parser.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `parseRules`/`parse_rules`/`RULES_VERSION` across membrane-blueprint/src
//! and membrane-runtime/src produced no match). Ported behavior: minimal
//! deterministic YAML-subset parser for the pinned architecture rule DSL
//! (`- id:`, `from:`/`disallow:`/`to:` sections, `path`, `severity`,
//! `rationale`, `minConfidence`, `edgeKinds`/`edge_kinds`).

pub const RULES_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleFrom {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleDisallowTo {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleDisallow {
    pub to: Option<RuleDisallowTo>,
    pub min_confidence: Option<String>,
    pub edge_kinds: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: String,
    pub from: RuleFrom,
    pub disallow: RuleDisallow,
    pub severity: String,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleSet {
    pub version: u32,
    pub rules: Vec<Rule>,
}

#[derive(Default)]
struct InProgressRule {
    id: String,
    from: RuleFrom,
    disallow: RuleDisallow,
    severity: String,
    rationale: Option<String>,
}

fn strip_quotes(value: &str) -> String {
    let v = value.trim();
    if (v.starts_with('"') && v.ends_with('"') && v.len() >= 2)
        || (v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2)
    {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

/// Mirrors `parseRules(yaml)`.
pub fn parse_rules(yaml: &str) -> RuleSet {
    let mut rules = Vec::new();
    let mut current: Option<InProgressRule> = None;
    let mut section: Option<&'static str> = None;

    macro_rules! push {
        () => {
            if let Some(c) = current.take() {
                if !c.id.is_empty() {
                    rules.push(Rule {
                        id: c.id,
                        from: c.from,
                        disallow: c.disallow,
                        severity: c.severity,
                        rationale: c.rationale,
                    });
                }
            }
        };
    }

    for raw in yaml.split(['\n']) {
        let line = raw.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("version:") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("- id:") {
            push!();
            section = None;
            current = Some(InProgressRule {
                id: rest.trim().to_string(),
                severity: "error".to_string(),
                ..Default::default()
            });
            continue;
        }
        let Some(c) = current.as_mut() else { continue };
        if line.starts_with("exceptions:") {
            section = Some("exceptions");
            continue;
        }
        if section == Some("exceptions") {
            continue;
        }
        if line == "from:" {
            section = Some("from");
            continue;
        }
        if line == "disallow:" {
            section = Some("disallow");
            continue;
        }
        if line == "to:" {
            section = Some("to");
            continue;
        }
        let Some(idx) = line.find(':') else { continue };
        let key = line[..idx].trim();
        if !key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
            || key.is_empty()
        {
            continue;
        }
        let value = strip_quotes(&line[idx + 1..]);
        match key {
            "severity" => c.severity = value,
            "rationale" => c.rationale = Some(value),
            "path" => {
                if section == Some("from") {
                    c.from.path = Some(value.clone());
                }
                c.disallow.to.get_or_insert_with(Default::default).path = Some(value);
            }
            "minConfidence" => c.disallow.min_confidence = Some(value),
            "edgeKinds" | "edge_kinds" => {
                let trimmed = value.trim_start_matches('[').trim_end_matches(']');
                c.disallow.edge_kinds = Some(
                    trimmed
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                );
            }
            _ => {}
        }
    }
    push!();
    RuleSet {
        version: RULES_VERSION,
        rules,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_rule() {
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
  rationale: "UI must not import DB internals"
"#;
        let parsed = parse_rules(yaml);
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.rules.len(), 1);
        let rule = &parsed.rules[0];
        assert_eq!(rule.id, "no-ui-to-db");
        assert_eq!(rule.from.path.as_deref(), Some("src/ui/**"));
        assert_eq!(
            rule.disallow.to.as_ref().unwrap().path.as_deref(),
            Some("src/db/**")
        );
        assert_eq!(
            rule.disallow.edge_kinds,
            Some(vec!["imports".to_string(), "calls".to_string()])
        );
        assert_eq!(rule.severity, "error");
        assert_eq!(rule.rationale.as_deref(), Some("UI must not import DB internals"));
    }

    #[test]
    fn multiple_rules_are_each_collected() {
        let yaml = "- id: a\n  severity: warning\n- id: b\n  severity: error\n";
        let parsed = parse_rules(yaml);
        assert_eq!(parsed.rules.len(), 2);
        assert_eq!(parsed.rules[0].id, "a");
        assert_eq!(parsed.rules[0].severity, "warning");
        assert_eq!(parsed.rules[1].id, "b");
        assert_eq!(parsed.rules[1].severity, "error");
    }

    #[test]
    fn empty_input_yields_no_rules() {
        let parsed = parse_rules("");
        assert!(parsed.rules.is_empty());
        assert_eq!(parsed.version, RULES_VERSION);
    }
}
