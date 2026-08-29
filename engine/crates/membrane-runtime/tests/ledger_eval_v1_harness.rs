//! Ledger eval-v1 promotion harness (LEDGER-MARKDOWN-INDEXING-AND-DOCUMENT-NAVIGATION-CANON.md
//! section 9/11/12/19). Indexes the corpus's real source documents through the production
//! Ledger sync/index path (`ledger::doc_spine::sync`), then evaluates every case in
//! `tests/fixtures/ledger-eval-v1/cases/{dev,heldout}.jsonl` against BOTH recall arms
//! (`legacy_scan`, `ledger_fts`) via `ledger::doc_spine::recall_shadow`, which always computes
//! both lanes regardless of the activation mode persisted in the database.
//!
//! Run with:
//!   cargo test --manifest-path engine/Cargo.toml -p membrane-runtime --test ledger_eval_v1_harness -- --nocapture
//!
//! Evaluation hygiene (canon section 12.1, binding): `dev` may be used to compare/tune; a
//! separate test below runs `heldout` exactly once, prints its numbers, and applies the frozen
//! promotion decision rule. No parameter in this file is a function of anything observed from
//! `heldout` — it exists to report, not to search.

use membrane_runtime::ledger::{doc_spine, LedgerDb};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Target {
    document: String,
    #[allow(dead_code)]
    heading: Option<String>,
    #[allow(dead_code)]
    quote: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Expected {
    status: String,
    match_mode: String,
    targets: Vec<Target>,
    #[allow(dead_code)]
    plausible_distractors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    split: String,
    case_type: String,
    query: String,
    expected: Expected,
}

fn repo_root() -> PathBuf {
    // engine/crates/membrane-runtime -> engine/crates -> engine -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ledger-eval-v1")
}

fn load_cases(split: &str) -> Vec<Case> {
    let path = corpus_dir().join("cases").join(format!("{split}.jsonl"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("parse case: {error}\n{line}")))
        .collect()
}

/// Deterministic, order-stable digest over both the corpus manifest and the case files actually
/// evaluated, so a reported result can be tied back to exactly what was run.
fn corpus_digest() -> String {
    let mut hasher = Sha256::new();
    let mut files = vec![corpus_dir().join("manifest.json")];
    for split in ["train", "dev", "heldout"] {
        files.push(corpus_dir().join("cases").join(format!("{split}.jsonl")));
    }
    for file in files {
        let bytes = std::fs::read(&file).unwrap_or_else(|error| panic!("read {}: {error}", file.display()));
        hasher.update(file.file_name().unwrap().to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(&bytes);
        hasher.update(b"\0");
    }
    hex::encode(hasher.finalize())
}

fn normalize_doc_path(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/").to_string()
}

/// Extract the repo-relative document path from a `doc://repo/worktree/<path>` source_ref.
fn hit_document_path(source_ref: &str) -> Option<String> {
    source_ref
        .strip_prefix("doc://repo/worktree/")
        .map(normalize_doc_path)
}

#[derive(Default, Debug, Clone)]
struct ArmStats {
    recall_at_1: usize,
    recall_at_5: usize,
    reciprocal_rank_sum: f64,
    total: usize,
    per_category: BTreeMap<String, (usize, usize, usize)>, // (hits@1, hits@5, total)
}

impl ArmStats {
    fn record(&mut self, category: &str, rank: Option<usize>) {
        self.total += 1;
        let entry = self.per_category.entry(category.to_owned()).or_default();
        entry.2 += 1;
        if let Some(rank) = rank {
            // rank is 1-based
            self.reciprocal_rank_sum += 1.0 / rank as f64;
            if rank <= 1 {
                self.recall_at_1 += 1;
                entry.0 += 1;
            }
            if rank <= 5 {
                self.recall_at_5 += 1;
                entry.1 += 1;
            }
        }
    }

    fn mrr(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.reciprocal_rank_sum / self.total as f64
        }
    }

    fn recall_at_1_pct(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.recall_at_1 as f64 / self.total as f64
        }
    }

    fn recall_at_5_pct(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.recall_at_5 as f64 / self.total as f64
        }
    }

    fn print(&self, label: &str) {
        println!(
            "  {label}: n={} recall@1={:.3} recall@5={:.3} mrr={:.4}",
            self.total,
            self.recall_at_1_pct(),
            self.recall_at_5_pct(),
            self.mrr()
        );
        for (category, (hit1, hit5, total)) in &self.per_category {
            println!(
                "    - {category}: n={total} recall@1={:.3} recall@5={:.3}",
                *hit1 as f64 / *total as f64,
                *hit5 as f64 / *total as f64
            );
        }
    }
}

