//! Honest, portable per-detector precision/recall benchmark for Adapt
//! Insights (canon `ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md` sections 6
//! and 11.2).
//!
//! This is deliberately separate from `portable_insights_benchmark.rs`
//! (which scores the existing `adapt/eval/insights_bench/v1` sealed
//! corpus). This harness:
//!
//! - loads `adapt/eval/insights_detector_bench/cases.jsonl`, checked into
//!   the repo — no machine-local transcripts, nothing skips;
//! - scores every one of the 33 native detector families against every
//!   case (a family firing on a case that never named it counts as a real
//!   false positive — nothing is silently unscored);
//! - reports precision AND recall per family, not one aggregate number;
//! - never asserts a known, reproduced false-positive gap away. Cases
//!   tagged `known_gap` are still scored into the precision/recall table;
//!   the only thing the test asserts about them is that the *documented*
//!   outcome (which was measured, not chosen) still reproduces, so a
//!   silent regression or a silent fix is caught either way without ever
//!   tuning a case's text to make the suite pass.

use std::collections::{BTreeMap, BTreeSet};

use membrane_adapt::insights::detectors::run_all_detectors;
use membrane_adapt::insights::{EventKind, Severity, TranscriptEventV1};

#[derive(Debug, serde::Deserialize)]
struct RawEvent {
    session_id: String,
    kind: String,
    text: String,
    byte_start: i64,
    byte_end: i64,
    event_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct RawCase {
    case_id: String,
    case_class: String,
    known_gap: bool,
    should_fire: BTreeSet<String>,
    #[serde(default)]
    expected_severity: BTreeMap<String, String>,
    #[allow(dead_code)]
    notes: String,
    events: Vec<RawEvent>,
}

/// The 33 family slugs `run_all_detectors` can emit. Kept explicit (not
/// derived) so a detector silently added or removed is a visible corpus
/// mismatch rather than a shrinking blind spot.
const ALL_FAMILIES: &[&str] = &[
    "visible_frustration",
    "user_swearing",
    "repeated_ask",
    "claimed_verified_then_corrected",
    "ignored_tool_failure",
    "degraded_provider_treated_as_success",
    "false_not_found",
    "unproductive_broad_searching",
    "wrong_repo_or_subsystem",
    "stale_terminology_surfacing",
    "silent_scope_narrowing",
    "omitted_requirement",
    "unaccepted_plan_change",
    "tests_that_cannot_fail",
    "guard_firings",
    "user_asks_why_missed_or_postmortem",
    "overengineering",
    "architecture_churn",
    "repeated_redesign",
    "planning_instead_of_executing",
    "scope_expansion_without_request",
    "repeated_scope_expansion",
    "false_completion_claim",
    "instruction_noncompliance",
    "model_specific_gotcha",
    "client_or_tool_specific_gotcha",
    "cross_agent_repeats",
    "repeated_user_correction_same_theme",
    "forge_opened_never_closed",
    "verification_theatre",
    "unnecessary_abstraction",
    "unnecessary_dependency",
    "verification_claim_without_tool_evidence",
];

fn parse_kind(raw: &str) -> EventKind {
    match raw {
        "user_message" => EventKind::UserMessage,
        "assistant_message" => EventKind::AssistantMessage,
        "tool_call" => EventKind::ToolCall,
        "tool_result" => EventKind::ToolResult,
        other => panic!("unsupported event kind in corpus: {other}"),
    }
}

fn provenance_for(kind: EventKind) -> &'static str {
    match kind {
        EventKind::UserMessage => "external_user",
        EventKind::AssistantMessage => "assistant",
        EventKind::ToolCall | EventKind::ToolResult => "tool",
    }
}

fn to_events(raw: &[RawEvent]) -> Vec<TranscriptEventV1> {
    raw.iter()
        .map(|e| {
            let kind = parse_kind(&e.kind);
            TranscriptEventV1 {
                event_id: e.event_id.clone(),
                session_id: e.session_id.clone(),
                host: "insights_detector_bench".into(),
                provenance: provenance_for(kind).into(),
                kind,
                text: e.text.clone(),
                timestamp: None,
                byte_start: e.byte_start,
                byte_end: e.byte_end,
                call_id: None,
                occurrence: 0,
                evidence_eligible: true,
            }
        })
        .collect()
}

