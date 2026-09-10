//! Native port of `blueprint/src/graph/conventions.mjs`.
//!
//! Descriptive convention mining only: file naming style, test placement,
//! and common top-level directories. Counterexamples are first-class and
//! `policy_authority` is always false — this module makes no rules, it only
//! reports weak observed evidence.

use regex::Regex;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct ConventionFile {
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct WeakEvidence {
    pub kind: &'static str,
    pub evidence_class: &'static str,
    pub claim: String,
    pub support: usize,
    pub total: usize,
    pub coverage: f64,
    pub examples: Vec<String>,
    pub counterexamples: Vec<String>,
    pub policy_authority: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectConventions {
    pub schema_version: u32,
    pub kind: &'static str,
    pub evidence_class: &'static str,
    pub policy_authority: bool,
    pub evidence: Vec<WeakEvidence>,
}

pub struct DetectOptions {
    pub minimum_examples: usize,
    pub minimum_coverage: f64,
}

impl Default for DetectOptions {
    fn default() -> Self {
        Self { minimum_examples: 3, minimum_coverage: 0.75 }
    }
}

fn normalize_path(value: &str) -> String {
    let replaced = value.replace('\\', "/");
    replaced.strip_prefix("./").map(str::to_string).unwrap_or(replaced)
}

fn kebab_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z][a-z0-9]*(?:-[a-z0-9]+)+$").unwrap())
}
fn snake_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z][a-z0-9]*(?:_[a-z0-9]+)+$").unwrap())
}
fn camel_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z][A-Za-z0-9]*$").unwrap())
}
fn has_upper_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[A-Z]").unwrap())
}
fn pascal_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Z][A-Za-z0-9]*$").unwrap())
}
fn has_ext_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\.[A-Za-z0-9]+$").unwrap())
}
fn test_dir_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(^|/)(?:tests?|__tests__)(/|$)").unwrap())
}
fn test_suffix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(?:^|[._-])(?:test|spec)\.[^.]+$").unwrap())
}

fn naming_style(name: &str) -> &'static str {
    let stem = match name.rfind('.') {
        Some(idx) if idx > 0 => &name[..idx],
        _ => name,
    };
    if kebab_re().is_match(stem) {
        return "kebab-case";
    }
    if snake_re().is_match(stem) {
        return "snake_case";
    }
    if camel_re().is_match(stem) && has_upper_re().is_match(stem) {
        return "camelCase";
    }
    if pascal_re().is_match(stem) {
        return "PascalCase";
    }
    "other"
}

fn is_test_path(path: &str) -> bool {
    test_dir_re().is_match(path) || test_suffix_re().is_match(path)
}

fn weak_evidence(kind: &'static str, claim: String, support: usize, total: usize, examples: &[String], counterexamples: &[String]) -> WeakEvidence {
    let coverage = if total > 0 { support as f64 / total as f64 } else { 0.0 };
    WeakEvidence {
        kind,
        evidence_class: "WeakEvidence",
        claim,
        support,
        total,
        coverage,
        examples: examples.iter().take(8).cloned().collect(),
        counterexamples: counterexamples.iter().take(8).cloned().collect(),
        policy_authority: false,
    }
}

