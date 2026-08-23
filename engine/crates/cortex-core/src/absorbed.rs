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
    if value.provenance.iter().any(|item| item.source.trim().is_empty()) {
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