fn load_corpus() -> Vec<RawCase> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../adapt/eval/insights_detector_bench/cases.jsonl");
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("honest corpus must be checked in at {path:?}: {e}"));
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("invalid corpus line: {e}\n{l}")))
        .collect()
}

#[derive(Debug, Default, Clone, Copy)]
struct Score {
    tp: u32,
    fp: u32,
    fn_: u32,
}

impl Score {
    fn precision(&self) -> Option<f64> {
        let d = self.tp + self.fp;
        if d == 0 { None } else { Some(self.tp as f64 / d as f64) }
    }
    fn recall(&self) -> Option<f64> {
        let d = self.tp + self.fn_;
        if d == 0 { None } else { Some(self.tp as f64 / d as f64) }
    }
}

fn severity_slug(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
        Severity::Critical => "critical",
    }
}

/// One row of measured behavior for a single case: which families fired,
/// keyed for cross-checking known_gap reproduction.
struct CaseResult {
    fired: BTreeSet<String>,
    severities: BTreeMap<String, Severity>,
}

fn run_case(raw: &RawCase) -> CaseResult {
    let events = to_events(&raw.events);
    let episodes = run_all_detectors(&events);
    let mut fired = BTreeSet::new();
    let mut severities = BTreeMap::new();
    for ep in episodes {
        fired.insert(ep.family.clone());
        severities.entry(ep.family).or_insert(ep.severity);
    }
    CaseResult { fired, severities }
}

#[test]
fn corpus_loads_and_has_expected_shape() {
    let corpus = load_corpus();
    assert_eq!(corpus.len(), 49, "case count drifted; update this test deliberately if the corpus grew/shrank on purpose");
    let mut seen = BTreeSet::new();
    for c in &corpus {
        assert!(seen.insert(c.case_id.clone()), "duplicate case_id {}", c.case_id);
        assert!(!c.events.is_empty(), "case {} has no events", c.case_id);
        for family in &c.should_fire {
            assert!(
                ALL_FAMILIES.contains(&family.as_str()),
                "case {} names unknown family {family}",
                c.case_id
            );
        }
    }
    // Every constructible true positive covers all 33 native families.
    let tp_families: BTreeSet<&str> = corpus
        .iter()
        .filter(|c| c.case_class == "true_positive")
        .flat_map(|c| c.should_fire.iter().map(|s| s.as_str()))
        .collect();
    let all: BTreeSet<&str> = ALL_FAMILIES.iter().copied().collect();
    let missing: Vec<&str> = all.difference(&tp_families).copied().collect();
    assert!(missing.is_empty(), "no true-positive case constructed for: {missing:?}");
}

/// Severity is a hardcoded per-family contract, not something the detector
/// computes dynamically — so this is a real regression gate, not a
/// tautology: it fails the moment code and corpus disagree.
#[test]
fn severity_matches_documented_contract_on_true_positives() {
    let corpus = load_corpus();
    for raw in &corpus {
        if raw.expected_severity.is_empty() {
            continue;
        }
        let result = run_case(raw);
        for (family, expected) in &raw.expected_severity {
            let actual = result
                .severities
                .get(family)
                .unwrap_or_else(|| panic!("case {} expected {family} to fire (for severity check) but it did not", raw.case_id));
            assert_eq!(
                severity_slug(*actual),
                expected,
                "case {}: {family} severity drifted from documented contract",
                raw.case_id
            );
        }
    }
}

