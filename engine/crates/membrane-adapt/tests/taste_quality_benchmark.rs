//! Native, language-neutral Taste extraction/admission conformance benchmark.
//!
//! The checked corpus is intentionally synthetic.  This test measures the
//! deterministic native path and refuses to promote that result into a claim
//! about real-world held-out traffic.

use membrane_adapt::admission::{
    evaluate_eligibility, EligibilityDecision, EligibilityInput, RuleIndex,
};
use membrane_adapt::authority::{AuthorityEffect, Origin, StoredRule};
use membrane_adapt::canonical::sha256_hex;
use membrane_adapt::record::RuleKey;
use membrane_adapt::taste::extract_candidates_with_source;
use membrane_transcript::{EventFlags, TranscriptEventV1};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const CASE_SCHEMA: &str = "adapt.taste-benchmark-case.v1";
const MANIFEST_SCHEMA: &str = "adapt.taste-benchmark-manifest.v1";
const SCORECARD_SCHEMA: &str = "adapt.taste-benchmark-scorecard.v1";
const FROZEN_SOURCE_DIGEST: &str =
    "f1c93aa760a4a9bd9e13ca385d1260f230dddb98ef8bfb514d8e21cd9328fe6d";

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: String,
    corpus_version: String,
    corpus_file: String,
    corpus_sha256: String,
    corpus_kind: String,
    labels_frozen_before_measurement: bool,
    thresholds: Thresholds,
    remaining_n4_evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
struct Thresholds {
    extraction_precision_min: f64,
    extraction_recall_min: f64,
    admission_precision_min: f64,
    admission_recall_min: f64,
    semantic_projection_precision_min: f64,
    authority_false_positive_rate_max: f64,
}

#[derive(Debug, Deserialize)]
struct Case {
    schema_version: String,
    case_id: String,
    partition: String,
    evidence_lane: String,
    input: CaseInput,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct CaseInput {
    text: String,
    kind: String,
    role: String,
    scope: String,
    classification: String,
    origin: String,
    #[serde(default)]
    flags: CaseFlags,
    #[serde(default)]
    admission_overrides: AdmissionOverrides,
}

#[derive(Debug, Default, Deserialize)]
struct CaseFlags {
    #[serde(default)]
    synthetic: bool,
    #[serde(default)]
    meta: bool,
    #[serde(default)]
    private_reasoning_omitted: bool,
    #[serde(default)]
    redacted: bool,
}

#[derive(Debug, Default, Deserialize)]
struct AdmissionOverrides {
    category: Option<String>,
    record_class: Option<String>,
    #[serde(default)]
    scope_dimensions: BTreeMap<String, String>,
    #[serde(default)]
    duplicate_existing: bool,
    #[serde(default)]
    stored_rules: Vec<StoredRule>,
}

#[derive(Debug, Deserialize)]
struct Expected {
    extract: bool,
    admit: bool,
    authority_eligible: bool,
    category: Option<String>,
    record_type: Option<String>,
    act_kind: Option<String>,
    scope: Option<String>,
    refusal_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
struct Confusion {
    true_positive: u64,
    false_positive: u64,
    false_negative: u64,
    true_negative: u64,
}

impl Confusion {
    fn observe(&mut self, expected: bool, observed: bool) {
        match (expected, observed) {
            (true, true) => self.true_positive += 1,
            (false, true) => self.false_positive += 1,
            (true, false) => self.false_negative += 1,
            (false, false) => self.true_negative += 1,
        }
    }

    fn precision(&self) -> f64 {
        ratio(self.true_positive, self.true_positive + self.false_positive)
    }

