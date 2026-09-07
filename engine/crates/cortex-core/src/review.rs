//! Proposal-only Cortex review boundaries.
//!
//! Stage 0 Dream remains deterministic and lives in [`crate::dream`]. This
//! module owns Stage 1's provider-output contracts and foreground gate. It
//! does not call a model or write durable memory.

use crate::{estimate_tokens, EventCursor, SessionEvent};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub const SEMANTIC_CURATION_PROPOSAL_SCHEMA_VERSION: u32 = 1;
pub const MEMORY_CANDIDATE_EXTRACTION_SCHEMA_VERSION: u32 = 1;
pub const SEMANTIC_CURATION_MAX_PROPOSALS: usize = 64;
pub const REVIEW_INPUT_SELECTION_SCHEMA_VERSION: u32 = 1;
/// The deterministic floor below which an event is not informative enough to
/// spend semantic-review budget on.  Selection never assigns semantic meaning
/// to an event; it only orders mechanical novelty scores.
pub const REVIEW_INPUT_NOVELTY_FLOOR: f64 = 0.2;

/// Stage 1 semantic review classes. Every value is proposal-only; Cortex
/// admission remains only path to durable truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticCurationKindV1 {
    Contradiction,
    NearDuplicate,
    Supersession,
    StaleSemanticAssumption,
    Merge,
    Split,
    UsefulnessLifecycleReview,
}

/// Content-free references carried by a semantic proposal. A proposal may
/// point at durable memories, absorbed session events, or both, but may not
/// omit all evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewEvidenceRefV1 {
    #[serde(default)]
    pub memory_ids: Vec<String>,
    #[serde(default)]
    pub source_event_ids: Vec<String>,
    #[serde(default)]
    pub source_content_hashes: Vec<String>,
}

/// Stage 1 semantic curation output. There is no mutation or authority field:
/// this record can only be submitted to separate Cortex admission/review
/// path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SemanticCurationProposalV1 {
    pub schema_version: u32,
    pub proposal_id: String,
    pub scope_id: String,
    pub kind: SemanticCurationKindV1,
    #[serde(default)]
    pub target_memory_ids: Vec<String>,
    pub evidence: Vec<ReviewEvidenceRefV1>,
}

/// Reasons semantic Stage 1 must refuse to claim output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticCurationBlockerV1 {
    /// No model input/provider is present in current Cortex production path.
    ModelInputUnavailable,
    /// No Cortex-owned semantic provider is wired to produce bounded proposals.
    SemanticProviderNotWired,
}

/// Explicit status for callers that need to surface unavailable Stage 1
/// without manufacturing an empty proposal list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum SemanticCurationStatusV1 {
    Blocked { reason: SemanticCurationBlockerV1 },
}

