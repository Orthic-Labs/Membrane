//! Registry-to-clap parity reporting and deterministic source generation.

use membrane_protocol::operations::OperationSpec;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const PARSER_INVENTORY_QUERY: &str = "";
const GENERATED_BANNER: &str = "// GENERATED — DO NOT EDIT";

/// Drift between the canonical operation registry and a CLI parser snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliParityReport {
    pub total_ops: usize,
    pub reachable: usize,
    pub missing: Vec<String>,
    pub extra: Vec<String>,
    pub flag_drift: Vec<(String, String, String)>,
}

impl CliParityReport {
    pub fn is_drift_free(&self) -> bool {
        self.reachable == self.total_ops
            && self.missing.is_empty()
            && self.extra.is_empty()
            && self.flag_drift.is_empty()
    }
}

/// Convert a registry id into its stable command-line spelling.
pub fn cli_subcommand_name(id: &str) -> String {
    id.to_lowercase().replace('.', "-")
}

/// Compare registry operations with a parser projection.
///
/// The parser is queried once per operation name. It returns the declared
/// `(long flag, default)` pairs or `None` when the subcommand is absent. An
/// empty-name query returns the parser's subcommand inventory as `(name, _)`
/// pairs; this sentinel is necessary because a name-indexed callback otherwise
/// cannot expose parser-only subcommands for `extra` reporting.
pub fn check_parity(
    operation_specs: &[OperationSpec],
    parser: impl Fn(&str) -> Option<Vec<(String, String)>>,
) -> CliParityReport {
    let expected_names: BTreeSet<String> = operation_specs
        .iter()
        .map(|spec| cli_subcommand_name(spec.id))
        .collect();
    let extra = parser(PARSER_INVENTORY_QUERY)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| !expected_names.contains(name))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let mut report = CliParityReport {
        total_ops: operation_specs.len(),
        reachable: 0,
        missing: Vec::new(),
        extra,
        flag_drift: Vec::new(),
    };

    for spec in operation_specs {
        let command_name = cli_subcommand_name(spec.id);
        let Some(actual_pairs) = parser(&command_name) else {
            report.missing.push(command_name);
            continue;
        };
        report.reachable += 1;

        let expected: BTreeMap<String, String> = spec
            .parameters
            .iter()
            .map(|parameter| {
                (
                    format!("--{}", parameter.name),
                    parameter.default.unwrap_or_default().to_string(),
                )
            })
            .collect();
        let actual: BTreeMap<String, String> = actual_pairs.into_iter().collect();

        for (flag, expected_default) in &expected {
            match actual.get(flag) {
                Some(actual_default) if actual_default == expected_default => {}
                Some(actual_default) => report.flag_drift.push((
                    command_name.clone(),
                    render_flag(flag, expected_default),
                    render_flag(flag, actual_default),
                )),
                None => report.flag_drift.push((
                    command_name.clone(),
                    render_flag(flag, expected_default),
                    "<missing>".to_string(),
                )),
            }
        }
        for (flag, actual_default) in &actual {
            if !expected.contains_key(flag) {
                report.flag_drift.push((
                    command_name.clone(),
                    "<none>".to_string(),
                    render_flag(flag, actual_default),
                ));
            }
        }
    }

    report
}

fn render_flag(flag: &str, default: &str) -> String {
    if default.is_empty() {
        flag.to_string()
    } else {
        format!("{flag}={default}")
    }
}

/// Emit the deterministic parser-snapshot match body for the registry.
pub fn generate_cli_subcommands(operations: &[OperationSpec]) -> String {
    let ids: Vec<&str> = operations.iter().map(|spec| spec.id).collect();
    let canonical_ids = serde_json::to_string(&ids).expect("operation ids serialize");
    let mut hasher = Sha256::new();
    hasher.update(canonical_ids.as_bytes());
    let digest = format!("sha256:{}", hex::encode(hasher.finalize()));

    let mut source =
        format!("{GENERATED_BANNER}\n// operation_registry_version: {digest}\nmatch name {{\n");
    source.push_str("    \"\" => Some(vec![\n");
    for spec in operations {
        source.push_str(&format!(
            "        ({:?}.to_string(), String::new()),\n",
            cli_subcommand_name(spec.id)
        ));
    }
    source.push_str("    ]),\n");

    for spec in operations {
        source.push_str(&format!(
            "    {:?} => Some(vec![\n",
            cli_subcommand_name(spec.id)
        ));
        for parameter in spec.parameters {
            source.push_str(&format!(
                "        ({:?}.to_string(), {:?}.to_string()),\n",
                format!("--{}", parameter.name),
                parameter.default.unwrap_or_default()
            ));
        }
        source.push_str("    ]),\n");
    }
    source.push_str("    _ => None,\n}\n");
    source
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::operations::OperationParameter;

    const LIMIT: OperationParameter = OperationParameter {
        name: "limit",
        default: Some("10"),
        help: "Maximum result count.",
    };
    const ALPHA: OperationSpec = OperationSpec {
        id: "Alpha.Read",
        help: "Read alpha.",
        parameters: &[LIMIT],
    };
    const BETA: OperationSpec = OperationSpec {
        id: "beta.write",
        help: "Write beta.",
        parameters: &[],
    };

    #[test]
    fn empty_input_has_an_empty_report() {
        let report = check_parity(&[], |_| None);
        assert_eq!(report.total_ops, 0);
        assert_eq!(report.reachable, 0);
        assert!(report.is_drift_free());
    }

    #[test]
    fn matching_specs_and_parser_are_drift_free() {
        let report = check_parity(&[ALPHA], |name| match name {
            "" => Some(vec![("alpha-read".into(), String::new())]),
            "alpha-read" => Some(vec![("--limit".into(), "10".into())]),
            _ => None,
        });
        assert!(report.is_drift_free(), "{report:#?}");
    }

    #[test]
    fn missing_operation_is_recorded() {
        let report = check_parity(&[ALPHA], |_| None);
        assert_eq!(report.missing, ["alpha-read"]);
        assert_eq!(report.reachable, 0);
    }

    #[test]
    fn parser_only_operation_is_extra() {
        let report = check_parity(&[ALPHA], |name| match name {
            "" => Some(vec![
                ("alpha-read".into(), String::new()),
                ("orphan".into(), String::new()),
            ]),
            "alpha-read" => Some(vec![("--limit".into(), "10".into())]),
            _ => None,
        });
        assert_eq!(report.extra, ["orphan"]);
    }

    #[test]
    fn flag_name_drift_is_recorded() {
        let report = check_parity(&[ALPHA], |name| match name {
            "" => Some(vec![("alpha-read".into(), String::new())]),
            "alpha-read" => Some(vec![("--count".into(), "10".into())]),
            _ => None,
        });
        assert_eq!(report.flag_drift.len(), 2);
        // `render_flag` includes the default value, so the expected rendering
        // of `--limit` with default `10` is `--limit=10`. The drift row is
        // `(command_name, expected_flag, actual_flag)`.
        assert!(report.flag_drift.iter().any(|row| row.1 == "--limit=10"));
    }

    #[test]
    fn generated_source_has_banner_version_and_every_arm() {
        let source = generate_cli_subcommands(&[ALPHA, BETA]);
        assert!(source.starts_with(GENERATED_BANNER));
        assert!(source.contains("// operation_registry_version: sha256:"));
        assert!(source.contains("\"alpha-read\" => Some"));
        assert!(source.contains("\"beta-write\" => Some"));
    }
}
