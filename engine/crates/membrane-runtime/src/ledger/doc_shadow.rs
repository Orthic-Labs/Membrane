//! Shadow-only replay metrics for Ledger candidates.
//!
//! These primitives deliberately make no admission decision. A clean replay remains shadow-only;
//! any regression first narrows the next replay to runbooks and decisions, then falls back to
//! registration-only if that narrowed replay also regresses.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum DocumentClass {
    Knowledge,
    Decision,
    Runbook,
    Policy,
    Content,
    Generated,
    Historical,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ReplayCandidateV1 {
    pub doc_id: String,
    pub section_id: Option<String>,
    pub document_class: DocumentClass,
    pub superseded: bool,
    pub duplicate: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ShadowReplayCaseV1 {
    pub expected_doc_id: String,
    pub expected_section_id: Option<String>,
    pub baseline: Vec<ReplayCandidateV1>,
    pub with_docs: Vec<ReplayCandidateV1>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ReplayQualityMetricsV1 {
    pub mean_rank: f64,
    pub correct_doc_rate: f64,
    pub correct_section_rate: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum ShadowReplayDisposition {
    /// Replay evidence is clean, but candidates remain excluded from live admission.
    ShadowOnly,
    /// Re-run using only conservative, task-aligned document classes.
    NarrowToRunbookAndDecision,
    /// Do not admit document candidates; retain registration/reconciliation only.
    RegistrationOnly,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ShadowReplayReportV1 {
    pub baseline: ReplayQualityMetricsV1,
    pub with_docs: ReplayQualityMetricsV1,
    /// Queries where a known-good target's rank became worse after documents were added.
    pub displacement_count: usize,
    pub superseded_leakage_count: usize,
    pub duplicate_leakage_count: usize,
    pub disposition: ShadowReplayDisposition,
}

/// Frozen replay input plus its derived report, suitable for durable shadow evidence.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FrozenShadowReplayReceiptV1 {
    pub schema_version: String,
    pub cases: Vec<ShadowReplayCaseV1>,
    pub report: ShadowReplayReportV1,
}

/// Evaluate an owned, immutable replay fixture & retain both input and result in one receipt.
pub fn evaluate_frozen_shadow_replay(
    mut cases: Vec<ShadowReplayCaseV1>,
) -> FrozenShadowReplayReceiptV1 {
    // Frozen evidence must remain byte-stable when an upstream source enumerates
    // independent queries in a different order. Candidate order stays intact:
    // it encodes each query's ranking and therefore must not be normalized.
    cases.sort_by_cached_key(|case| {
        serde_json::to_vec(case).expect("shadow replay cases are always serializable")
    });
    let report = evaluate_shadow_replay(&cases);
    FrozenShadowReplayReceiptV1 {
        schema_version: "ledger.doc_shadow_receipt.v1".into(),
        cases,
        report,
    }
}

/// Provider-facing task classes eligible for document-candidate shadow observation.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum DocTaskClassV1 {
    DocLookup,
    Orientation,
    Runbook,
}

/// Identity required to prove a document candidate belongs to current source snapshot.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DocCandidateFreshnessV1 {
    pub content_hash: String,
    pub revision: String,
    pub index_generation: i64,
}

impl DocCandidateFreshnessV1 {
    /// Freshness is valid only when all three source identity fields match exactly.
    pub fn matches_current(&self, current: &Self) -> bool {
        !self.content_hash.is_empty()
            && !self.revision.is_empty()
            && self.content_hash == current.content_hash
            && self.revision == current.revision
            && self.index_generation == current.index_generation
    }
}

/// Minimal candidate surface for a future `DocCandidateProvider` integration.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DocCandidateProviderCandidateV1 {
    pub doc_id: String,
    pub document_class: DocumentClass,
    pub freshness: DocCandidateFreshnessV1,
}

/// Conservative policy: max two current, task-aligned documents, observed only in shadow.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DocCandidateProviderPolicyV1 {
    pub max_candidates: usize,
    pub shadow_only: bool,
}

impl Default for DocCandidateProviderPolicyV1 {
    fn default() -> Self {
        Self {
            max_candidates: 2,
            shadow_only: true,
        }
    }
}

/// Candidate subset available to shadow replay; this is never a live-admission result.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DocCandidateShadowSelectionV1 {
    pub shadow_only: bool,
    pub candidates: Vec<DocCandidateProviderCandidateV1>,
}

/// Select current candidates for a provider's shadow lane without granting prompt admission.
pub fn select_doc_candidates_for_shadow(
    policy: &DocCandidateProviderPolicyV1,
    task_class: DocTaskClassV1,
    current: &DocCandidateFreshnessV1,
    candidates: &[DocCandidateProviderCandidateV1],
) -> DocCandidateShadowSelectionV1 {
    let candidates = candidates
        .iter()
        .filter(|candidate| {
            candidate.freshness.matches_current(current)
                && task_allows_document(task_class, candidate.document_class)
        })
        .take(policy.max_candidates.min(2))
        .cloned()
        .collect();
    DocCandidateShadowSelectionV1 {
        shadow_only: policy.shadow_only,
        candidates,
    }
}