/// Authoritative foreground-memory signal consumed by background extraction.
/// `AvailableNoEmission` is distinct from missing signal and permits work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundMemoryStateV1 {
    Unavailable,
    AvailableNoEmission,
    AvailableEmission(ForegroundMemoryEmissionV1),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReviewContractError {
    #[error("unsupported review schema version: {0}")]
    SchemaVersion(u32),
    #[error("review field is empty: {0}")]
    EmptyField(&'static str),
    #[error("review evidence is empty")]
    EmptyEvidence,
    #[error("review evidence contains duplicate id: {0}")]
    DuplicateEvidenceId(String),
    #[error("cursor session identity is empty")]
    EmptyCursorSession,
    #[error("event session identity does not match cursor")]
    CursorSessionMismatch,
    #[error("event sequence must be positive")]
    InvalidEventSequence,
    #[error("event sequence is not strictly increasing")]
    UnorderedEvents,
    #[error("foreground emission range is invalid")]
    InvalidForegroundRange,
    #[error("foreground emission session identity does not match cursor")]
    ForegroundSessionMismatch,
    #[error("extraction limit must be positive: {0}")]
    InvalidLimit(&'static str),
    #[error("extraction input exceeds {limit} ({observed})")]
    InputBudgetExceeded {
        limit: &'static str,
        observed: usize,
    },
    #[error("candidate provenance does not match bounded event window")]
    CandidateProvenanceMismatch,
    #[error("candidate schema version is unsupported: {0}")]
    CandidateSchemaVersion(u32),
    #[error("semantic curation proposal count exceeds the bounded limit")]
    ProposalLimit,
    #[error("review input selection novelty floor is invalid")]
    InvalidNoveltyFloor,
}

/// Mechanical reason an eligible event was not selected for this run.  A
/// skipped event is not consumed by the cursor and remains eligible later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewInputSelectionSkipReasonV1 {
    BudgetExhausted,
    BelowNoveltyFloor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewInputSelectionCandidateV1 {
    pub event_id: String,
    pub seq: u64,
    pub novelty_score: f64,
    pub estimated_input_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewInputSelectionSkippedV1 {
    pub event_id: String,
    pub reason: ReviewInputSelectionSkipReasonV1,
}

/// Receipt for one deterministic input-selection pass.  It is intentionally
/// content-free: scores order event identities but never label an Adapt
/// category or otherwise interpret an episode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewInputSelectionV1 {
    pub schema_version: u32,
    pub candidates_considered: Vec<ReviewInputSelectionCandidateV1>,
    pub selected: Vec<String>,
    pub skipped: Vec<ReviewInputSelectionSkippedV1>,
    pub quiet_period: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewInputSelectionResultV1 {
    pub events: Vec<SessionEvent>,
    pub receipt: ReviewInputSelectionV1,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReviewInputSelectionLimitsV1 {
    pub max_input_tokens: usize,
    pub novelty_floor: f64,
}

impl ReviewInputSelectionLimitsV1 {
    pub fn validate(self) -> Result<(), ReviewContractError> {
        if !self.novelty_floor.is_finite() || !(0.0..=1.0).contains(&self.novelty_floor) {
            return Err(ReviewContractError::InvalidNoveltyFloor);
        }
        Ok(())
    }
}

/// Select only new events after the stored cursor.  Novelty is nearest-
/// neighbour distance against the already-reviewed event baseline, computed
/// locally from canonical payload tokens.  No model or Adapt taxonomy is
/// involved.
pub fn select_review_input(
    cursor: &EventCursor,
    candidates: &[SessionEvent],
    reviewed_baseline: &[SessionEvent],
    limits: ReviewInputSelectionLimitsV1,
) -> Result<ReviewInputSelectionResultV1, ReviewContractError> {
    limits.validate()?;
    if cursor.session_id.trim().is_empty() {
        return Err(ReviewContractError::EmptyCursorSession);
    }

    let mut previous_seq = cursor.last_seq;
    let mut ranked = Vec::new();
    for event in candidates {
        if event.session_id != cursor.session_id {
            return Err(ReviewContractError::CursorSessionMismatch);
        }
        if event.seq == 0 {
            return Err(ReviewContractError::InvalidEventSequence);
        }
        if event.seq <= cursor.last_seq {
            continue;
        }
        if event.seq <= previous_seq {
            return Err(ReviewContractError::UnorderedEvents);
        }
        previous_seq = event.seq;
        let estimated_input_tokens = estimate_tokens(&event.payload.to_string());
        ranked.push((
            event,
            novelty_score(event, reviewed_baseline),
            estimated_input_tokens,
        ));
    }

    let considered = ranked
        .iter()
        .map(|(event, score, estimated_input_tokens)| ReviewInputSelectionCandidateV1 {
            event_id: event.event_id.clone(),
            seq: event.seq,
            novelty_score: *score,
            estimated_input_tokens: *estimated_input_tokens,
        })
        .collect::<Vec<_>>();
    let mut order = (0..ranked.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| {
        ranked[*right]
            .1
            .partial_cmp(&ranked[*left].1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| ranked[*right].0.seq.cmp(&ranked[*left].0.seq))
            .then_with(|| ranked[*left].0.event_id.cmp(&ranked[*right].0.event_id))
    });

    let mut remaining = limits.max_input_tokens;
    let mut selected_ids = Vec::new();
    let mut selected_indices = Vec::new();
    let mut skipped_by_index = vec![None; ranked.len()];
    let mut above_floor = false;
    for index in order {
        let (event, score, estimated_input_tokens) = ranked[index];
        if score < limits.novelty_floor {
            skipped_by_index[index] = Some(ReviewInputSelectionSkipReasonV1::BelowNoveltyFloor);
            continue;
        }
        above_floor = true;
        if estimated_input_tokens <= remaining {
            remaining -= estimated_input_tokens;
            selected_ids.push(event.event_id.clone());
            selected_indices.push(index);
        } else {
            skipped_by_index[index] = Some(ReviewInputSelectionSkipReasonV1::BudgetExhausted);
        }
    }

    let skipped = ranked
        .iter()
        .enumerate()
        .filter_map(|(index, (event, _, _))| {
            skipped_by_index[index].map(|reason| ReviewInputSelectionSkippedV1 {
                event_id: event.event_id.clone(),
                reason,
            })
        })
        .collect::<Vec<_>>();
    let events = selected_indices
        .into_iter()
        .map(|index| (*ranked[index].0).clone())
        .collect::<Vec<_>>();
    // Provider request validation requires event order to remain the source
    // cursor order, while the receipt preserves score-ranked selection order.
    let mut events = events;
    events.sort_by_key(|event| event.seq);
    Ok(ReviewInputSelectionResultV1 {
        events,
        receipt: ReviewInputSelectionV1 {
            schema_version: REVIEW_INPUT_SELECTION_SCHEMA_VERSION,
            candidates_considered: considered,
            selected: selected_ids,
            skipped,
            quiet_period: !above_floor,
        },
    })
}

fn novelty_score(candidate: &SessionEvent, baseline: &[SessionEvent]) -> f64 {
    if baseline.is_empty() {
        return 1.0;
    }
    let candidate_tokens = payload_tokens(candidate);
    let nearest_similarity = baseline
        .iter()
        .map(|event| {
            if event.content_hash == candidate.content_hash {
                1.0
            } else {
                jaccard_similarity(&candidate_tokens, &payload_tokens(event))
            }
        })
        .fold(0.0, f64::max);
    (1.0 - nearest_similarity).clamp(0.0, 1.0)
}

fn payload_tokens(event: &SessionEvent) -> std::collections::BTreeSet<String> {
    serde_json::to_string(&event.payload)
        .unwrap_or_default()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn jaccard_similarity(
    left: &std::collections::BTreeSet<String>,
    right: &std::collections::BTreeSet<String>,
) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }
    let intersection = left.intersection(right).count() as f64;
    let union = left.union(right).count() as f64;
    if union == 0.0 {
        1.0
    } else {
        intersection / union
    }
}

impl SemanticCurationProposalV1 {
    /// Validate proposal shape without admitting, mutating, or interpreting
    /// semantic content.
    pub fn validate(&self) -> Result<(), ReviewContractError> {
        if self.schema_version != SEMANTIC_CURATION_PROPOSAL_SCHEMA_VERSION {
            return Err(ReviewContractError::SchemaVersion(self.schema_version));
        }
        if self.proposal_id.trim().is_empty() {
            return Err(ReviewContractError::EmptyField("proposal_id"));
        }
        if self.scope_id.trim().is_empty() {
            return Err(ReviewContractError::EmptyField("scope_id"));
        }
        if self.evidence.is_empty() {
            return Err(ReviewContractError::EmptyEvidence);
        }

        let mut ids = HashSet::new();
        for target in &self.target_memory_ids {
            if target.trim().is_empty() {
                return Err(ReviewContractError::EmptyField("target_memory_ids"));
            }
        }
        for evidence in &self.evidence {
            if evidence.memory_ids.is_empty()
                && evidence.source_event_ids.is_empty()
                && evidence.source_content_hashes.is_empty()
            {
                return Err(ReviewContractError::EmptyEvidence);
            }
            for id in evidence
                .memory_ids
                .iter()
                .chain(evidence.source_event_ids.iter())
                .chain(evidence.source_content_hashes.iter())
            {
                if id.trim().is_empty() {
                    return Err(ReviewContractError::EmptyField("evidence"));
                }
                if !ids.insert(id) {
                    return Err(ReviewContractError::DuplicateEvidenceId(id.clone()));
                }
            }
        }
        Ok(())
    }
}

/// Validate Stage 1 output as proposals only. This helper deliberately does
/// not expose a Cortex store or perform admission.
pub fn validate_semantic_curation_proposals(
    proposals: &[SemanticCurationProposalV1],
) -> Result<(), ReviewContractError> {
    if proposals.len() > SEMANTIC_CURATION_MAX_PROPOSALS {
        return Err(ReviewContractError::ProposalLimit);
    }
    for proposal in proposals {
        proposal.validate()?;
    }
    Ok(())
}

/// Range emitted by an authoritative foreground memory writer. Sequence
/// bounds are half-open, matching [`crate::event_range`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForegroundMemoryEmissionV1 {
    pub emission_id: String,
    pub session_id: String,
    pub start_seq: u64,
    pub end_seq: u64,
}

impl ForegroundMemoryEmissionV1 {
    pub fn validate(&self, cursor: &EventCursor) -> Result<(), ReviewContractError> {
        if self.emission_id.trim().is_empty() {
            return Err(ReviewContractError::EmptyField("emission_id"));
        }
        if self.session_id != cursor.session_id {
            return Err(ReviewContractError::ForegroundSessionMismatch);
        }
        if self.start_seq == 0 || self.start_seq >= self.end_seq {
            return Err(ReviewContractError::InvalidForegroundRange);
        }
        Ok(())
    }

    pub fn overlaps(&self, from_seq: u64, to_seq: u64) -> bool {
        self.start_seq < to_seq && self.end_seq > from_seq
    }
}

/// Hard input bounds for one background extraction attempt. Runtime time and
/// request accounting belongs to background runner; core enforces only bounds
/// it can observe over supplied event window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryCandidateExtractionLimitsV1 {
    pub max_events: usize,
    pub max_input_tokens: usize,
    pub max_duration_ms: u64,
    pub max_model_requests: usize,
}

impl MemoryCandidateExtractionLimitsV1 {
    pub fn validate(self) -> Result<(), ReviewContractError> {
        if self.max_events == 0 {
            return Err(ReviewContractError::InvalidLimit("max_events"));
        }
        if self.max_input_tokens == 0 {
            return Err(ReviewContractError::InvalidLimit("max_input_tokens"));
        }
        if self.max_duration_ms == 0 {
            return Err(ReviewContractError::InvalidLimit("max_duration_ms"));
        }
        if self.max_model_requests == 0 {
            return Err(ReviewContractError::InvalidLimit("max_model_requests"));
        }
        Ok(())
    }
}

/// Deterministically bounded input selected after caller's cursor. It is not
/// candidate output and carries only source identity/provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtractionWindowV1 {
    pub session_id: String,
    pub from_seq: u64,
    pub to_seq: u64,
    pub event_ids: Vec<String>,
    pub source_content_hashes: Vec<String>,
    pub estimated_input_tokens: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateExtractionSkipV1 {
    NoNewEvents,
    ForegroundMemoryEmissionPresent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateExtractionBlockerV1 {
    ForegroundMemoryEmissionSignalUnavailable,
    ModelInputUnavailable,
    CursorInputUnavailable,
}

/// Result of core input gate. `WindowBound` stops before semantic generation;
/// only real model/provider runner may proceed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum MemoryCandidateExtractionDecisionV1 {
    Skipped {
        reason: MemoryCandidateExtractionSkipV1,
    },
    Blocked {
        reason: MemoryCandidateExtractionBlockerV1,
    },
    WindowBound {
        window: ExtractionWindowV1,
    },
}

/// Candidate shape accepted from semantic provider. Validation ties provider
/// output to one bounded event window before Cortex admission sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryCandidateV1 {
    pub schema_version: u32,
    pub candidate_id: String,
    pub session_id: String,
    pub from_seq: u64,
    pub to_seq: u64,
    pub scope_id: String,
    pub content: String,
    pub source_event_ids: Vec<String>,
    pub source_content_hashes: Vec<String>,
}