/// Descriptive convention mining only. Counterexamples are first-class.
pub fn detect_project_conventions(files: &[ConventionFile], options: &DetectOptions) -> ProjectConventions {
    let paths: Vec<String> = files
        .iter()
        .map(|file| normalize_path(&file.path))
        .filter(|path| !path.is_empty())
        .collect();

    let mut evidence = Vec::new();

    // Production filename conventions and test-placement conventions are distinct
    // populations. Mixing *.test/spec paths into ordinary source naming makes the
    // inferred production style depend on test framework suffixes rather than the
    // repository's source convention.
    let source_paths: Vec<&String> = paths
        .iter()
        .filter(|path| has_ext_re().is_match(path) && !is_test_path(path))
        .collect();

    let mut styles: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for path in &source_paths {
        let name = path.rsplit('/').next().unwrap_or(path.as_str());
        let style = naming_style(name);
        styles.entry(style).or_default().push((*path).clone());
    }
    let mut ranked_styles: Vec<(&'static str, Vec<String>)> = styles.into_iter().collect();
    ranked_styles.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(b.0)));

    if let Some((style, examples)) = ranked_styles.first() {
        if examples.len() >= options.minimum_examples {
            let counterexamples: Vec<String> = ranked_styles[1..].iter().flat_map(|(_, rows)| rows.clone()).collect();
            let row = weak_evidence(
                "file_naming",
                format!("Most source files use {style}."),
                examples.len(),
                source_paths.len(),
                examples,
                &counterexamples,
            );
            if row.coverage >= options.minimum_coverage {
                evidence.push(row);
            }
        }
    }

    let test_paths: Vec<&String> = paths.iter().filter(|path| is_test_path(path)).collect();
    if test_paths.len() >= options.minimum_examples {
        let directory_tests: Vec<String> = test_paths.iter().filter(|path| test_dir_re().is_match(path)).map(|p| (*p).clone()).collect();
        let colocated: Vec<String> = test_paths.iter().filter(|path| !directory_tests.contains(*path)).map(|p| (*p).clone()).collect();
        let use_directory = directory_tests.len() >= colocated.len();
        let (preferred, counter, mode) = if use_directory {
            (directory_tests, colocated, "dedicated test directories")
        } else {
            (colocated, directory_tests, "co-located test/spec files")
        };
        let row = weak_evidence(
            "test_placement",
            format!("Tests are usually placed in {mode}."),
            preferred.len(),
            test_paths.len(),
            &preferred,
            &counter,
        );
        if row.coverage >= options.minimum_coverage {
            evidence.push(row);
        }
    }

    let mut top_directories: BTreeMap<String, usize> = BTreeMap::new();
    for path in &paths {
        let first = if path.contains('/') { path.split('/').next().unwrap_or(".") } else { "." };
        *top_directories.entry(first.to_string()).or_insert(0) += 1;
    }
    let mut common_dirs: Vec<(String, usize)> = top_directories.into_iter().filter(|(_, count)| *count >= options.minimum_examples).collect();
    common_dirs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if !common_dirs.is_empty() {
        let total_count: usize = common_dirs.iter().map(|(_, count)| count).sum();
        let examples: Vec<String> = common_dirs.iter().take(8).map(|(dir, count)| format!("{dir}:{count}")).collect();
        let names: Vec<String> = common_dirs.iter().take(5).map(|(dir, _)| dir.clone()).collect();
        evidence.push(weak_evidence(
            "module_layout",
            format!("Common top-level code areas: {}.", names.join(", ")),
            total_count,
            paths.len(),
            &examples,
            &[],
        ));
    }

    ProjectConventions {
        schema_version: 1,
        kind: "project-conventions",
        evidence_class: "WeakEvidence",
        policy_authority: false,
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> ConventionFile {
        ConventionFile { path: path.to_string() }
    }

    #[test]
    fn conventions_are_weak_descriptive_evidence_and_retain_counterexamples() {
        let files = vec![
            file("src/foo-bar.ts"),
            file("src/baz-qux.ts"),
            file("src/other-name.ts"),
            file("src/not_style.ts"),
            file("tests/a.test.ts"),
            file("tests/b.test.ts"),
            file("tests/c.test.ts"),
            file("src/d.spec.ts"),
        ];
        let result = detect_project_conventions(&files, &DetectOptions { minimum_examples: 3, minimum_coverage: 0.6 });
        assert_eq!(result.evidence_class, "WeakEvidence");
        assert!(!result.policy_authority);
        let naming = result.evidence.iter().find(|row| row.kind == "file_naming").expect("file_naming evidence");
        assert!(naming.counterexamples.iter().any(|path| path.contains("not_style")));
        assert!(!naming.policy_authority);
    }

    #[test]
    fn naming_style_classification_matches_legacy_rules() {
        assert_eq!(naming_style("foo-bar.ts"), "kebab-case");
        assert_eq!(naming_style("foo_bar.ts"), "snake_case");
        assert_eq!(naming_style("fooBar.ts"), "camelCase");
        assert_eq!(naming_style("FooBar.ts"), "PascalCase");
        assert_eq!(naming_style("foo.ts"), "other");
    }

    #[test]
    fn test_placement_detection_matches_legacy_rules() {
        assert!(is_test_path("tests/a.test.ts"));
        assert!(is_test_path("src/d.spec.ts"));
        assert!(!is_test_path("src/foo-bar.ts"));
    }
}