fn task_allows_document(task_class: DocTaskClassV1, document_class: DocumentClass) -> bool {
    match task_class {
        DocTaskClassV1::DocLookup => matches!(
            document_class,
            DocumentClass::Knowledge
                | DocumentClass::Decision
                | DocumentClass::Runbook
                | DocumentClass::Policy
        ),
        DocTaskClassV1::Orientation => {
            matches!(
                document_class,
                DocumentClass::Knowledge | DocumentClass::Decision
            )
        }
        DocTaskClassV1::Runbook => {
            matches!(
                document_class,
                DocumentClass::Runbook | DocumentClass::Decision
            )
        }
    }
}

impl ShadowReplayReportV1 {
    /// Produce disposition for a failed conservative retry without granting live admission.
    pub fn retry_after_narrowing(&self) -> Self {
        let mut retry = self.clone();
        if retry.disposition == ShadowReplayDisposition::NarrowToRunbookAndDecision {
            retry.disposition = ShadowReplayDisposition::RegistrationOnly;
        }
        retry
    }
}

/// Compare frozen baseline ranks with a merged `+docs` ranking.
///
/// Correctness is presence in ranked results; mean rank is one-based and assigns an absent
/// expected document the stable penalty `results.len() + 1`. Safety leakage examines only the
/// document-augmented ranking, since baseline candidates are out of scope for Ledger rollout.
pub fn evaluate_shadow_replay(cases: &[ShadowReplayCaseV1]) -> ShadowReplayReportV1 {
    let baseline = quality(cases, |case| &case.baseline);
    let with_docs = quality(cases, |case| &case.with_docs);
    let displacement_count = cases
        .iter()
        .filter(|case| {
            rank(&case.with_docs, &case.expected_doc_id)
                > rank(&case.baseline, &case.expected_doc_id)
        })
        .count();
    let superseded_leakage_count = cases
        .iter()
        .flat_map(|case| &case.with_docs)
        .filter(|candidate| candidate.superseded)
        .count();
    let duplicate_leakage_count = cases
        .iter()
        .flat_map(|case| &case.with_docs)
        .filter(|candidate| candidate.duplicate)
        .count();

    let regressed = with_docs.mean_rank > baseline.mean_rank
        || with_docs.correct_doc_rate < baseline.correct_doc_rate
        || with_docs.correct_section_rate < baseline.correct_section_rate
        || displacement_count > 0
        || superseded_leakage_count > 0
        || duplicate_leakage_count > 0;
    ShadowReplayReportV1 {
        baseline,
        with_docs,
        displacement_count,
        superseded_leakage_count,
        duplicate_leakage_count,
        disposition: if regressed {
            ShadowReplayDisposition::NarrowToRunbookAndDecision
        } else {
            ShadowReplayDisposition::ShadowOnly
        },
    }
}

fn quality<'a>(
    cases: &'a [ShadowReplayCaseV1],
    ranked: impl Fn(&'a ShadowReplayCaseV1) -> &'a [ReplayCandidateV1],
) -> ReplayQualityMetricsV1 {
    if cases.is_empty() {
        return ReplayQualityMetricsV1 {
            mean_rank: 0.0,
            correct_doc_rate: 0.0,
            correct_section_rate: 0.0,
        };
    }
    let mut total_rank = 0usize;
    let mut correct_docs = 0usize;
    let mut correct_sections = 0usize;
    for case in cases {
        let candidates = ranked(case);
        let expected_rank = rank(candidates, &case.expected_doc_id);
        total_rank += expected_rank;
        correct_docs += usize::from(expected_rank <= candidates.len());
        correct_sections += usize::from(case.expected_section_id.as_ref().is_some_and(|section| {
            candidates.iter().any(|candidate| {
                candidate.doc_id == case.expected_doc_id
                    && candidate.section_id.as_deref() == Some(section)
            })
        }));
    }
    let denominator = cases.len() as f64;
    ReplayQualityMetricsV1 {
        mean_rank: total_rank as f64 / denominator,
        correct_doc_rate: correct_docs as f64 / denominator,
        correct_section_rate: correct_sections as f64 / denominator,
    }
}

fn rank(candidates: &[ReplayCandidateV1], expected_doc_id: &str) -> usize {
    candidates
        .iter()
        .position(|candidate| candidate.doc_id == expected_doc_id)
        .map(|index| index + 1)
        .unwrap_or(candidates.len() + 1)
}
