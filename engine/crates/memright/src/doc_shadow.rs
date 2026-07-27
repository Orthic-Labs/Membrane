//! Shadow-only replay metrics for Doc Spine candidates.
//!
//! These primitives deliberately make no admission decision. A clean replay remains shadow-only;
//! any regression first narrows the next replay to runbooks and decisions, then falls back to
//! registration-only if that narrowed replay also regresses.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayCandidateV1 {
    pub doc_id: String,
    pub section_id: Option<String>,
    pub document_class: DocumentClass,
    pub superseded: bool,
    pub duplicate: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShadowReplayCaseV1 {
    pub expected_doc_id: String,
    pub expected_section_id: Option<String>,
    pub baseline: Vec<ReplayCandidateV1>,
    pub with_docs: Vec<ReplayCandidateV1>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplayQualityMetricsV1 {
    pub mean_rank: f64,
    pub correct_doc_rate: f64,
    pub correct_section_rate: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadowReplayDisposition {
    /// Replay evidence is clean, but candidates remain excluded from live admission.
    ShadowOnly,
    /// Re-run using only conservative, task-aligned document classes.
    NarrowToRunbookAndDecision,
    /// Do not admit document candidates; retain registration/reconciliation only.
    RegistrationOnly,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShadowReplayReportV1 {
    pub baseline: ReplayQualityMetricsV1,
    pub with_docs: ReplayQualityMetricsV1,
    /// Queries where a known-good target's rank became worse after documents were added.
    pub displacement_count: usize,
    pub superseded_leakage_count: usize,
    pub duplicate_leakage_count: usize,
    pub disposition: ShadowReplayDisposition,
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
/// document-augmented ranking, since baseline candidates are out of scope for Doc Spine rollout.
pub fn evaluate_shadow_replay(cases: &[ShadowReplayCaseV1]) -> ShadowReplayReportV1 {
    let baseline = quality(cases, |case| &case.baseline);
    let with_docs = quality(cases, |case| &case.with_docs);
    let displacement_count = cases
        .iter()
        .filter(|case| rank(&case.with_docs, &case.expected_doc_id) > rank(&case.baseline, &case.expected_doc_id))
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