impl MemoryCandidateV1 {
    /// Check provider output against core's bounded source window. This does
    /// not admit or persist candidate content.
    pub fn validate_against(&self, window: &ExtractionWindowV1) -> Result<(), ReviewContractError> {
        if self.schema_version != MEMORY_CANDIDATE_EXTRACTION_SCHEMA_VERSION {
            return Err(ReviewContractError::CandidateSchemaVersion(
                self.schema_version,
            ));
        }
        if self.candidate_id.trim().is_empty()
            || self.session_id.trim().is_empty()
            || self.scope_id.trim().is_empty()
            || self.content.trim().is_empty()
        {
            return Err(ReviewContractError::EmptyField("candidate"));
        }
        if self.session_id != window.session_id
            || self.from_seq != window.from_seq
            || self.to_seq != window.to_seq
            || self.source_event_ids != window.event_ids
            || self.source_content_hashes != window.source_content_hashes
        {
            return Err(ReviewContractError::CandidateProvenanceMismatch);
        }
        Ok(())
    }
}

/// Validate all candidates returned for one already-authorized extraction
/// window. This is the Cortex-owned foreground/provenance fence: callers may
/// enqueue valid candidates for admission, but this helper never persists
/// them or advances a cursor.
pub fn validate_memory_candidates_for_window(
    window: &ExtractionWindowV1,
    candidates: &[MemoryCandidateV1],
) -> Result<(), ReviewContractError> {
    if candidates.len() > SEMANTIC_CURATION_MAX_PROPOSALS {
        return Err(ReviewContractError::ProposalLimit);
    }
    for candidate in candidates {
        candidate.validate_against(window)?;
    }
    Ok(())
}

/// Bind one extraction attempt to its real event cursor. This function only
/// selects and budgets source events; it never creates a memory candidate.
pub fn bound_memory_candidate_extraction_window(
    cursor: &EventCursor,
    events: &[SessionEvent],
    foreground_emission: Option<&ForegroundMemoryEmissionV1>,
    limits: MemoryCandidateExtractionLimitsV1,
    model_input_available: bool,
) -> Result<MemoryCandidateExtractionDecisionV1, ReviewContractError> {
    let foreground_state = match foreground_emission {
        Some(emission) => ForegroundMemoryStateV1::AvailableEmission(emission.clone()),
        None => ForegroundMemoryStateV1::Unavailable,
    };
    bound_memory_candidate_extraction_window_with_state(
        cursor,
        events,
        &foreground_state,
        limits,
        model_input_available,
    )
}

