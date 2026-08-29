//! Cortex relevance spot-check qualification harness.
//!
//! Run with:
//!   rightkit cargo test -p membrane-runtime --test cortex_relevance_spotcheck -- --nocapture
//!
//! The default ignored test opens `CORTEX_DB`, prefers real production recall-log
//! queries, falls back to recent parsed user-session messages, and writes one atomic
//! JSON report. It is ignored in ordinary CI because it requires the installed real
//! embedder and local evaluation data. Unit-level fixture coverage below is deterministic.

use cortex_core::{MemoryEntry, MemoryTier};
use membrane_runtime::cortex_relevance_spotcheck::{
    calibrate_thresholds, judge_hits, run_spotcheck, write_report_atomic,
    RelevanceSpotcheckReportV1, VerdictV1, DEFAULT_SAMPLE_SIZE,
};
use std::path::{Path, PathBuf};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cortex-relevance-spotcheck-v1/queries.json")
}

fn fixture_entry(id: &str, content: &str, score: f64) -> MemoryEntry {
    MemoryEntry {
        id: id.into(),
        tier: MemoryTier::Semantic,
        content: content.into(),
        keywords: Vec::new(),
        score,
        created_at: "2026-08-29T00:00:00Z".into(),
        access_count: 0,
        embedding: None,
        scope_id: "global".into(),
    }
}

#[test]
fn frozen_fixture_judge_exercises_strict_useful_and_irrelevant_states() {
    let thresholds = calibrate_thresholds(&[0.10, 0.20, 0.30, 0.40, 0.50], "fixture-current-space")
        .expect("calibrate fixture thresholds");
    let relevant = vec![(
        fixture_entry(
            "global/replay",
            "Cortex durable memory replay validates recall ranking and relevance",
            1.0,
        ),
        0.50,
    )];
    let partial = vec![(
        fixture_entry("global/backup", "Cortex backup procedure", 1.0),
        0.30,
    )];
    let irrelevant = vec![(
        fixture_entry("global/marketing", "homepage typography and brand colors", 1.0),
        0.10,
    )];
    assert_eq!(
        judge_hits("validate Cortex durable memory replay relevance", &relevant, &thresholds),
        VerdictV1::Relevant
    );
    assert_eq!(
        judge_hits("review Cortex backup restore integrity", &partial, &thresholds),
        VerdictV1::Partial
    );
    assert_eq!(
        judge_hits("validate Cortex durable memory replay relevance", &irrelevant, &thresholds),
        VerdictV1::Irrelevant
    );
}

#[test]
fn frozen_queries_are_redacted_real_session_excerpts() {
    let text = std::fs::read_to_string(fixture_path()).expect("read spot-check fixture");
    let value: serde_json::Value = serde_json::from_str(&text).expect("parse spot-check fixture");
    let queries = value["queries"].as_array().expect("queries array");
    assert!(queries.len() >= 10);
    for row in queries {
        let query = row["query"].as_str().expect("query string");
        assert!(query.chars().count() >= 10);
        assert!(!query.contains("sk-"));
        assert!(!query.contains("ghp_"));
    }
}

#[test]
#[ignore = "requires CORTEX_DB, local real queries, and the installed real embedder"]
fn cortex_relevance_spotcheck_real_queries() {
    let db = std::env::var_os("CORTEX_DB")
        .map(PathBuf::from)
        .expect("CORTEX_DB is required for the live spot-check");
    let output = std::env::var_os("CORTEX_RELEVANCE_SPOTCHECK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("cortex-relevance-spotcheck-v1.json"));
    let report = run_spotcheck(&db, DEFAULT_SAMPLE_SIZE, None, Some(&fixture_path())).unwrap_or_else(
        |failed_json| {
            let failed: RelevanceSpotcheckReportV1 =
                serde_json::from_str(&failed_json).expect("failed report JSON");
            write_report_atomic(&output, &failed).expect("atomic failed report write");
            panic!("live relevance spot-check failed loud: {}", failed.reason.unwrap_or_default());
        },
    );
    write_report_atomic(&output, &report).expect("atomic report write");
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    println!("report={}", output.display());
    assert!(report.ok, "{}", report.reason.unwrap_or_default());
    assert_eq!(report.schema, "cortex.relevance-spotcheck.v1");
    assert_eq!(report.variance_status, "not_measured_single_judge");
    assert_eq!(report.measurement_only.recall_rows_added, 0);
    assert_eq!(report.measurement_only.injection_counter_delta, 0);
    assert_eq!(report.measurement_only.production_injection_count, 0);
    assert!(!report.thresholds.provenance.contains("old 0.45/0.35 reused"));
}
