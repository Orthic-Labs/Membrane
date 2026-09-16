//! Deterministic first-party behavioral learner semantics for the admitted
//! `adapt_behavioral_review` background job (ADP-035).
//!
//! This module is the learner, not the scheduler: it consumes one bounded,
//! already-validated event window and emits proposal-only records. It never
//! writes durable truth, never injects Pull candidates, and never mutates a
//! cursor — the daemon owns cursor advancement and only after a proposal has
//! been handed to the durable proposal sink.
//!
//! Honesty contract:
//! - findings are typed recurrence facts over exact event identities/digests;
//! - a missing or malformed window is a typed unavailable, never an empty
//!   proposal list dressed up as a clean review;
//! - proposal identity is derived from content (scope, session, class,
//!   evidence) so retries and re-drains are idempotent no-ops;
//! - summaries carry no raw transcript text, only typed facts and digests.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Contract carried on every proposal this learner emits.
pub const ADAPT_LEARNER_CONTRACT: &str = "adapt.learner-proposal.v1";
/// Proposal records are bounded by the background-review frame; keep the
/// learner's own ceiling strictly lower so provider framing headroom remains.
pub const MAX_LEARNER_EVENTS: usize = 512;
pub const MAX_LEARNER_PROPOSALS: usize = 32;
pub const MAX_PROPOSAL_EVIDENCE_REFS: usize = 64;
/// Minimum exact recurrences before a behavioral finding may be proposed.
pub const MIN_RECURRENCE: usize = 2;
/// This learner reads typed event facts only; it is not a model and does not
/// claim one.
pub const LEARNER_ANALYZER_ID: &str = "adapt-behavioral-learner";
pub const LEARNER_ANALYZER_VERSION: u32 = 1;

/// Protocol-neutral projection of one session event supplied by the daemon's
/// request. The caller performs the wire-to-learner mapping; this crate does
/// not depend on the wire schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearnerEventV1 {
    pub event_id: String,
    pub seq: u64,
    pub event_type: String,
    pub scope_id: String,
    pub content_hash: String,
    pub occurred_at_ms: u64,
    /// Untrusted event payload. The learner reads typed fields only and never
    /// copies payload text into a proposal.
    pub payload: Value,
}

/// Bounded input for one admitted `adapt_behavioral_review` run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdaptLearnerInputV1 {
    pub job_id: String,
    pub session_id: String,
    pub scope_id: String,
    /// Last consumed sequence for this session; every event must be newer.
    pub cursor_last_seq: u64,
    pub events: Vec<LearnerEventV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnerUnavailableReason {
    /// No events were supplied for the admitted window.
    EmptyWindow,
    /// Event rows violated session, ordering, scope, or bound invariants.
    InvalidInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdaptLearnerStatusV1 {
    /// One or more bounded proposals were produced.
    Proposals,
    /// The window was examined and no committed finding recurred. The caller
    /// must not advance the cursor: proposal handoff is the only consumption
    /// receipt this protocol admits.
    NoFindings,
    /// Input was missing or invalid; nothing was learned.
    Unavailable { reason: LearnerUnavailableReason },
}

/// Closed set of v1 behavioral finding classes. Each names an observed
/// recurrence fact, not a diagnosis or an approved change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnerFindingClass {
    /// The same deterministic detector emitted failure episodes in two or
    /// more distinct reviewed windows.
    RepeatedDetectorEpisodes,
    /// Detector coverage reported unavailable/failed with the same missing
    /// field set across two or more windows — a repeated evidence gap, not a
    /// "no finding".
    RepeatedDetectorCoverageGap,
    /// The same typed event content was emitted under two or more distinct
    /// event identities in one window.
    RepeatedIdenticalEmission,
}

impl LearnerFindingClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RepeatedDetectorEpisodes => "repeated_detector_episodes",
            Self::RepeatedDetectorCoverageGap => "repeated_detector_coverage_gap",
            Self::RepeatedIdenticalEmission => "repeated_identical_emission",
        }
    }

    const fn honesty_limit(self) -> &'static str {
        match self {
            Self::RepeatedDetectorEpisodes => {
                "Recurrence of typed detector episodes across reviewed windows. \
                 Root cause, user preference, and prevented failure are not inferred."
            }
            Self::RepeatedDetectorCoverageGap => {
                "The same required host facts were repeatedly unavailable. Whether \
                 the host cannot supply them is not inferred."
            }
            Self::RepeatedIdenticalEmission => {
                "Identical typed event content recurred under distinct event \
                 identities. Whether the repetition was erroneous is not inferred."
            }
        }
    }
}