/// Rank (1-based) of the first hit whose document path satisfies this case's expected targets,
/// or None if no hit within the searched window satisfies it. `no_match` cases are satisfied by
/// an empty hit list (correct abstention) and scored as rank 1 in that situation; a `no_match`
/// case that returns any hit is scored as a miss (rank None) since it produced a false positive.
fn score_case(expected: &Expected, hits: &[doc_spine::DocRecallHitV1]) -> Option<usize> {
    let target_paths: Vec<String> = expected
        .targets
        .iter()
        .map(|t| normalize_doc_path(&t.document))
        .collect();

    match expected.status.as_str() {
        "no_match" => {
            if hits.is_empty() {
                Some(1)
            } else {
                None
            }
        }
        "match" | "relocation" => match expected.match_mode.as_str() {
            "all_of" => {
                // All targets must appear somewhere in the returned hit set; rank credited as
                // the position of the last-satisfied target.
                let mut last_rank = 0usize;
                for target in &target_paths {
                    let rank = hits
                        .iter()
                        .position(|hit| hit_document_path(&hit.source_ref).as_deref() == Some(target.as_str()))
                        .map(|index| index + 1);
                    match rank {
                        Some(rank) => last_rank = last_rank.max(rank),
                        None => return None,
                    }
                }
                Some(last_rank)
            }
            _ => hits
                .iter()
                .position(|hit| {
                    target_paths
                        .iter()
                        .any(|target| hit_document_path(&hit.source_ref).as_deref() == Some(target.as_str()))
                })
                .map(|index| index + 1),
        },
        other => panic!("unknown expected.status: {other}"),
    }
}

fn run_split(db: &LedgerDb, split: &str) -> (ArmStats, ArmStats) {
    let cases = load_cases(split);
    assert!(!cases.is_empty(), "split {split} has no cases");
    let mut legacy = ArmStats::default();
    let mut fts = ArmStats::default();
    for case in &cases {
        assert_eq!(case.split, split, "case {} has wrong split label", case.id);
        let shadow = doc_spine::recall_shadow(db, &case.query, 5)
            .unwrap_or_else(|error| panic!("recall_shadow failed for {}: {error}", case.id));
        let legacy_rank = score_case(&case.expected, &shadow.legacy_hits);
        let fts_rank = score_case(&case.expected, &shadow.fts_hits);
        legacy.record(&case.case_type, legacy_rank);
        fts.record(&case.case_type, fts_rank);
    }
    (legacy, fts)
}

fn index_corpus_documents(db: &LedgerDb) {
    let root = repo_root();
    let report = doc_spine::sync(db, &root).expect("ledger sync over repo root failed");
    assert!(
        report.registered > 0,
        "ledger sync registered no documents from {}",
        root.display()
    );
    eprintln!(
        "indexed repo via production sync: scanned={} registered={} parsed={} skipped={}",
        report.scanned, report.registered, report.parsed, report.skipped
    );
}

/// Dev-split comparison. This is the ONLY test in this file permitted to be run repeatedly while
/// iterating on BM25 weights/normalization; heldout is exercised exactly once by the test below.
#[test]
fn ledger_eval_v1_dev_split_both_arms() {
    let db = LedgerDb::open_in_memory();
    index_corpus_documents(&db);
    println!("corpus_digest={}", corpus_digest());
    let (legacy, fts) = run_split(&db, "dev");
    println!("dev split — legacy_scan:");
    legacy.print("legacy_scan");
    println!("dev split — ledger_fts:");
    fts.print("ledger_fts");
    // No hard assertion here beyond "the harness produced numbers" — dev is for tuning/reporting,
    // not a pass/fail gate. Sanity floor: harness must have actually matched something.
    assert!(legacy.recall_at_5 + fts.recall_at_5 > 0, "both arms scored zero on dev — harness likely miswired");
}

/// Held-out promotion run. Frozen configuration (no field weights or normalization changed after
/// this file was written against dev numbers). Run exactly once per candidate per canon 12.1;
/// re-running this test re-executes the same frozen code path, it does not re-tune anything.
#[test]
fn ledger_eval_v1_heldout_split_both_arms() {
    let db = LedgerDb::open_in_memory();
    index_corpus_documents(&db);
    println!("corpus_digest={}", corpus_digest());
    let (legacy, fts) = run_split(&db, "heldout");
    println!("heldout split — legacy_scan:");
    legacy.print("legacy_scan");
    println!("heldout split — ledger_fts:");
    fts.print("ledger_fts");

    // Promotion decision rule (frozen, from the qualification task): ledger_fts activates only
    // if held-out MRR >= legacy_scan MRR and ledger_fts recall@5 is not materially worse
    // (defined here as: not more than 2 percentage points below legacy_scan recall@5).
    let mrr_ok = fts.mrr() >= legacy.mrr();
    let recall5_ok = fts.recall_at_5_pct() >= legacy.recall_at_5_pct() - 0.02;
    let promote = mrr_ok && recall5_ok;
    println!(
        "PROMOTION DECISION: promote_ledger_fts={promote} (mrr_ok={mrr_ok} recall5_ok={recall5_ok}) legacy_mrr={:.4} fts_mrr={:.4} legacy_recall5={:.3} fts_recall5={:.3}",
        legacy.mrr(),
        fts.mrr(),
        legacy.recall_at_5_pct(),
        fts.recall_at_5_pct()
    );
}