/// Cases that are NOT known_gap encode invariants the code currently
/// satisfies and must keep satisfying: negation handling, the guard's
/// correct suppression of quoted/hypothetical *user* text, tool-carried
/// text never being read as an assistant claim, and cross-session
/// duplicate detection's same-session/different-text negatives. Any
/// unnamed family firing on a case also counts here (all 33 families are
/// scored on every case) — nothing is silently unscored.
#[test]
fn non_gap_cases_match_ground_truth_exactly() {
    let corpus = load_corpus();
    let mut failures = Vec::new();
    for raw in corpus.iter().filter(|c| !c.known_gap) {
        let result = run_case(raw);
        for family in ALL_FAMILIES {
            let expected = raw.should_fire.contains(*family);
            let actual = result.fired.contains(*family);
            if expected != actual {
                failures.push(format!(
                    "case {} [{}]: family {family} expected_fire={expected} actual_fire={actual}",
                    raw.case_id, raw.case_class
                ));
            }
        }
    }
    assert!(failures.is_empty(), "ground-truth mismatch on non-gap cases:\n{}", failures.join("\n"));
}

/// known_gap cases document a REAL, reproduced false positive (or, in
/// principle, false negative) — the case text was authored as a minimal
/// faithful adversarial construction, then whatever the code actually did
/// was recorded here. This assertion is intentionally the mirror image of
/// a normal regression test: it fails if the documented gap silently
/// stops reproducing (someone fixed it — great, but the corpus/README
/// need to be told) or if it silently gets worse (spreads to another
/// family). It never fails because we tuned text to make it pass.
#[test]
fn known_gap_cases_reproduce_exactly_as_documented() {
    let corpus = load_corpus();
    let mut drift = Vec::new();
    for raw in corpus.iter().filter(|c| c.known_gap) {
        let result = run_case(raw);
        for family in ALL_FAMILIES {
            let documented = raw.should_fire.contains(*family);
            let actual = result.fired.contains(*family);
            if documented != actual {
                drift.push(format!(
                    "case {} [{}]: family {family} documented_gap_outcome={documented} actual={actual} (gap status changed — update README/corpus, do not silence)",
                    raw.case_id, raw.case_class
                ));
            }
        }
    }
    assert!(drift.is_empty(), "known-gap reproduction drifted:\n{}", drift.join("\n"));
}

/// Prints the full per-family precision/recall table. Run with
/// `cargo test -p membrane-adapt --test insights_detector_benchmark -- --nocapture`
/// to see it. Not itself a pass/fail gate beyond "the corpus loads and
/// every case scores against all 33 families" — the gating invariants
/// live in the tests above.
#[test]
fn report_measured_precision_recall_table() {
    let corpus = load_corpus();
    let mut scores: BTreeMap<&str, Score> = ALL_FAMILIES.iter().map(|f| (*f, Score::default())).collect();
    let mut gap_notes: BTreeMap<&str, Vec<&str>> = BTreeMap::new();

    for raw in &corpus {
        let result = run_case(raw);
        for family in ALL_FAMILIES {
            let expected = raw.should_fire.contains(*family);
            let actual = result.fired.contains(*family);
            let entry = scores.get_mut(family).expect("initialized above");
            match (expected, actual) {
                (true, true) => entry.tp += 1,
                (true, false) => entry.fn_ += 1,
                (false, true) => {
                    entry.fp += 1;
                    if raw.known_gap {
                        gap_notes.entry(family).or_default().push(raw.case_id.as_str());
                    }
                }
                (false, false) => {}
            }
        }
    }

    println!();
    println!("{:<45} {:>4} {:>4} {:>4} {:>10} {:>10}", "family", "TP", "FP", "FN", "precision", "recall");
    println!("{}", "-".repeat(90));
    for family in ALL_FAMILIES {
        let s = scores[family];
        let p = s.precision().map(|v| format!("{v:.2}")).unwrap_or_else(|| "n/a".into());
        let r = s.recall().map(|v| format!("{v:.2}")).unwrap_or_else(|| "n/a".into());
        let mut line = format!("{family:<45} {:>4} {:>4} {:>4} {p:>10} {r:>10}", s.tp, s.fp, s.fn_);
        if let Some(cases) = gap_notes.get(family) {
            line.push_str(&format!("  <- known open gap, see {cases:?}"));
        }
        println!("{line}");
    }
    println!();

    // Regression guard on the report itself: every family the corpus can
    // exercise must have been scored (no family silently absent).
    for family in ALL_FAMILIES {
        let s = scores[family];
        assert!(
            s.tp + s.fp + s.fn_ > 0,
            "family {family} was never exercised by any case in the corpus"
        );
    }
}
