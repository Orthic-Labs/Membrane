use membrane_adapt::benchmark::{portable_case_from_value, run_benchmark};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn corpus_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../adapt/eval/n4_heldout/v1")
        .join(name)
}

fn load(name: &str) -> Vec<membrane_adapt::benchmark::LabelledCase> {
    let body = std::fs::read_to_string(corpus_path(name)).expect("real corpus split checked in");
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).expect("valid JSONL row");
            portable_case_from_value(&value).expect("sealed portable case")
        })
        .collect()
}

#[test]
fn real_dev_corpus_is_integrity_bound_and_benchmarkable() {
    let cases = load("dev.jsonl");
    assert_eq!(cases.len(), 8);
    let families: BTreeSet<_> = cases
        .iter()
        .flat_map(|case| case.expected_families.iter().cloned())
        .collect();
    assert_eq!(families.len(), 8);

    let report = run_benchmark(&cases);
    println!("real-dev report_digest={}", report.report_digest);
    for (family, score) in &report.by_family {
        println!(
            "{family}: tp={} fp={} fn={} precision={:.3} recall={:.3}",
            score.true_positives,
            score.false_positives,
            score.false_negatives,
            score.precision(),
            score.recall()
        );
    }
    assert_eq!(report.corpus_size, 8);
    assert_eq!(report.by_family.len(), 8);
}

#[test]
fn real_heldout_corpus_parses_without_executing_detectors() {
    // Heldout integrity may be checked at any time. Detector execution stays
    // owner-gated so no tuning decision can observe heldout outcomes early.
    let cases = load("heldout.jsonl");
    assert_eq!(cases.len(), 8);
    assert!(cases.iter().all(|case| case.expected_families.len() == 1));
}