    fn recall(&self) -> f64 {
        ratio(self.true_positive, self.true_positive + self.false_negative)
    }
}

#[derive(Debug, Serialize, PartialEq)]
struct Metrics {
    extraction: Confusion,
    extraction_precision: f64,
    extraction_recall: f64,
    admission: Confusion,
    admission_precision: f64,
    admission_recall: f64,
    semantic_projection_correct: u64,
    semantic_projection_total: u64,
    semantic_projection_precision: f64,
    authority_negative_cases: u64,
    authority_false_positives: u64,
    authority_false_positive_rate: f64,
}

#[derive(Debug, Serialize, PartialEq)]
struct DiagnosticSummary {
    cases: u64,
    extracted: u64,
    admitted: u64,
    status: &'static str,
}

#[derive(Debug, Serialize, PartialEq)]
struct Scorecard {
    schema_version: &'static str,
    corpus_version: String,
    corpus_sha256: String,
    corpus_kind: String,
    implementation: &'static str,
    source_digest: &'static str,
    thresholds: Thresholds,
    metrics: Metrics,
    synthetic_conformance_gate: &'static str,
    implicit_evidence_diagnostic: DiagnosticSummary,
    n4_exit_gate: &'static str,
    remaining_n4_evidence: Vec<String>,
    case_failures: Vec<String>,
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn bench_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../adapt/eval/taste_bench/v1")
}

fn read_inputs() -> (Manifest, Vec<Case>, Vec<u8>) {
    let dir = bench_dir();
    let manifest: Manifest = serde_json::from_slice(
        &fs::read(dir.join("manifest.json")).expect("read Taste benchmark manifest"),
    )
    .expect("parse Taste benchmark manifest");
    assert_eq!(manifest.schema_version, MANIFEST_SCHEMA);
    assert_eq!(manifest.corpus_kind, "synthetic_conformance");
    assert!(manifest.labels_frozen_before_measurement);

    let bytes = fs::read(dir.join(&manifest.corpus_file)).expect("read Taste benchmark cases");
    assert_eq!(
        sha256_hex(&bytes),
        manifest.corpus_sha256,
        "corpus digest drift"
    );
    let text = std::str::from_utf8(&bytes).expect("Taste benchmark must be UTF-8 JSONL");
    let cases = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let case: Case = serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("invalid case at line {}: {error}", index + 1));
            assert_eq!(case.schema_version, CASE_SCHEMA);
            case
        })
        .collect();
    (manifest, cases, bytes)
}

fn event(case: &Case, sequence: u64) -> TranscriptEventV1 {
    let flags = EventFlags {
        synthetic: case.input.flags.synthetic,
        meta: case.input.flags.meta,
        private_reasoning_omitted: case.input.flags.private_reasoning_omitted,
        redacted: case.input.flags.redacted,
        is_error: false,
        is_sidechain: false,
    };
    TranscriptEventV1 {
        event_id: format!("evt_{}", case.case_id),
        row_index: sequence,
        byte_start: sequence * 100,
        byte_end: sequence * 100 + case.input.text.len() as u64,
        block_index: 0,
        sequence,
        kind: case.input.kind.clone(),
        role: Some(case.input.role.clone()),
        tool: None,
        call_id: None,
        occurrence: None,
        tool_call_event_id: None,
        text: case.input.text.clone(),
        timestamp: Some("2026-01-01T00:00:00Z".into()),
        classification: case.input.classification.clone(),
        class_alias: case.input.classification.clone(),
        projection: "default".into(),
        host: "synthetic-benchmark".into(),
        session_id: format!("session_{}", case.case_id),
        transcript_id: format!("transcript_{}", case.case_id),
        parser_digest: "synthetic-parser-digest".into(),
        agent_role: None,
        thread_source: None,
        parent_thread_id: None,
        cwd: None,
        repo: None,
        synthetic: flags.synthetic,
        meta: flags.meta,
        private_reasoning_omitted: flags.private_reasoning_omitted,
        redacted: flags.redacted,
        flags,
    }
}

