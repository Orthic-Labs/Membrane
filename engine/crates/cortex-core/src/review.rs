//! Proposal-only Cortex review boundaries.
//!
//! Stage 0 Dream remains deterministic and lives in [`crate::dream`]. This
//! module owns Stage 1's provider-output contracts and foreground gate. It
//! does not call a model or write durable memory.

use crate::{estimate_tokens, EventCursor, SessionEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const SEMANTIC_CURATION_PROPOSAL_SCHEMA_VERSION: u32 = 1;
pub const MEMORY_CANDIDATE_EXTRACTION_SCHEMA_VERSION: u32 = 1;
pub const SEMANTIC_CURATION_MAX_PROPOSALS: usize = 64;

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