/// Bind extraction window against three-state foreground signal. This is the
/// production Stage 1 gate: no new events, overlap, missing signal, provider
/// availability, and hard source budgets stay typed and deterministic.
pub fn bound_memory_candidate_extraction_window_with_state(
    cursor: &EventCursor,
    events: &[SessionEvent],
    foreground_state: &ForegroundMemoryStateV1,
    limits: MemoryCandidateExtractionLimitsV1,
    model_input_available: bool,
) -> Result<MemoryCandidateExtractionDecisionV1, ReviewContractError> {
    limits.validate()?;
    if cursor.session_id.trim().is_empty() {
        return Err(ReviewContractError::EmptyCursorSession);
    }
    match foreground_state {
        ForegroundMemoryStateV1::Unavailable | ForegroundMemoryStateV1::AvailableNoEmission => {}
        ForegroundMemoryStateV1::AvailableEmission(emission) => {
            emission.validate(cursor)?;
        }
    }

    let mut previous_seq = cursor.last_seq;
    let mut selected = Vec::new();
    for event in events {
        if event.session_id != cursor.session_id {
            return Err(ReviewContractError::CursorSessionMismatch);
        }
        if event.seq == 0 {
            return Err(ReviewContractError::InvalidEventSequence);
        }
        if event.seq <= cursor.last_seq {
            continue;
        }
        if event.seq <= previous_seq {
            return Err(ReviewContractError::UnorderedEvents);
        }
        previous_seq = event.seq;
        selected.push(event);
    }

    if selected.is_empty() {
        return Ok(MemoryCandidateExtractionDecisionV1::Skipped {
            reason: MemoryCandidateExtractionSkipV1::NoNewEvents,
        });
    }

    let from_seq = selected
        .first()
        .map(|event| event.seq)
        .unwrap_or(cursor.last_seq.saturating_add(1));
    let to_seq = selected
        .last()
        .map(|event| event.seq.saturating_add(1))
        .unwrap_or(from_seq);

    // Foreground emission wins before model or budget checks: background path
    // must skip range entirely when authoritative memory already exists.
    match foreground_state {
        ForegroundMemoryStateV1::AvailableEmission(emission)
            if emission.overlaps(from_seq, to_seq) =>
        {
            return Ok(MemoryCandidateExtractionDecisionV1::Skipped {
                reason: MemoryCandidateExtractionSkipV1::ForegroundMemoryEmissionPresent,
            });
        }
        ForegroundMemoryStateV1::Unavailable => {
            return Ok(MemoryCandidateExtractionDecisionV1::Blocked {
                reason:
                    MemoryCandidateExtractionBlockerV1::ForegroundMemoryEmissionSignalUnavailable,
            });
        }
        ForegroundMemoryStateV1::AvailableNoEmission
        | ForegroundMemoryStateV1::AvailableEmission(_) => {}
    }

    if !model_input_available {
        return Ok(MemoryCandidateExtractionDecisionV1::Blocked {
            reason: MemoryCandidateExtractionBlockerV1::ModelInputUnavailable,
        });
    }

    if selected.len() > limits.max_events {
        return Err(ReviewContractError::InputBudgetExceeded {
            limit: "max_events",
            observed: selected.len(),
        });
    }

    let estimated_input_tokens = selected
        .iter()
        .map(|event| estimate_tokens(&event.payload.to_string()))
        .sum::<usize>();
    if estimated_input_tokens > limits.max_input_tokens {
        return Err(ReviewContractError::InputBudgetExceeded {
            limit: "max_input_tokens",
            observed: estimated_input_tokens,
        });
    }

    Ok(MemoryCandidateExtractionDecisionV1::WindowBound {
        window: ExtractionWindowV1 {
            session_id: cursor.session_id.clone(),
            from_seq,
            to_seq,
            event_ids: selected
                .iter()
                .map(|event| event.event_id.clone())
                .collect(),
            source_content_hashes: selected
                .iter()
                .map(|event| event.content_hash.clone())
                .collect(),
            estimated_input_tokens,
        },
    })
}

// ---------------------------------------------------------------------------
// CTX-023 / CTX-024: first-party deterministic Stage 1 analyzer.
//
// This is NOT a semantic model.  It is a closed, explainable set of lexical and
// structural rules over the caller's own event window.  Every output is a
// *candidate* proposal that must still pass Cortex admission; nothing here
// writes durable truth, and every proposal names the events it derives from so
// a reviewer can recover the parents.
// ---------------------------------------------------------------------------

/// Stable identity of the deterministic analyzer.  It is deliberately not a
/// model name: no model is involved.
pub const DETERMINISTIC_REVIEW_ANALYZER_ID: &str = "cortex-deterministic-review-analyzer";
pub const DETERMINISTIC_REVIEW_ANALYZER_VERSION: u32 = 1;
/// Shingle-overlap floor above which two events are proposed as near
/// duplicates.  Exact normalized equality (Stage 0's rule) always qualifies.
pub const DETERMINISTIC_NEAR_DUPLICATE_SIMILARITY: f64 = 0.85;

/// Protocol-neutral view of one source event.  Cortex core deliberately does
/// not depend on the wire contract; the runtime adapts its events into this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeterministicReviewEventV1 {
    pub event_id: String,
    pub seq: u64,
    pub scope_id: String,
    pub content_hash: String,
    /// Flattened text of the event payload, in canonical key order.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeterministicReviewLimitsV1 {
    pub max_proposals: usize,
    pub near_duplicate_similarity: f64,
}