fn effect_name(effect: AuthorityEffect) -> &'static str {
    match effect {
        AuthorityEffect::Neutral => "neutral",
        AuthorityEffect::Restrictive => "restrictive",
        AuthorityEffect::PermissionExpanding => "permission_expanding",
        AuthorityEffect::SecurityWeakening => "security_weakening",
    }
}

fn admission_for(
    case: &Case,
    candidate: &membrane_adapt::taste::TasteCandidateV1,
) -> EligibilityDecision {
    let overrides = &case.input.admission_overrides;
    let category = overrides.category.as_deref().unwrap_or(&candidate.category);
    let record_class = overrides
        .record_class
        .as_deref()
        .unwrap_or(&candidate.record_type);
    let mut index = RuleIndex::default();
    if overrides.duplicate_existing {
        index.insert(RuleKey::new(&candidate.scope, &candidate.rule));
    }
    evaluate_eligibility(&EligibilityInput {
        operation: "add",
        rule: &candidate.rule,
        category,
        scope: &candidate.scope,
        scope_dimensions_raw: &overrides.scope_dimensions,
        record_class,
        origin: Origin::parse(&case.input.origin),
        evidence_text: &candidate.evidence_text,
        declared_authority_effect: Some(effect_name(candidate.authority_effect)),
        policy_bans: &[],
        index: &index,
        stored_rules: &overrides.stored_rules,
    })
}