/// Content-free evidence bound to one learner proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearnerEvidenceV1 {
    pub source_event_ids: Vec<String>,
    pub source_content_hashes: Vec<String>,
}

/// One bounded, proposal-only learner finding. The record carries no durable
/// authority: it can only be written to the proposal sink for governed
/// Cortex review.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdaptLearnerProposalV1 {
    pub schema_version: u32,
    pub contract: String,
    /// Deterministic content-derived identity: retries and re-drains collapse.
    pub proposal_id: String,
    /// Provenance only; not part of proposal identity.
    pub job_id: String,
    pub session_id: String,
    pub scope_id: String,
    pub finding_class: LearnerFindingClass,
    /// Bounded, content-free summary: typed facts and counts only.
    pub summary: String,
    pub evidence: LearnerEvidenceV1,
    pub first_seq: u64,
    pub last_seq: u64,
    pub occurrence_count: u32,
    pub honesty_limit: String,
    /// Hard marker so a consumer cannot mistake this for durable truth.
    pub boundary: String,
}

impl AdaptLearnerProposalV1 {
    /// Recompute the content-derived identity for integrity checking. Every
    /// semantic field is covered, so any post-emission mutation is detectable.
    fn derived_id(&self) -> String {
        derive_proposal_id(
            &self.scope_id,
            &self.session_id,
            self.finding_class,
            &self.summary,
            &self.evidence,
            self.first_seq,
            self.last_seq,
            self.occurrence_count,
        )
    }

