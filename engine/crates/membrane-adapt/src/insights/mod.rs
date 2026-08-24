//! Insights — failure-pattern and gotcha learning (canon §6).
//!
//! Report/diagnostic only by default. An Insight states what was observed and
//! what the evidence supports; it never establishes user intent, never grants
//! permission, and never auto-admits to Cortex.

pub mod detectors;
pub mod guards;
pub mod recurrence;
pub mod sealed_issue;

use serde::{Deserialize, Serialize};

/// Internal domain-contract schema tags. Adapt-internal V1 schemas, not
/// public protocol shapes.
pub const FAILURE_EPISODE_SCHEMA: &str = "adapt.failure-episode.v1";
pub const INSIGHT_ISSUE_SCHEMA: &str = "adapt.insight-issue.v1";

pub const HONESTY_LIMIT: &str = "Insights detects only observable failure signals in transcripts. Episodes/issues are heuristic; 'likelyMechanism' and 'suggestedRemediations' are candidate inferences, not authoritative diagnoses.";

/// Minimal internal transcript-event input. The native transcript owner owns
/// production of these; Adapt consumes them read-only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEventV1 {
    pub event_id: String,
    pub session_id: String,
    pub host: String,
    /// `external_user`, `assistant`, `tool`, `unknown`, ...
    pub provenance: String,
    pub kind: EventKind,
    pub text: String,
    #[serde(default)]
    pub timestamp: Option<String>,
    pub byte_start: i64,
    pub byte_end: i64,
    #[serde(default)]
    pub call_id: Option<String>,
    #[serde(default)]
    pub occurrence: u32,
    #[serde(default = "default_true")]
    pub evidence_eligible: bool,
}

fn default_true() -> bool {
    true
}

impl TryFrom<&membrane_transcript::TranscriptEventV1> for TranscriptEventV1 {
    type Error = String;

