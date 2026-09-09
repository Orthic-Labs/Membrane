//! Typed, model-free records used by Cortex's absorbed session store.
//!
//! This module intentionally contains no I/O or embedding machinery.  It is the
//! validation and sequencing boundary shared by storage adapters.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const ABSORBED_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvenanceRef {
    pub source: String,
    #[serde(default)]
    pub source_event_ids: Vec<String>,
    #[serde(default)]
    pub producer: Option<String>,
}

/// Production provenance classification for a [`ProvenanceRef::producer`] value.
///
/// This is a read-only classifier over the already-recorded `producer` string; it
/// never invents a producer for a record that omitted one, and it never widens
/// `producer` beyond the plain string the caller supplied. `Unavailable` is the
/// explicit, typed outcome when the field is absent — callers must not substitute
/// a guessed producer for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProducerClass {
    /// `semantic-producer`: derived by the analyzer pipeline, not a direct observation.
    Analyzer,
    /// `coderight-v7`: a real host-observed signal.
    Observed,
    /// A non-empty producer string that is not one of the known canonical producers.
    /// Recorded as-is; classification stays unresolved rather than guessed.
    Unclassified,
    /// No `producer` was recorded on this reference. Distinct from `Unclassified`
    /// so an absent input is never conflated with an unrecognized one.
    Unavailable,
}

/// The producer identifiers Cortex currently recognizes as classified sources.
/// `semantic-producer` is the analyzer path; `coderight-v7` is the real host
/// telemetry/observation path (CTX031). Any other non-empty value is recorded
/// verbatim but classified `Unclassified` rather than guessed into one of these.
pub const KNOWN_PRODUCERS: &[&str] = &["semantic-producer", "coderight-v7"];