impl Default for DeterministicReviewLimitsV1 {
    fn default() -> Self {
        Self {
            max_proposals: SEMANTIC_CURATION_MAX_PROPOSALS,
            near_duplicate_similarity: DETERMINISTIC_NEAR_DUPLICATE_SIMILARITY,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeterministicReviewFindingsV1 {
    pub proposals: Vec<SemanticCurationProposalV1>,
    /// True when the proposal cap stopped emission before the window was
    /// exhausted.  The caller must record this as an omission.
    pub truncated: bool,
}

/// Flatten a JSON payload to text in canonical (sorted) key order.  Only scalar
/// leaves contribute; keys are included so structurally distinct payloads do
/// not collapse into the same string.
pub fn review_payload_text(payload: &serde_json::Value) -> String {
    let mut out = String::new();
    push_payload_text(payload, &mut out);
    out.trim().to_string()
}

fn push_payload_text(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Null => {}
        serde_json::Value::Bool(inner) => push_token(&inner.to_string(), out),
        serde_json::Value::Number(inner) => push_token(&inner.to_string(), out),
        serde_json::Value::String(inner) => push_token(inner, out),
        serde_json::Value::Array(items) => {
            for item in items {
                push_payload_text(item, out);
            }
        }
        serde_json::Value::Object(entries) => {
            // serde_json's default map is ordered, so iteration is canonical.
            for (key, item) in entries {
                push_token(key, out);
                push_payload_text(item, out);
            }
        }
    }
}

fn push_token(token: &str, out: &mut String) {
    let token = token.trim();
    if token.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(token);
}

/// Closed marker table.  Each entry maps a literal token sequence to a
/// predicate family and a polarity.  Nothing outside this table is treated as
/// an assertion, which is what keeps the rule explainable without a model.
const ASSERTION_MARKERS: &[(&str, &str, bool)] = &[
    ("does not use", "use", false),
    ("do not use", "use", false),
    ("is not", "be", false),
    ("are not", "be", false),
    ("was not", "be", false),
    ("were not", "be", false),
    ("must not", "must", false),
    ("should not", "should", false),
    ("will not", "will", false),
    ("cannot", "can", false),
    ("never", "always", false),
    ("uses", "use", true),
    ("use", "use", true),
    ("is", "be", true),
    ("are", "be", true),
    ("was", "be", true),
    ("were", "be", true),
    ("must", "must", true),
    ("should", "should", true),
    ("will", "will", true),
    ("can", "can", true),
    ("always", "always", true),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeterministicAssertion {
    subject: String,
    family: &'static str,
    polarity: bool,
    object: BTreeSet<String>,
}

fn normalized_tokens(text: &str) -> Vec<String> {
    crate::dream::dedup_key(text)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Split flattened event text into sentence-like units before matching.
fn sentences(text: &str) -> Vec<String> {
    text.split(|character| matches!(character, '.' | '!' | '?' | ';' | '\n'))
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// Extract at most one assertion per sentence at the FIRST marker occurrence.
/// Subject and object are literal normalized token spans — no interpretation.
fn extract_assertions(text: &str) -> Vec<DeterministicAssertion> {
    let mut assertions = Vec::new();
    for sentence in sentences(text) {
        let tokens = normalized_tokens(&sentence);
        if tokens.is_empty() {
            continue;
        }
        let mut found: Option<(usize, usize, &'static str, bool)> = None;
        'positions: for position in 1..tokens.len() {
            for (marker, family, polarity) in ASSERTION_MARKERS {
                let marker_tokens = marker.split(' ').collect::<Vec<_>>();
                if position + marker_tokens.len() >= tokens.len() + 1 {
                    continue;
                }
                if tokens[position..position + marker_tokens.len()]
                    .iter()
                    .zip(marker_tokens.iter())
                    .all(|(token, marker_token)| token == marker_token)
                {
                    found = Some((position, marker_tokens.len(), family, *polarity));
                    break 'positions;
                }
            }
        }
        let Some((position, marker_len, family, polarity)) = found else {
            continue;
        };
        let subject = tokens[..position].join(" ");
        let object = tokens[position + marker_len..]
            .iter()
            .cloned()
            .collect::<BTreeSet<String>>();
        if subject.is_empty() || object.is_empty() {
            continue;
        }
        assertions.push(DeterministicAssertion {
            subject,
            family,
            polarity,
            object,
        });
    }
    assertions
}

fn shingles(tokens: &[String]) -> BTreeSet<String> {
    if tokens.len() < 3 {
        return tokens.iter().cloned().collect();
    }
    tokens.windows(3).map(|window| window.join(" ")).collect()
}

fn set_jaccard(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f64 {
    let union = left.union(right).count() as f64;
    if union == 0.0 {
        return 0.0;
    }
    left.intersection(right).count() as f64 / union
}

fn stable_proposal_id(kind: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    for part in parts {
        hasher.update([0u8]);
        hasher.update(part.as_bytes());
    }
    let digest = hex::encode(hasher.finalize());
    format!("cdr-{kind}-{}", &digest[..16])
}

/// Evidence naming both parents so a reviewer can recover them.  Each parent
/// gets its own reference; a content hash is emitted once because proposal
/// validation rejects a repeated id inside one proposal.
fn parent_evidence(
    earlier: &DeterministicReviewEventV1,
    later: &DeterministicReviewEventV1,
) -> Vec<ReviewEvidenceRefV1> {
    let mut seen = HashSet::new();
    [earlier, later]
        .into_iter()
        .map(|event| ReviewEvidenceRefV1 {
            memory_ids: Vec::new(),
            source_event_ids: vec![event.event_id.clone()],
            source_content_hashes: if seen.insert(event.content_hash.clone()) {
                vec![event.content_hash.clone()]
            } else {
                Vec::new()
            },
        })
        .collect()
}

fn proposal(
    kind: SemanticCurationKindV1,
    kind_slug: &str,
    scope_id: &str,
    discriminator: &str,
    earlier: &DeterministicReviewEventV1,
    later: &DeterministicReviewEventV1,
) -> SemanticCurationProposalV1 {
    SemanticCurationProposalV1 {
        schema_version: SEMANTIC_CURATION_PROPOSAL_SCHEMA_VERSION,
        proposal_id: stable_proposal_id(
            kind_slug,
            &[
                scope_id,
                discriminator,
                earlier.event_id.as_str(),
                later.event_id.as_str(),
            ],
        ),
        scope_id: scope_id.to_string(),
        kind,
        target_memory_ids: Vec::new(),
        evidence: parent_evidence(earlier, later),
    }
}

/// Deterministic Stage 1 analysis over one bounded event window.
///
/// Rules, in order:
/// 1. **Near duplicate** — same scope and either identical Stage 0 dedup key
///    (the exact rule [`crate::dream`] already uses) or 3-gram shingle overlap
///    at or above the configured floor.
/// 2. **Contradiction candidate** — same scope, same literal subject span and
///    predicate family, and either opposite polarity (an explicit negation
///    marker) or object token sets that do not intersect at all.
/// 3. **Supersession candidate** — same scope, subject, family and polarity,
///    with overlapping (restating) object tokens, taking the later event as
///    the restatement.  Pairs already reported as near duplicates are not
///    repeated here.
///
/// The output is a candidate set for human/governed review.  It asserts a
/// *structural* relationship, never that the analyzer understood the text.
pub fn analyze_events_for_curation(
    events: &[DeterministicReviewEventV1],
    limits: DeterministicReviewLimitsV1,
) -> DeterministicReviewFindingsV1 {
    let mut ordered = events.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.seq
            .cmp(&right.seq)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });

    let mut proposals = Vec::new();
    let mut truncated = false;
    let mut near_duplicate_pairs = HashSet::new();
    let mut emitted_ids = HashSet::new();
    let mut push = |proposal: SemanticCurationProposalV1,
                    proposals: &mut Vec<SemanticCurationProposalV1>,
                    truncated: &mut bool| {
        if !emitted_ids.insert(proposal.proposal_id.clone()) {
            return;
        }
        if proposals.len() >= limits.max_proposals {
            *truncated = true;
            return;
        }
        proposals.push(proposal);
    };

    let keys = ordered
        .iter()
        .map(|event| normalized_tokens(&event.text))
        .collect::<Vec<_>>();

    // 1. near duplicates
    for (left_index, left) in ordered.iter().enumerate() {
        for (right_index, right) in ordered.iter().enumerate().skip(left_index + 1) {
            if left.scope_id != right.scope_id {
                continue;
            }
            if keys[left_index].is_empty() || keys[right_index].is_empty() {
                continue;
            }
            let identical = keys[left_index] == keys[right_index];
            let similarity = set_jaccard(
                &shingles(&keys[left_index]),
                &shingles(&keys[right_index]),
            );
            if !identical && similarity < limits.near_duplicate_similarity {
                continue;
            }
            near_duplicate_pairs.insert((left.event_id.clone(), right.event_id.clone()));
            push(
                proposal(
                    SemanticCurationKindV1::NearDuplicate,
                    "near-duplicate",
                    &left.scope_id,
                    if identical { "exact" } else { "shingle" },
                    left,
                    right,
                ),
                &mut proposals,
                &mut truncated,
            );
        }
    }

    // 2 & 3. assertion pairs, grouped by (scope, subject, predicate family)
    let mut groups: BTreeMap<(String, String, &'static str), Vec<(usize, DeterministicAssertion)>> =
        BTreeMap::new();
    for (index, event) in ordered.iter().enumerate() {
        for assertion in extract_assertions(&event.text) {
            groups
                .entry((
                    event.scope_id.clone(),
                    assertion.subject.clone(),
                    assertion.family,
                ))
                .or_default()
                .push((index, assertion));
        }
    }

    for ((scope_id, subject, family), members) in groups {
        for left_position in 0..members.len() {
            for right_position in (left_position + 1)..members.len() {
                let (left_index, left_assertion) = &members[left_position];
                let (right_index, right_assertion) = &members[right_position];
                if left_index == right_index {
                    continue;
                }
                let earlier = ordered[*left_index];
                let later = ordered[*right_index];
                let discriminator = format!("{subject}|{family}");
                if left_assertion.polarity != right_assertion.polarity {
                    push(
                        proposal(
                            SemanticCurationKindV1::Contradiction,
                            "contradiction",
                            &scope_id,
                            &format!("{discriminator}|polarity"),
                            earlier,
                            later,
                        ),
                        &mut proposals,
                        &mut truncated,
                    );
                    continue;
                }
                let overlap = left_assertion
                    .object
                    .intersection(&right_assertion.object)
                    .count();
                if overlap == 0 {
                    push(
                        proposal(
                            SemanticCurationKindV1::Contradiction,
                            "contradiction",
                            &scope_id,
                            &format!("{discriminator}|object"),
                            earlier,
                            later,
                        ),
                        &mut proposals,
                        &mut truncated,
                    );
                    continue;
                }
                if near_duplicate_pairs
                    .contains(&(earlier.event_id.clone(), later.event_id.clone()))
                {
                    continue;
                }
                push(
                    proposal(
                        SemanticCurationKindV1::Supersession,
                        "supersession",
                        &scope_id,
                        &discriminator,
                        earlier,
                        later,
                    ),
                    &mut proposals,
                    &mut truncated,
                );
            }
        }
    }

    proposals.sort_by(|left, right| left.proposal_id.cmp(&right.proposal_id));
    DeterministicReviewFindingsV1 {
        proposals,
        truncated,
    }
}

/// Build the single bounded episodic candidate for an already-gated window.
///
/// The caller must have obtained `window` from
/// [`bound_memory_candidate_extraction_window_with_state`], which is the
/// CTX-024 foreground gate: this function never decides whether extraction is
/// permitted.  Content is a whitespace-normalized transcript of the window's
/// own events — no summary is invented.
pub fn extract_bounded_memory_candidate(
    window: &ExtractionWindowV1,
    events: &[DeterministicReviewEventV1],
) -> Option<MemoryCandidateV1> {
    let mut by_id = BTreeMap::new();
    for event in events {
        by_id.insert(event.event_id.as_str(), event);
    }
    let mut lines = Vec::new();
    let mut scope_id = None;
    for event_id in &window.event_ids {
        let Some(event) = by_id.get(event_id.as_str()) else {
            // The window must be built from these very events; a missing one
            // means the caller mismatched its inputs.
            return None;
        };
        if scope_id.is_none() && !event.scope_id.trim().is_empty() {
            scope_id = Some(event.scope_id.clone());
        }
        let text = event.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        lines.push(format!("{}: {}", event.seq, text));
    }
    if lines.is_empty() {
        return None;
    }
    let content = lines.join("\n");
    let candidate_id = stable_proposal_id(
        "episodic",
        &[
            window.session_id.as_str(),
            &window.from_seq.to_string(),
            &window.to_seq.to_string(),
            &window.source_content_hashes.join(","),
        ],
    );
    Some(MemoryCandidateV1 {
        schema_version: MEMORY_CANDIDATE_EXTRACTION_SCHEMA_VERSION,
        candidate_id,
        session_id: window.session_id.clone(),
        from_seq: window.from_seq,
        to_seq: window.to_seq,
        scope_id: scope_id.unwrap_or_else(crate::default_scope),
        content,
        source_event_ids: window.event_ids.clone(),
        source_content_hashes: window.source_content_hashes.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{content_hash, ProvenanceRef, ABSORBED_SCHEMA_VERSION};
    use serde_json::json;

    fn event(session_id: &str, seq: u64, event_id: &str, text: &str) -> SessionEvent {
        let payload = json!({"text": text});
        SessionEvent {
            schema_version: ABSORBED_SCHEMA_VERSION,
            session_id: session_id.to_string(),
            seq,
            event_id: event_id.to_string(),
            event_type: "assistant_message".to_string(),
            payload: payload.clone(),
            scope_id: "scope".to_string(),
            authority: "observed".to_string(),
            influence_class: "episodic".to_string(),
            lifecycle: "active".to_string(),
            retention: "session".to_string(),
            provenance: vec![ProvenanceRef {
                source: "test".to_string(),
                source_event_ids: vec![event_id.to_string()],
                producer: None,
            }],
            occurred_at_ms: seq,
            recorded_at_ms: seq,
            content_hash: content_hash(&payload).expect("test payload hashes"),
        }
    }

    fn limits() -> MemoryCandidateExtractionLimitsV1 {
        MemoryCandidateExtractionLimitsV1 {
            max_events: 8,
            max_input_tokens: 128,
            max_duration_ms: 1_000,
            max_model_requests: 1,
        }
    }

    // ---- CTX-023 / CTX-024 deterministic analyzer -------------------------

    fn analyzer_event(seq: u64, id: &str, text: &str) -> DeterministicReviewEventV1 {
        let payload = json!({ "text": text });
        DeterministicReviewEventV1 {
            event_id: id.to_string(),
            seq,
            scope_id: "scope".to_string(),
            content_hash: content_hash(&payload).expect("payload hashes"),
            text: review_payload_text(&payload),
        }
    }

    fn kinds(findings: &DeterministicReviewFindingsV1) -> Vec<SemanticCurationKindV1> {
        let mut kinds = findings
            .proposals
            .iter()
            .map(|proposal| proposal.kind)
            .collect::<Vec<_>>();
        kinds.sort_by_key(|kind| format!("{kind:?}"));
        kinds
    }

    fn realistic_window() -> Vec<DeterministicReviewEventV1> {
        vec![
            analyzer_event(1, "e1", "The release pipeline uses the windows signing runner."),
            // Near duplicate of e1 (punctuation/casing only).
            analyzer_event(2, "e2", "the release pipeline uses the Windows signing runner"),
            // Contradiction by explicit negation of the same subject/family.
            analyzer_event(3, "e3", "The release pipeline does not use the windows signing runner."),
            // Supersession: same subject/family/polarity, overlapping object.
            analyzer_event(4, "e4", "The staging index is stale after four hours."),
            analyzer_event(5, "e5", "The staging index is stale after two hours."),
        ]
    }

    #[test]
    fn deterministic_analyzer_reuses_stage_zero_duplicate_key() {
        // The near-duplicate rule must agree with Stage 0's notion of "same
        // content", not invent a second one.
        assert_eq!(
            crate::dream::dedup_key("The release pipeline uses the windows signing runner."),
            crate::dream::dedup_key("the release pipeline uses the Windows signing runner")
        );
    }

    #[test]
    fn deterministic_analyzer_produces_all_three_proposal_kinds() {
        let findings = analyze_events_for_curation(
            &realistic_window(),
            DeterministicReviewLimitsV1::default(),
        );
        let kinds = kinds(&findings);
        assert!(kinds.contains(&SemanticCurationKindV1::NearDuplicate), "{kinds:?}");
        assert!(kinds.contains(&SemanticCurationKindV1::Contradiction), "{kinds:?}");
        assert!(kinds.contains(&SemanticCurationKindV1::Supersession), "{kinds:?}");
        assert!(!findings.truncated);
        validate_semantic_curation_proposals(&findings.proposals)
            .expect("analyzer output is proposal-shaped");
    }

    #[test]
    fn deterministic_analyzer_output_is_stable_and_parents_are_recoverable() {
        let events = realistic_window();
        let first = analyze_events_for_curation(&events, DeterministicReviewLimitsV1::default());
        let second = analyze_events_for_curation(&events, DeterministicReviewLimitsV1::default());
        assert_eq!(first, second);
        for proposal in &first.proposals {
            let parents = proposal
                .evidence
                .iter()
                .flat_map(|evidence| evidence.source_event_ids.iter())
                .collect::<Vec<_>>();
            assert_eq!(parents.len(), 2, "every proposal names both parents");
            for parent in parents {
                assert!(
                    events.iter().any(|event| &event.event_id == parent),
                    "parent {parent} is recoverable from the window"
                );
            }
            // Proposal-only: no admission, no mutation, no target rewrite.
            assert!(proposal.target_memory_ids.is_empty());
        }
    }

    #[test]
    fn deterministic_analyzer_never_pairs_across_scopes() {
        let mut events = realistic_window();
        events[1].scope_id = "other-scope".to_string();
        let findings =
            analyze_events_for_curation(&events, DeterministicReviewLimitsV1::default());
        for proposal in &findings.proposals {
            assert_ne!(proposal.scope_id, "other-scope");
            let ids = proposal
                .evidence
                .iter()
                .flat_map(|evidence| evidence.source_event_ids.iter())
                .collect::<Vec<_>>();
            assert!(!ids.contains(&&"e2".to_string()));
        }
    }

    #[test]
    fn deterministic_analyzer_ignores_unrelated_events() {
        let events = vec![
            analyzer_event(1, "e1", "Opened the settings panel."),
            analyzer_event(2, "e2", "Ran the installer smoke check."),
        ];
        let findings =
            analyze_events_for_curation(&events, DeterministicReviewLimitsV1::default());
        assert!(findings.proposals.is_empty(), "{:?}", findings.proposals);
    }

    #[test]
    fn deterministic_analyzer_honours_the_proposal_cap() {
        let findings = analyze_events_for_curation(
            &realistic_window(),
            DeterministicReviewLimitsV1 {
                max_proposals: 1,
                ..DeterministicReviewLimitsV1::default()
            },
        );
        assert_eq!(findings.proposals.len(), 1);
        assert!(findings.truncated, "truncation is recorded, not hidden");
    }

    #[test]
    fn bounded_episodic_candidate_matches_its_window_exactly() {
        let cursor = EventCursor {
            session_id: "session".to_string(),
            last_seq: 0,
        };
        let events = vec![event("session", 1, "e1", "shipped the installer")];
        let decision = bound_memory_candidate_extraction_window_with_state(
            &cursor,
            &events,
            &ForegroundMemoryStateV1::AvailableNoEmission,
            limits(),
            true,
        )
        .expect("window binds");
        let MemoryCandidateExtractionDecisionV1::WindowBound { window } = decision else {
            panic!("expected a bound window");
        };
        let analyzer_events = events
            .iter()
            .map(|event| DeterministicReviewEventV1 {
                event_id: event.event_id.clone(),
                seq: event.seq,
                scope_id: event.scope_id.clone(),
                content_hash: event.content_hash.clone(),
                text: review_payload_text(&event.payload),
            })
            .collect::<Vec<_>>();
        let candidate = extract_bounded_memory_candidate(&window, &analyzer_events)
            .expect("candidate is extracted");
        candidate
            .validate_against(&window)
            .expect("candidate is bound to its own window");
        // Content is derived from the window's own events, not invented.
        assert!(candidate.content.contains("shipped the installer"));
        assert_eq!(candidate.source_event_ids, window.event_ids);
    }

    #[test]
    fn stage_one_status_is_explicitly_blocked_without_model_input() {
        let status = SemanticCurationStatusV1::Blocked {
            reason: SemanticCurationBlockerV1::ModelInputUnavailable,
        };
        let encoded = serde_json::to_value(status).expect("status serializes");
        assert_eq!(encoded["blocked"]["reason"], "model_input_unavailable");
    }

    #[test]
    fn stage_one_proposal_requires_evidence_and_remains_proposal_shaped() {
        let proposal = SemanticCurationProposalV1 {
            schema_version: SEMANTIC_CURATION_PROPOSAL_SCHEMA_VERSION,
            proposal_id: "proposal-1".to_string(),
            scope_id: "scope".to_string(),
            kind: SemanticCurationKindV1::NearDuplicate,
            target_memory_ids: vec!["memory-1".to_string()],
            evidence: vec![ReviewEvidenceRefV1 {
                memory_ids: vec!["memory-1".to_string()],
                source_event_ids: vec!["event-1".to_string()],
                source_content_hashes: vec!["sha256:abc".to_string()],
            }],
        };
        proposal
            .validate()
            .expect("evidence-bound proposal validates");
        let encoded = serde_json::to_value(proposal).expect("proposal serializes");
        assert!(encoded.get("authority").is_none());
        assert!(encoded.get("mutation").is_none());
    }

    #[test]
    fn stage_one_candidate_batch_uses_cortex_provenance_fence() {
        let window = ExtractionWindowV1 {
            session_id: "session-1".into(),
            from_seq: 2,
            to_seq: 3,
            event_ids: vec!["event-2".into()],
            source_content_hashes: vec!["hash-2".into()],
            estimated_input_tokens: 4,
        };
        let candidate = MemoryCandidateV1 {
            schema_version: MEMORY_CANDIDATE_EXTRACTION_SCHEMA_VERSION,
            candidate_id: "candidate-1".into(),
            session_id: "session-1".into(),
            from_seq: 2,
            to_seq: 3,
            scope_id: "scope".into(),
            content: "bounded candidate".into(),
            source_event_ids: vec!["event-2".into()],
            source_content_hashes: vec!["hash-2".into()],
        };
        validate_memory_candidates_for_window(&window, &[candidate.clone()]).unwrap();
        let mut tampered = candidate;
        tampered.source_event_ids = vec!["other-event".into()];
        assert_eq!(
            validate_memory_candidates_for_window(&window, &[tampered]),
            Err(ReviewContractError::CandidateProvenanceMismatch)
        );
    }

    #[test]
    fn extraction_skips_when_foreground_emission_covers_cursor_range() {
        let cursor = EventCursor {
            session_id: "session".to_string(),
            last_seq: 1,
        };
        let emission = ForegroundMemoryEmissionV1 {
            emission_id: "emission-1".to_string(),
            session_id: "session".to_string(),
            start_seq: 2,
            end_seq: 3,
        };
        let decision = bound_memory_candidate_extraction_window(
            &cursor,
            &[event("session", 2, "event-2", "already remembered")],
            Some(&emission),
            limits(),
            false,
        )
        .expect("foreground skip is a valid decision");
        assert_eq!(
            decision,
            MemoryCandidateExtractionDecisionV1::Skipped {
                reason: MemoryCandidateExtractionSkipV1::ForegroundMemoryEmissionPresent,
            }
        );
    }

    #[test]
    fn extraction_fails_closed_without_foreground_emission_signal() {
        let cursor = EventCursor {
            session_id: "session".to_string(),
            last_seq: 1,
        };
        let decision = bound_memory_candidate_extraction_window(
            &cursor,
            &[event("session", 2, "event-2", "new")],
            None,
            limits(),
            true,
        )
        .expect("missing signal is observable, not an error");
        assert_eq!(
            decision,
            MemoryCandidateExtractionDecisionV1::Blocked {
                reason:
                    MemoryCandidateExtractionBlockerV1::ForegroundMemoryEmissionSignalUnavailable,
            }
        );
    }

    #[test]
    fn extraction_reports_missing_model_after_real_cursor_and_marker() {
        let cursor = EventCursor {
            session_id: "session".to_string(),
            last_seq: 1,
        };
        let emission = ForegroundMemoryEmissionV1 {
            emission_id: "emission-old".to_string(),
            session_id: "session".to_string(),
            start_seq: 1,
            end_seq: 2,
        };
        let decision = bound_memory_candidate_extraction_window(
            &cursor,
            &[event("session", 2, "event-2", "new")],
            Some(&emission),
            limits(),
            false,
        )
        .expect("missing model is observable, not an error");
        assert_eq!(
            decision,
            MemoryCandidateExtractionDecisionV1::Blocked {
                reason: MemoryCandidateExtractionBlockerV1::ModelInputUnavailable,
            }
        );
    }

    #[test]
    fn extraction_bounds_to_events_after_cursor() {
        let cursor = EventCursor {
            session_id: "session".to_string(),
            last_seq: 1,
        };
        let emission = ForegroundMemoryEmissionV1 {
            emission_id: "emission-old".to_string(),
            session_id: "session".to_string(),
            start_seq: 1,
            end_seq: 2,
        };
        let decision = bound_memory_candidate_extraction_window(
            &cursor,
            &[
                event("session", 1, "event-1", "old"),
                event("session", 2, "event-2", "new one"),
                event("session", 3, "event-3", "new two"),
            ],
            Some(&emission),
            limits(),
            true,
        )
        .expect("bounded input is valid");
        let MemoryCandidateExtractionDecisionV1::WindowBound { window } = decision else {
            panic!("model-backed boundary should only bind new events");
        };
        assert_eq!(window.from_seq, 2);
        assert_eq!(window.to_seq, 4);
        assert_eq!(
            window.event_ids,
            vec!["event-2".to_string(), "event-3".to_string()]
        );
    }

    #[test]
    fn candidate_provenance_must_match_bounded_window() {
        let window = ExtractionWindowV1 {
            session_id: "session".to_string(),
            from_seq: 2,
            to_seq: 3,
            event_ids: vec!["event-2".to_string()],
            source_content_hashes: vec!["sha256:event-2".to_string()],
            estimated_input_tokens: 3,
        };
        let candidate = MemoryCandidateV1 {
            schema_version: MEMORY_CANDIDATE_EXTRACTION_SCHEMA_VERSION,
            candidate_id: "candidate-1".to_string(),
            session_id: "session".to_string(),
            from_seq: 2,
            to_seq: 3,
            scope_id: "scope".to_string(),
            content: "bounded candidate".to_string(),
            source_event_ids: vec!["event-other".to_string()],
            source_content_hashes: vec!["sha256:event-2".to_string()],
        };
        assert_eq!(
            candidate.validate_against(&window),
            Err(ReviewContractError::CandidateProvenanceMismatch)
        );
    }
}