    fn try_from(event: &membrane_transcript::TranscriptEventV1) -> Result<Self, Self::Error> {
        let kind = match event.kind.as_str() {
            "user_message" => EventKind::UserMessage,
            "assistant_message" => EventKind::AssistantMessage,
            "tool_call" => EventKind::ToolCall,
            "tool_result" => EventKind::ToolResult,
            other => return Err(format!("unsupported Adapt event kind: {other}")),
        };
        let byte_start = i64::try_from(event.byte_start)
            .map_err(|_| "byte_start exceeds Adapt V1 range".to_string())?;
        let byte_end = i64::try_from(event.byte_end)
            .map_err(|_| "byte_end exceeds Adapt V1 range".to_string())?;
        let occurrence = u32::try_from(event.occurrence.unwrap_or_default())
            .map_err(|_| "occurrence exceeds Adapt V1 range".to_string())?;
        let provenance = match kind {
            EventKind::UserMessage if event.role.as_deref() == Some("user") => "external_user",
            EventKind::UserMessage => "unknown",
            EventKind::AssistantMessage => "assistant",
            EventKind::ToolCall | EventKind::ToolResult => "tool",
        };
        Ok(Self {
            event_id: event.event_id.clone(),
            session_id: event.session_id.clone(),
            host: event.host.clone(),
            provenance: provenance.to_string(),
            kind,
            text: event.text.clone(),
            timestamp: event.timestamp.clone(),
            byte_start,
            byte_end,
            call_id: event.call_id.clone(),
            occurrence,
            evidence_eligible: !event.synthetic
                && !event.meta
                && !event.private_reasoning_omitted
                && !event.flags.synthetic
                && !event.flags.meta
                && !event.flags.private_reasoning_omitted,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    UserMessage,
    AssistantMessage,
    ToolCall,
    ToolResult,
}

impl TranscriptEventV1 {
    pub fn is_user(&self) -> bool {
        self.kind == EventKind::UserMessage && self.provenance == "external_user"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

/// Fixed, narrow disposition vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserDisposition {
    Logged,
    Forgiven,
    Repeated,
    Escalated,
    PostmortemRequested,
}

/// One byte-span evidence entry bound to exact source bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeEvidenceSpan {
    pub event_id: String,
    pub kind: String,
    pub session_id: String,
    pub host: String,
    pub byte_start: i64,
    pub byte_end: i64,
    /// Short excerpt, bounded at 240 chars plus ellipsis marker.
    pub text: String,
}

/// `FailureEpisodeV1` — one detected occurrence of a failure mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FailureEpisodeV1 {
    pub schema_version: String,
    pub episode_id: String,
    /// Detector slug / canonical family name.
    pub family: String,
    pub severity: Severity,
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    pub sessions: Vec<String>,
    pub hosts: Vec<String>,
    pub agents: Vec<String>,
    /// Deterministic signature within the family used for recurrence
    /// grouping (e.g., normalized subject of the repeated ask).
    pub signature: String,
    pub user_expectation: String,
    pub observed_failure: String,
    pub evidence: Vec<EpisodeEvidenceSpan>,
    /// Labelled inference, never authoritative.
    #[serde(default)]
    pub likely_mechanism: String,
    #[serde(default)]
    pub suggested_remediations: Vec<String>,
    pub user_disposition: UserDisposition,
    pub honesty_limit: String,
}

impl FailureEpisodeV1 {
    pub fn new(
        detector: &str,
        severity: Severity,
        confidence: f64,
        signature: &str,
        observed_failure: &str,
        user_expectation: &str,
        evidence_events: &[&TranscriptEventV1],
    ) -> Self {
        let spans: Vec<(String, i64, i64)> = evidence_events
            .iter()
            .map(|ev| (ev.event_id.clone(), ev.byte_start, ev.byte_end))
            .collect();
        let episode_id = crate::canonical::derive_episode_id(detector, &spans);
        let mut sessions: Vec<String> = evidence_events.iter().map(|e| e.session_id.clone()).collect();
        sessions.sort();
        sessions.dedup();
        let mut hosts: Vec<String> = evidence_events.iter().map(|e| e.host.clone()).collect();
        hosts.sort();
        hosts.dedup();
        let evidence = evidence_events
            .iter()
            .filter(|e| e.evidence_eligible)
            .map(|ev| {
                let mut text = ev.text.chars().take(240).collect::<String>();
                if ev.text.chars().count() > 240 {
                    text.push('\u{2026}');
                }
                EpisodeEvidenceSpan {
                    event_id: ev.event_id.clone(),
                    kind: format!("{:?}", ev.kind).to_lowercase(),
                    session_id: ev.session_id.clone(),
                    host: ev.host.clone(),
                    byte_start: ev.byte_start,
                    byte_end: ev.byte_end,
                    text,
                }
            })
            .collect();
        let timestamps: Vec<&String> =
            evidence_events.iter().filter_map(|e| e.timestamp.as_ref()).collect();
        Self {
            schema_version: FAILURE_EPISODE_SCHEMA.to_string(),
            episode_id,
            family: detector.to_string(),
            severity,
            confidence: confidence.clamp(0.0, 1.0),
            timestamp: timestamps.iter().min().map(|s| (*s).clone()),
            sessions,
            hosts,
            agents: vec![],
            signature: signature.to_string(),
            user_expectation: user_expectation.to_string(),
            observed_failure: observed_failure.to_string(),
            evidence,
            likely_mechanism: String::new(),
            suggested_remediations: vec![],
            user_disposition: UserDisposition::Logged,
            honesty_limit: HONESTY_LIMIT.to_string(),
        }
    }

    /// Nearest preceding user message text for context/expectation.
    pub fn nearest_user_text(events: &[TranscriptEventV1], index: usize) -> String {
        let start = index.saturating_sub(6);
        for ev in events[start..index].iter().rev() {
            if ev.is_user() && !ev.text.trim().is_empty() {
                return ev.text.trim().chars().take(200).collect();
            }
        }
        String::new()
    }
}

/// Issue lifecycle (canon §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueState {
    Observed,
    Recurring,
    Confirmed,
    MitigationProposed,
    Mitigated,
    Reopened,
    Obsolete,
    Dismissed,
}

impl IssueState {
    pub fn can_transition_to(self, target: IssueState) -> bool {
        use IssueState::*;
        matches!(
            (self, target),
            (Observed, Recurring)
                | (Observed, Dismissed)
                | (Observed, Obsolete)
                | (Recurring, Confirmed)
                | (Recurring, Dismissed)
                | (Recurring, Obsolete)
                | (Confirmed, MitigationProposed)
                | (Confirmed, Obsolete)
                | (Confirmed, Dismissed)
                | (MitigationProposed, Mitigated)
                | (MitigationProposed, Confirmed)
                | (Mitigated, Reopened)
                | (Mitigated, Obsolete)
                | (Reopened, MitigationProposed)
                | (Reopened, Dismissed)
        )
    }
}

/// `InsightIssueV1` — longitudinal issue formed from recurring episodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsightIssueV1 {
    pub schema_version: String,
    pub issue_id: String,
    pub family: String,
    /// Deterministic recurrence-group signature, sealed with semantics.
    pub recurrence_signature: String,
    pub canonical_description: String,
    /// Applicability dimensions (model/client/repo/tool where known).
    pub applicability: std::collections::BTreeMap<String, String>,
    /// Episode IDs and their digests — underlying episodes are preserved.
    pub episode_ids: Vec<String>,
    pub recurrence_count: u32,
    pub distinct_sessions: u32,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
    pub confidence: f64,
    pub state: IssueState,
    /// Candidate mechanisms — labelled inference.
    pub candidate_mechanisms: Vec<String>,
    /// Linked remediation proposal IDs (see `remediation`).
    pub mitigation_links: Vec<String>,
    pub recurrence_after_mitigation: u32,
    pub honesty_limit: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: &str, session: &str, kind: EventKind, text: &str) -> TranscriptEventV1 {
        TranscriptEventV1 {
            event_id: id.into(),
            session_id: session.into(),
            host: "pi".into(),
            provenance: if kind == EventKind::UserMessage { "external_user".into() } else { "assistant".into() },
            kind,
            text: text.into(),
            timestamp: Some("2026-08-24T01:00:00Z".into()),
            byte_start: 0,
            byte_end: text.len() as i64,
            call_id: None,
            occurrence: 0,
            evidence_eligible: true,
        }
    }

    #[test]
    fn episode_ids_are_deterministic_per_evidence() {
        let e1 = ev("a", "s1", EventKind::AssistantMessage, "claim");
        let ep1 = FailureEpisodeV1::new("f", Severity::Low, 0.5, "sig", "obs", "", &[&e1]);
        let ep2 = FailureEpisodeV1::new("f", Severity::Low, 0.5, "sig", "obs", "", &[&e1]);
        assert_eq!(ep1.episode_id, ep2.episode_id);
        let e2 = ev("b", "s1", EventKind::AssistantMessage, "claim");
        let ep3 = FailureEpisodeV1::new("f", Severity::Low, 0.5, "sig", "obs", "", &[&e2]);
        assert_ne!(ep1.episode_id, ep3.episode_id);
    }

    #[test]
    fn illegal_issue_transitions_refused() {
        assert!(!IssueState::Observed.can_transition_to(IssueState::Mitigated));
        assert!(!IssueState::Dismissed.can_transition_to(IssueState::Recurring));
        assert!(IssueState::Mitigated.can_transition_to(IssueState::Reopened));
    }
}