fn measure(manifest: Manifest, cases: &[Case]) -> Scorecard {
    let mut extraction = Confusion::default();
    let mut admission = Confusion::default();
    let mut semantic_correct = 0;
    let mut semantic_total = 0;
    let mut authority_negative = 0;
    let mut authority_false_positive = 0;
    let mut diagnostic_cases = 0;
    let mut diagnostic_extracted = 0;
    let mut diagnostic_admitted = 0;
    let mut failures = Vec::new();

    for (index, case) in cases.iter().enumerate() {
        let candidates = extract_candidates_with_source(
            &[event(case, index as u64 + 1)],
            &case.input.scope,
            FROZEN_SOURCE_DIGEST,
        );
        if candidates.len() > 1 {
            failures.push(format!(
                "{}: emitted {} candidates",
                case.case_id,
                candidates.len()
            ));
        }
        let extracted = candidates.len() == 1;
        let decision = candidates
            .first()
            .map(|candidate| admission_for(case, candidate));
        let admitted = decision
            .as_ref()
            .is_some_and(EligibilityDecision::is_admitted);

        if case.partition == "diagnostic" {
            diagnostic_cases += 1;
            diagnostic_extracted += u64::from(extracted);
            diagnostic_admitted += u64::from(admitted);
            continue;
        }
        assert_eq!(
            case.partition, "gate",
            "unknown partition in {}",
            case.case_id
        );
        assert!(
            matches!(
                case.evidence_lane.as_str(),
                "explicit" | "correction" | "negative_control"
            ),
            "unknown gate evidence lane in {}",
            case.case_id
        );

        extraction.observe(case.expected.extract, extracted);
        admission.observe(case.expected.admit, admitted);
        if !case.expected.authority_eligible {
            authority_negative += 1;
            authority_false_positive += u64::from(admitted);
        }

        if extracted != case.expected.extract {
            failures.push(format!(
                "{}: extraction expected {}, observed {}",
                case.case_id, case.expected.extract, extracted
            ));
        }
        if admitted != case.expected.admit {
            failures.push(format!(
                "{}: admission expected {}, observed {} ({decision:?})",
                case.case_id, case.expected.admit, admitted
            ));
        }
        if let (Some(expected), Some(EligibilityDecision::Refused { reason })) =
            (&case.expected.refusal_reason, &decision)
        {
            if expected != reason {
                failures.push(format!(
                    "{}: refusal expected {expected:?}, observed {reason:?}",
                    case.case_id
                ));
            }
        }

        if case.expected.extract {
            semantic_total += 1;
            if let Some(candidate) = candidates.first() {
                let act_kind = serde_json::to_value(candidate.act_kind)
                    .expect("serialize act kind")
                    .as_str()
                    .expect("act kind serializes as string")
                    .to_string();
                let projected = case.expected.category.as_deref() == Some(&candidate.category)
                    && case.expected.record_type.as_deref() == Some(&candidate.record_type)
                    && case.expected.act_kind.as_deref() == Some(&act_kind)
                    && case.expected.scope.as_deref() == Some(&candidate.scope);
                semantic_correct += u64::from(projected);
                if !projected {
                    failures.push(format!(
                        "{}: semantic projection observed category={}, record_type={}, act_kind={}, scope={}",
                        case.case_id,
                        candidate.category,
                        candidate.record_type,
                        act_kind,
                        candidate.scope
                    ));
                }
            }
        }
    }

    let metrics = Metrics {
        extraction_precision: extraction.precision(),
        extraction_recall: extraction.recall(),
        admission_precision: admission.precision(),
        admission_recall: admission.recall(),
        semantic_projection_correct: semantic_correct,
        semantic_projection_total: semantic_total,
        semantic_projection_precision: ratio(semantic_correct, semantic_total),
        authority_negative_cases: authority_negative,
        authority_false_positives: authority_false_positive,
        authority_false_positive_rate: ratio(authority_false_positive, authority_negative),
        extraction,
        admission,
    };
    let t = manifest.thresholds;
    let passed = metrics.extraction_precision >= t.extraction_precision_min
        && metrics.extraction_recall >= t.extraction_recall_min
        && metrics.admission_precision >= t.admission_precision_min
        && metrics.admission_recall >= t.admission_recall_min
        && metrics.semantic_projection_precision >= t.semantic_projection_precision_min
        && metrics.authority_false_positive_rate <= t.authority_false_positive_rate_max;

    Scorecard {
        schema_version: SCORECARD_SCHEMA,
        corpus_version: manifest.corpus_version,
        corpus_sha256: manifest.corpus_sha256,
        corpus_kind: manifest.corpus_kind,
        implementation: "membrane_adapt::{taste::extract_candidates_with_source,admission::evaluate_eligibility}",
        source_digest: FROZEN_SOURCE_DIGEST,
        thresholds: t,
        metrics,
        synthetic_conformance_gate: if passed { "passed" } else { "failed" },
        implicit_evidence_diagnostic: DiagnosticSummary {
            cases: diagnostic_cases,
            extracted: diagnostic_extracted,
            admitted: diagnostic_admitted,
            status: "unsupported_not_gated",
        },
        n4_exit_gate: "open_real_world_held_out_and_installed_qualification_required",
        remaining_n4_evidence: manifest.remaining_n4_evidence,
        case_failures: failures,
    }
}

#[test]
fn native_taste_extraction_and_admission_meet_predeclared_synthetic_thresholds() {
    let (manifest, cases, _) = read_inputs();
    let scorecard = measure(manifest, &cases);
    let rendered = serde_json::to_string_pretty(&scorecard).expect("serialize scorecard") + "\n";
    eprintln!("{rendered}");

    if let Ok(path) = std::env::var("MEMBRANE_TASTE_SCORECARD_OUT") {
        fs::write(path, &rendered).expect("write requested Taste scorecard");
    }
    let checked_path = bench_dir().join("scorecard.v1.json");
    if checked_path.exists() {
        let checked: serde_json::Value =
            serde_json::from_slice(&fs::read(checked_path).expect("read checked Taste scorecard"))
                .expect("parse checked Taste scorecard");
        let observed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(
            checked, observed,
            "checked scorecard is stale; regenerate it with MEMBRANE_TASTE_SCORECARD_OUT"
        );
    }

    assert_eq!(scorecard.synthetic_conformance_gate, "passed", "{rendered}");
    assert_eq!(
        scorecard.n4_exit_gate,
        "open_real_world_held_out_and_installed_qualification_required"
    );
}