/// Classify a [`ProvenanceRef`] by its `producer` field. Pure and total: every
/// input, including `None`, maps to a typed outcome — there is no panic or
/// default-guess path.
pub fn classify_producer(reference: &ProvenanceRef) -> ProducerClass {
    match reference.producer.as_deref().map(str::trim) {
        None => ProducerClass::Unavailable,
        Some("") => ProducerClass::Unavailable,
        Some(value) if value == KNOWN_PRODUCERS[0] => ProducerClass::Analyzer,
        Some(value) if value == KNOWN_PRODUCERS[1] => ProducerClass::Observed,
        Some(_) => ProducerClass::Unclassified,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordGovernance {
    pub scope_id: String,
    pub authority: String,
    pub influence_class: String,
    pub lifecycle: String,
    pub retention: String,
    #[serde(default)]
    pub provenance: Vec<ProvenanceRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRecord {
    pub schema_version: u32,
    pub session_id: String,
    pub scope_id: String,
    pub workspace_root: Option<String>,
    pub permission_mode: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub status: String,
    pub title: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub authority: String,
    pub influence_class: String,
    pub lifecycle: String,
    pub retention: String,
    #[serde(default)]
    pub provenance: Vec<ProvenanceRef>,
    pub content_hash: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub started_at_ms: Option<u64>,
    pub ended_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionEvent {
    pub schema_version: u32,
    pub session_id: String,
    pub seq: u64,
    pub event_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: serde_json::Value,
    pub scope_id: String,
    pub authority: String,
    pub influence_class: String,
    pub lifecycle: String,
    pub retention: String,
    #[serde(default)]
    pub provenance: Vec<ProvenanceRef>,
    pub occurred_at_ms: u64,
    pub recorded_at_ms: u64,
    pub content_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskRecord {
    pub schema_version: u32,
    pub task_id: String,
    pub session_id: String,
    pub scope_id: String,
    pub authority: String,
    pub influence_class: String,
    pub lifecycle: String,
    pub retention: String,
    #[serde(default)]
    pub provenance: Vec<ProvenanceRef>,
    pub status: String,
    pub title: String,
    pub goal: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub completed_at_ms: Option<u64>,
    pub content_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactRecord {
    pub schema_version: u32,
    pub artifact_id: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub handle: String,
    pub content_hash: String,
    pub media_type: String,
    pub byte_length: u64,
    pub scope_id: String,
    pub authority: String,
    pub influence_class: String,
    pub lifecycle: String,
    pub retention: String,
    #[serde(default)]
    pub provenance: Vec<ProvenanceRef>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventCursor {
    pub session_id: String,
    pub last_seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AbsorbedValidationError {
    #[error("record schema version is unsupported")]
    SchemaVersion,
    #[error("record identity is empty")]
    EmptyIdentity,
    #[error("record governance field is empty")]
    EmptyGovernance,
    #[error("event session identity does not match")]
    SessionMismatch,
    #[error("event sequence must be positive")]
    InvalidSequence,
    #[error("event sequence expected {expected}, got {actual}")]
    SequenceGap { expected: u64, actual: u64 },
    #[error("duplicate event id: {0}")]
    DuplicateEventId(String),
    #[error("duplicate event sequence: {0}")]
    DuplicateSequence(u64),
    #[error("event sequence is out of order")]
    Reordered,
    #[error("event content hash is empty")]
    EmptyContentHash,
    #[error("event payload must be an object")]
    InvalidPayload,
}

pub fn validate_governance(value: &RecordGovernance) -> Result<(), AbsorbedValidationError> {
    if value.scope_id.trim().is_empty()
        || value.authority.trim().is_empty()
        || value.influence_class.trim().is_empty()
        || value.lifecycle.trim().is_empty()
        || value.retention.trim().is_empty()
    {
        return Err(AbsorbedValidationError::EmptyGovernance);
    }
    if value
        .provenance
        .iter()
        .any(|item| item.source.trim().is_empty())
    {
        return Err(AbsorbedValidationError::EmptyGovernance);
    }
    // A `producer` field that is present but blank is worse than an absent one: it
    // reads as classified while carrying no real identity. Reject it here rather
    // than let a downstream classifier silently treat it as `Unavailable`.
    if value
        .provenance
        .iter()
        .any(|item| matches!(item.producer.as_deref(), Some(p) if p.trim().is_empty()))
    {
        return Err(AbsorbedValidationError::EmptyGovernance);
    }
    Ok(())
}

fn validate_record_fields(
    scope_id: &str,
    authority: &str,
    influence_class: &str,
    lifecycle: &str,
    retention: &str,
    provenance: &[ProvenanceRef],
) -> Result<(), AbsorbedValidationError> {
    validate_governance(&RecordGovernance {
        scope_id: scope_id.to_string(),
        authority: authority.to_string(),
        influence_class: influence_class.to_string(),
        lifecycle: lifecycle.to_string(),
        retention: retention.to_string(),
        provenance: provenance.to_vec(),
    })
}

pub fn validate_event(event: &SessionEvent) -> Result<(), AbsorbedValidationError> {
    if event.schema_version != ABSORBED_SCHEMA_VERSION {
        return Err(AbsorbedValidationError::SchemaVersion);
    }
    if event.session_id.trim().is_empty() || event.event_id.trim().is_empty() {
        return Err(AbsorbedValidationError::EmptyIdentity);
    }
    if event.seq == 0 {
        return Err(AbsorbedValidationError::InvalidSequence);
    }
    if event.content_hash.trim().is_empty() {
        return Err(AbsorbedValidationError::EmptyContentHash);
    }
    if !event.payload.is_object() {
        return Err(AbsorbedValidationError::InvalidPayload);
    }
    validate_record_fields(
        &event.scope_id,
        &event.authority,
        &event.influence_class,
        &event.lifecycle,
        &event.retention,
        &event.provenance,
    )
}

/// Validate an imported stream.  Streams are one session, strictly contiguous,
/// and ordered.  Sequence one is the first durable event; tombstoned numbers
/// remain represented by the caller's existing cursor and cannot be reused.
pub fn validate_event_import(events: &[SessionEvent]) -> Result<(), AbsorbedValidationError> {
    let mut ids = HashSet::with_capacity(events.len());
    let mut session: Option<&str> = None;
    let mut expected = 1u64;
    for event in events {
        validate_event(event)?;
        if let Some(current) = session {
            if current != event.session_id {
                return Err(AbsorbedValidationError::SessionMismatch);
            }
        } else {
            session = Some(&event.session_id);
        }
        if !ids.insert(event.event_id.as_str()) {
            return Err(AbsorbedValidationError::DuplicateEventId(
                event.event_id.clone(),
            ));
        }
        if event.seq != expected {
            if event.seq < expected {
                return Err(if event.seq == expected.saturating_sub(1) {
                    AbsorbedValidationError::DuplicateSequence(event.seq)
                } else {
                    AbsorbedValidationError::Reordered
                });
            }
            return Err(AbsorbedValidationError::SequenceGap {
                expected,
                actual: event.seq,
            });
        }
        expected = expected.saturating_add(1);
    }
    Ok(())
}

/// Deterministic SHA-256 over canonical JSON.  This is useful for callers
/// constructing records and never performs an embedding/model call.
pub fn content_hash<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(value)?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

pub fn event_range(events: &[SessionEvent], start_seq: u64, end_seq: u64) -> Vec<SessionEvent> {
    events
        .iter()
        .filter(|event| event.seq >= start_seq && event.seq < end_seq)
        .cloned()
        .collect()
}

#[cfg(test)]
mod provenance_classification_tests {
    use super::*;

    fn governance_with_producer(producer: Option<&str>) -> RecordGovernance {
        RecordGovernance {
            scope_id: "scope-1".to_string(),
            authority: "authority-1".to_string(),
            influence_class: "advisory".to_string(),
            lifecycle: "active".to_string(),
            retention: "standard".to_string(),
            provenance: vec![ProvenanceRef {
                source: "source-1".to_string(),
                source_event_ids: vec![],
                producer: producer.map(str::to_string),
            }],
        }
    }

    #[test]
    fn classifies_semantic_producer_as_analyzer() {
        let reference = ProvenanceRef {
            source: "s".to_string(),
            source_event_ids: vec![],
            producer: Some("semantic-producer".to_string()),
        };
        assert_eq!(classify_producer(&reference), ProducerClass::Analyzer);
    }

    #[test]
    fn classifies_coderight_v7_as_observed() {
        let reference = ProvenanceRef {
            source: "s".to_string(),
            source_event_ids: vec![],
            producer: Some("coderight-v7".to_string()),
        };
        assert_eq!(classify_producer(&reference), ProducerClass::Observed);
    }

    #[test]
    fn classifies_unknown_producer_as_unclassified_not_guessed() {
        let reference = ProvenanceRef {
            source: "s".to_string(),
            source_event_ids: vec![],
            producer: Some("some-other-tool".to_string()),
        };
        assert_eq!(classify_producer(&reference), ProducerClass::Unclassified);
    }

    #[test]
    fn classifies_absent_producer_as_unavailable() {
        let reference = ProvenanceRef {
            source: "s".to_string(),
            source_event_ids: vec![],
            producer: None,
        };
        assert_eq!(classify_producer(&reference), ProducerClass::Unavailable);
    }

    /// Negative control: a blank-but-present producer must NOT silently classify as
    /// `Unavailable` via governance validation — it must fail validation outright,
    /// because a present-but-empty producer is a guessed/corrupt input, not a typed
    /// absence. If this passes validation, the injected fault (blank producer) goes
    /// undetected.
    #[test]
    fn negative_control_blank_producer_fails_governance_validation() {
        let governance = governance_with_producer(Some("   "));
        assert_eq!(
            validate_governance(&governance),
            Err(AbsorbedValidationError::EmptyGovernance)
        );
    }

    #[test]
    fn absent_producer_passes_governance_validation() {
        let governance = governance_with_producer(None);
        assert!(validate_governance(&governance).is_ok());
    }
}