    /// Structural validation a sink/drain can run without trusting the
    /// producer. Identity re-derivation makes forged or mutated proposals
    /// detectable rather than silently re-queued.
    pub fn validate(&self) -> Result<(), LearnerValidationError> {
        if self.schema_version != 1 {
            return Err(LearnerValidationError::SchemaVersion);
        }
        if self.contract != ADAPT_LEARNER_CONTRACT || self.boundary != "proposal_only" {
            return Err(LearnerValidationError::ContractMismatch);
        }
        for value in [
            &self.proposal_id,
            &self.job_id,
            &self.session_id,
            &self.scope_id,
            &self.summary,
            &self.honesty_limit,
        ] {
            if value.trim().is_empty() {
                return Err(LearnerValidationError::EmptyField);
            }
        }
        if self.honesty_limit != self.finding_class.honesty_limit() {
            return Err(LearnerValidationError::HonestyLimitMismatch);
        }
        if self.evidence.source_event_ids.is_empty()
            || self.evidence.source_event_ids.len() > MAX_PROPOSAL_EVIDENCE_REFS
            || self.evidence.source_content_hashes.len() > MAX_PROPOSAL_EVIDENCE_REFS
        {
            return Err(LearnerValidationError::InvalidEvidence);
        }
        if self.evidence.source_event_ids.iter().any(|id| id.trim().is_empty())
            || self
                .evidence
                .source_content_hashes
                .iter()
                .any(|hash| hash.trim().is_empty())
        {
            return Err(LearnerValidationError::InvalidEvidence);
        }
        // Evidence refs are a bounded sample; occurrence_count records the
        // true observed count and may exceed the sampled evidence length.
        if self.first_seq == 0
            || self.last_seq < self.first_seq
            || self.occurrence_count < MIN_RECURRENCE as u32
            || self.occurrence_count as usize > MAX_LEARNER_EVENTS
        {
            return Err(LearnerValidationError::InvalidWindow);
        }
        if self.proposal_id != self.derived_id() {
            return Err(LearnerValidationError::IdentityMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearnerValidationError {
    SchemaVersion,
    ContractMismatch,
    EmptyField,
    HonestyLimitMismatch,
    InvalidEvidence,
    InvalidWindow,
    IdentityMismatch,
}

/// Result of one learner run. `consumed_through_seq` is informational for the
/// caller's cursor decision; the daemon still advances only after proposal
/// handoff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdaptLearnerResultV1 {
    pub status: AdaptLearnerStatusV1,
    pub analyzer: String,
    pub proposals: Vec<AdaptLearnerProposalV1>,
    pub consumed_through_seq: u64,
}

#[allow(clippy::too_many_arguments)]
fn derive_proposal_id(
    scope_id: &str,
    session_id: &str,
    finding_class: LearnerFindingClass,
    summary: &str,
    evidence: &LearnerEvidenceV1,
    first_seq: u64,
    last_seq: u64,
    occurrence_count: u32,
) -> String {
    format!(
        "alp_{}",
        crate::canonical::sha256_canonical(&json!([
            ADAPT_LEARNER_CONTRACT,
            scope_id,
            session_id,
            finding_class.as_str(),
            summary,
            evidence.source_event_ids,
            evidence.source_content_hashes,
            first_seq,
            last_seq,
            occurrence_count,
        ]))
    )
}

fn bounded_ids<'a>(ids: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut out: Vec<String> = ids.cloned().collect();
    out.sort();
    out.dedup();
    out.truncate(MAX_PROPOSAL_EVIDENCE_REFS);
    out
}

fn make_proposal(
    input: &AdaptLearnerInputV1,
    finding_class: LearnerFindingClass,
    summary: String,
    members: &[&LearnerEventV1],
) -> AdaptLearnerProposalV1 {
    let evidence = LearnerEvidenceV1 {
        source_event_ids: bounded_ids(members.iter().map(|event| &event.event_id)),
        source_content_hashes: bounded_ids(members.iter().map(|event| &event.content_hash)),
    };
    let first_seq = members.iter().map(|event| event.seq).min().unwrap_or(0);
    let last_seq = members.iter().map(|event| event.seq).max().unwrap_or(0);
    let proposal_id = derive_proposal_id(
        &input.scope_id,
        &input.session_id,
        finding_class,
        &summary,
        &evidence,
        first_seq,
        last_seq,
        members.len() as u32,
    );
    AdaptLearnerProposalV1 {
        schema_version: 1,
        contract: ADAPT_LEARNER_CONTRACT.into(),
        proposal_id,
        job_id: input.job_id.clone(),
        session_id: input.session_id.clone(),
        scope_id: input.scope_id.clone(),
        finding_class,
        summary,
        evidence,
        first_seq,
        last_seq,
        occurrence_count: members.len() as u32,
        honesty_limit: finding_class.honesty_limit().into(),
        boundary: "proposal_only".into(),
    }
}

/// Event type written by `adapt_observations` for one analyzed window.
const DETECTOR_STATE_EVENT: &str = "adapt.detector_state";

/// Typed payload states treated as coverage gaps rather than findings.
const COVERAGE_GAP_STATES: &[&str] = &["unavailable", "failed"];

fn validate_input(input: &AdaptLearnerInputV1) -> Result<(), LearnerUnavailableReason> {
    if input.job_id.trim().is_empty()
        || input.session_id.trim().is_empty()
        || input.scope_id.trim().is_empty()
    {
        return Err(LearnerUnavailableReason::InvalidInput);
    }
    if input.events.len() > MAX_LEARNER_EVENTS {
        return Err(LearnerUnavailableReason::InvalidInput);
    }
    let mut prior_seq = input.cursor_last_seq;
    for event in &input.events {
        if event.event_id.trim().is_empty()
            || event.event_type.trim().is_empty()
            || event.scope_id.trim().is_empty()
            || event.content_hash.trim().is_empty()
            || !event.payload.is_object()
            || event.seq == 0
            || event.seq <= prior_seq
        {
            return Err(LearnerUnavailableReason::InvalidInput);
        }
        prior_seq = event.seq;
    }
    Ok(())
}

/// Execute learner semantics for one admitted window. Pure and deterministic:
/// identical input always yields identical proposals and identities.
pub fn run_adapt_behavioral_review(input: &AdaptLearnerInputV1) -> AdaptLearnerResultV1 {
    let consumed_through_seq = input
        .events
        .last()
        .map(|event| event.seq)
        .unwrap_or(input.cursor_last_seq);
    let result = |status, proposals| AdaptLearnerResultV1 {
        status,
        analyzer: format!("{LEARNER_ANALYZER_ID}@{LEARNER_ANALYZER_VERSION}"),
        proposals,
        consumed_through_seq,
    };
    if let Err(reason) = validate_input(input) {
        return result(AdaptLearnerStatusV1::Unavailable { reason }, Vec::new());
    }
    if input.events.is_empty() {
        return result(
            AdaptLearnerStatusV1::Unavailable {
                reason: LearnerUnavailableReason::EmptyWindow,
            },
            Vec::new(),
        );
    }

    let mut proposals = Vec::new();

    // 1. Repeated detector episodes for the same deterministic detector.
    let mut episode_windows: BTreeMap<String, Vec<&LearnerEventV1>> = BTreeMap::new();
    // 2. Repeated coverage gaps with the same missing-field signature.
    let mut coverage_gaps: BTreeMap<String, Vec<&LearnerEventV1>> = BTreeMap::new();
    // 3. Identical typed content emitted under distinct event identities.
    let mut identical: BTreeMap<(String, String), Vec<&LearnerEventV1>> = BTreeMap::new();

    for event in &input.events {
        if event.event_type == DETECTOR_STATE_EVENT {
            let state = event
                .payload
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("");
            let detector = event
                .payload
                .get("detector")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let episode_count = event
                .payload
                .get("episodes")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            if state == "ran" && !detector.is_empty() && episode_count > 0 {
                episode_windows.entry(detector).or_default().push(event);
            }
            if COVERAGE_GAP_STATES.contains(&state) {
                let mut missing: Vec<String> = event
                    .payload
                    .get("missing_fields")
                    .and_then(Value::as_array)
                    .map(|rows| {
                        rows.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                missing.sort();
                missing.dedup();
                let signature = format!("{state}:{}", missing.join(","));
                coverage_gaps.entry(signature).or_default().push(event);
            }
        }
        identical
            .entry((event.event_type.clone(), event.content_hash.clone()))
            .or_default()
            .push(event);
    }

    for (detector, members) in episode_windows {
        if members.len() < MIN_RECURRENCE {
            continue;
        }
        proposals.push(make_proposal(
            input,
            LearnerFindingClass::RepeatedDetectorEpisodes,
            format!(
                "Detector {detector} emitted failure episodes in {} distinct reviewed windows; review for durable Insight formation.",
                members.len()
            ),
            &members,
        ));
    }
    for (signature, members) in coverage_gaps {
        if members.len() < MIN_RECURRENCE {
            continue;
        }
        proposals.push(make_proposal(
            input,
            LearnerFindingClass::RepeatedDetectorCoverageGap,
            format!(
                "Detector coverage reported an evidence gap ({signature}) across {} windows; review whether the host can supply the missing facts.",
                members.len()
            ),
            &members,
        ));
    }
    for ((event_type, _content_hash), members) in identical {
        if members.len() < MIN_RECURRENCE {
            continue;
        }
        proposals.push(make_proposal(
            input,
            LearnerFindingClass::RepeatedIdenticalEmission,
            format!(
                "Event type {event_type} emitted identical typed content under {} distinct event identities in this window.",
                members.len()
            ),
            &members,
        ));
    }

    proposals.sort_by(|a, b| a.proposal_id.cmp(&b.proposal_id));
    proposals.truncate(MAX_LEARNER_PROPOSALS);
    // A proposal this learner cannot itself validate must never reach the
    // sink: drop it rather than forward a malformed record.
    proposals.retain(|proposal| proposal.validate().is_ok());

    if proposals.is_empty() {
        result(AdaptLearnerStatusV1::NoFindings, proposals)
    } else {
        result(AdaptLearnerStatusV1::Proposals, proposals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(seq: u64, event_type: &str, payload: Value) -> LearnerEventV1 {
        // Content hash derives from payload bytes, mirroring a real producer.
        LearnerEventV1 {
            event_id: format!("ev-{seq}"),
            seq,
            event_type: event_type.into(),
            scope_id: "scope".into(),
            content_hash: format!(
                "sha256:{}",
                crate::canonical::sha256_canonical(&payload)
            ),
            occurred_at_ms: seq * 1000,
            payload,
        }
    }

    fn input(events: Vec<LearnerEventV1>) -> AdaptLearnerInputV1 {
        AdaptLearnerInputV1 {
            job_id: "job-1".into(),
            session_id: "session-1".into(),
            scope_id: "scope".into(),
            cursor_last_seq: 0,
            events,
        }
    }

    #[test]
    fn repeated_detector_episodes_propose() {
        let payload = |seq: u64| {
            json!({"state":"ran","detector":"required_verification_completion.v1","episodes":[{"episode_id":format!("e{seq}")}]})
        };
        let out = run_adapt_behavioral_review(&input(vec![
            event(1, DETECTOR_STATE_EVENT, payload(1)),
            event(2, DETECTOR_STATE_EVENT, payload(2)),
        ]));
        assert_eq!(out.status, AdaptLearnerStatusV1::Proposals);
        assert_eq!(out.proposals.len(), 1);
        let proposal = &out.proposals[0];
        assert_eq!(
            proposal.finding_class,
            LearnerFindingClass::RepeatedDetectorEpisodes
        );
        assert!(proposal.validate().is_ok());
        assert_eq!(proposal.boundary, "proposal_only");
    }

    #[test]
    fn proposal_identity_is_content_derived_not_job_derived() {
        let payload = json!({"state":"ran","detector":"d","episodes":[1]});
        let first = run_adapt_behavioral_review(&input(vec![
            event(1, DETECTOR_STATE_EVENT, payload.clone()),
            event(2, DETECTOR_STATE_EVENT, payload),
        ]));
        let mut second_input = input(vec![
            event(1, DETECTOR_STATE_EVENT, json!({"state":"ran","detector":"d","episodes":[1]})),
            event(2, DETECTOR_STATE_EVENT, json!({"state":"ran","detector":"d","episodes":[1]})),
        ]);
        second_input.job_id = "job-2".into();
        let second = run_adapt_behavioral_review(&second_input);
        assert_eq!(
            first.proposals[0].proposal_id,
            second.proposals[0].proposal_id,
            "re-drain/retry must collapse to one identity"
        );
    }

    #[test]
    fn empty_and_invalid_windows_are_typed_unavailable() {
        let empty = run_adapt_behavioral_review(&input(Vec::new()));
        assert_eq!(
            empty.status,
            AdaptLearnerStatusV1::Unavailable {
                reason: LearnerUnavailableReason::EmptyWindow
            }
        );
        let mut unordered = input(vec![
            event(2, "message", json!({})),
            event(1, "message", json!({})),
        ]);
        unordered.events[0].seq = 2;
        let out = run_adapt_behavioral_review(&unordered);
        assert_eq!(
            out.status,
            AdaptLearnerStatusV1::Unavailable {
                reason: LearnerUnavailableReason::InvalidInput
            }
        );
    }

    #[test]
    fn clean_window_reports_no_findings_without_consuming() {
        let out = run_adapt_behavioral_review(&input(vec![
            event(1, "assistant_message", json!({"text":"ok"})),
            event(2, "packet_delivered", json!({"k":"v"})),
        ]));
        assert_eq!(out.status, AdaptLearnerStatusV1::NoFindings);
        assert!(out.proposals.is_empty());
        assert_eq!(out.consumed_through_seq, 2);
    }

    #[test]
    fn identical_emissions_and_coverage_gaps_propose() {
        let shared = json!({"kind":"heartbeat"});
        let out = run_adapt_behavioral_review(&input(vec![
            event(1, "tool_receipt", shared.clone()),
            event(2, "tool_receipt", shared),
            event(
                3,
                DETECTOR_STATE_EVENT,
                json!({"state":"unavailable","detector":"d","missing_fields":["a","b"],"episodes":[]}),
            ),
            event(
                4,
                DETECTOR_STATE_EVENT,
                json!({"state":"unavailable","detector":"d","missing_fields":["b","a"],"episodes":[]}),
            ),
        ]));
        assert_eq!(out.status, AdaptLearnerStatusV1::Proposals);
        let classes: Vec<_> = out
            .proposals
            .iter()
            .map(|proposal| proposal.finding_class)
            .collect();
        assert!(classes.contains(&LearnerFindingClass::RepeatedIdenticalEmission));
        assert!(classes.contains(&LearnerFindingClass::RepeatedDetectorCoverageGap));
    }

    #[test]
    fn forged_identity_fails_validation() {
        let payload = json!({"state":"ran","detector":"d","episodes":[1]});
        let out = run_adapt_behavioral_review(&input(vec![
            event(1, DETECTOR_STATE_EVENT, payload.clone()),
            event(2, DETECTOR_STATE_EVENT, payload),
        ]));
        let mut forged = out.proposals[0].clone();
        forged.summary.push_str(" mutated");
        assert_eq!(
            forged.validate(),
            Err(LearnerValidationError::IdentityMismatch)
        );
        let mut boundary = out.proposals[0].clone();
        boundary.boundary = "durable".into();
        assert_eq!(
            boundary.validate(),
            Err(LearnerValidationError::ContractMismatch)
        );
    }
}
