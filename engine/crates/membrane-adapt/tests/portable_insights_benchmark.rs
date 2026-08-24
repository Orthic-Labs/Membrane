use membrane_adapt::benchmark::{portable_case_from_value, run_benchmark};

#[test]
fn sealed_portable_corpus_meets_every_family_gate() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../adapt/eval/insights_bench/v1/cases.jsonl");
    let body = std::fs::read_to_string(path).expect("portable corpus must be checked in");
    let cases: Vec<_> = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).expect("valid corpus JSONL");
            portable_case_from_value(&value).expect("sealed portable case")
        })
        .collect();
    let report = run_benchmark(&cases);
    assert_eq!(report.corpus_size, 70);
    assert_eq!(report.by_family.len(), 33);
    for (family, score) in report.by_family {
        assert!(score.true_positives >= 1, "{family} lacks a positive hit");
        assert_eq!(score.false_positives, 0, "{family} false positive");
        assert_eq!(score.false_negatives, 0, "{family} false negative");
        assert!(score.precision() >= 0.95, "{family} precision below gate");
    }
}
