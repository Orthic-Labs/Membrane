//! Parity tests for `conventions::detect_project_conventions`, ported from
//! the legacy `blueprint/src/graph/conventions.mjs` behavior exercised by
//! `blueprint/tests/framework-process-contracts.test.mjs`'s
//! "conventions are weak descriptive evidence and retain counterexamples"
//! case:
//!
//! - convention mining is purely descriptive: `evidenceClass` is always
//!   `WeakEvidence` and `policyAuthority` is always false, at both the
//!   top level and on every individual evidence row;
//! - the dominant file-naming style is reported with its counterexamples
//!   intact, never silently discarded;
//! - test files (directory- or suffix-based) are excluded from the
//!   production naming population so test-framework suffixes never skew
//!   the inferred source convention.

use membrane_blueprint::conventions::{detect_project_conventions, ConventionFile, DetectOptions};

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

    let naming = result.evidence.iter().find(|row| row.kind == "file_naming").expect("file_naming evidence row present");
    assert_eq!(naming.evidence_class, "WeakEvidence");
    assert!(!naming.policy_authority);
    assert!(naming.claim.contains("kebab-case"));
    assert!(naming.counterexamples.iter().any(|path| path.contains("not_style")));
    // The three kebab-case files must count as examples, not the snake_case outlier.
    assert!(naming.examples.iter().all(|path| !path.contains("not_style")));
}

#[test]
fn test_placement_prefers_the_larger_population_and_records_the_smaller_as_counterexample() {
    let files = vec![
        file("tests/a.test.ts"),
        file("tests/b.test.ts"),
        file("tests/c.test.ts"),
        file("tests/d.test.ts"),
        file("src/e.spec.ts"),
    ];
    let result = detect_project_conventions(&files, &DetectOptions { minimum_examples: 3, minimum_coverage: 0.5 });
    let placement = result.evidence.iter().find(|row| row.kind == "test_placement").expect("test_placement evidence row present");
    assert!(placement.claim.contains("dedicated test directories"));
    assert!(placement.counterexamples.iter().any(|path| path.contains("e.spec.ts")));
}

#[test]
fn module_layout_lists_common_top_level_directories_without_counterexamples() {
    let files = vec![
        file("src/a.ts"),
        file("src/b.ts"),
        file("src/c.ts"),
        file("docs/a.md"),
        file("docs/b.md"),
        file("docs/c.md"),
    ];
    let result = detect_project_conventions(&files, &DetectOptions { minimum_examples: 3, minimum_coverage: 0.5 });
    let layout = result.evidence.iter().find(|row| row.kind == "module_layout").expect("module_layout evidence row present");
    assert!(layout.claim.contains("src"));
    assert!(layout.claim.contains("docs"));
    assert!(layout.counterexamples.is_empty());
    assert!(!layout.policy_authority);
}

#[test]
fn below_minimum_examples_yields_no_evidence_row() {
    let files = vec![file("src/a.ts"), file("src/b-two.ts")];
    let result = detect_project_conventions(&files, &DetectOptions { minimum_examples: 3, minimum_coverage: 0.6 });
    assert!(result.evidence.iter().find(|row| row.kind == "file_naming").is_none());
}
