//! Deterministic transcript chunking and retrieval.
//!
//! Transcript chunks are a rebuildable Cortex projection. Raw session events remain the
//! source of truth; this module never calls a model and never mutates an event.

use crate::absorbed::SessionEvent;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const TRANSCRIPT_CHUNK_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranscriptChunk {
    pub schema_version: u32,
    pub chunk_id: String,
    pub session_id: String,
    /// Inclusive first event sequence in this chunk.
    pub seq_start: u64,
    /// Exclusive event sequence boundary in this chunk.
    pub seq_end: u64,
    pub role: String,
    pub speaker: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub authority: String,
    pub scope_id: String,
    pub content_hash: String,
    pub model_provenance: Option<String>,
    pub source_event_ids: Vec<String>,
    pub content: String,
    #[serde(default)]
    pub omissions: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TranscriptChunkConfig {
    pub max_chars: usize,
}

impl Default for TranscriptChunkConfig {
    fn default() -> Self {
        Self { max_chars: 4_096 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TranscriptError {
    #[error("transcript chunk size must be positive")]
    InvalidSize,
    #[error("transcript event belongs to another session")]
    SessionMismatch,
    #[error("transcript event sequence must be positive")]
    InvalidSequence,
}

#[derive(Clone, Debug, Default)]
pub struct TranscriptChunkBuilder {
    config: TranscriptChunkConfig,
}

impl TranscriptChunkBuilder {
    pub fn new(config: TranscriptChunkConfig) -> Result<Self, TranscriptError> {
        if config.max_chars == 0 {
            return Err(TranscriptError::InvalidSize);
        }
        Ok(Self { config })
    }

    pub fn config(&self) -> TranscriptChunkConfig {
        self.config
    }

    pub fn build(
        &self,
        session_id: &str,
        events: &[SessionEvent],
    ) -> Result<Vec<TranscriptChunk>, TranscriptError> {
        if self.config.max_chars == 0 {
            return Err(TranscriptError::InvalidSize);
        }
        let mut chunks = Vec::new();
        let mut current = Vec::new();
        let mut current_len = 0usize;
        for event in events {
            if event.session_id != session_id {
                return Err(TranscriptError::SessionMismatch);
            }
            if event.seq == 0 {
                return Err(TranscriptError::InvalidSequence);
            }
            let text = event_text(event);
            let projected = current_len.saturating_add(text.len()).saturating_add(
                if current.is_empty() { 0 } else { 1 },
            );
            if !current.is_empty() && projected > self.config.max_chars {
                chunks.push(make_chunk(session_id, &current));
                current.clear();
                current_len = 0;
            }
            current_len = current_len
                .saturating_add(text.len())
                .saturating_add(if current.is_empty() { 0 } else { 1 });
            current.push(event);
        }
        if !current.is_empty() {
            chunks.push(make_chunk(session_id, &current));
        }
        // A size boundary can hide a sequence gap because each chunk is assembled independently.
        // Attach cross-boundary omissions to the first chunk that cannot account for them.
        for index in 1..chunks.len() {
            let previous_end = chunks[index - 1].seq_end;
            let next_start = chunks[index].seq_start;
            if next_start > previous_end {
                chunks[index].omissions.push(format!(
                    "missing event sequence {previous_end}..{next_start}"
                ));
            }
        }
        Ok(chunks)
    }
}

impl TranscriptChunk {
    pub fn is_complete(&self) -> bool {
        self.omissions.is_empty()
    }
}

pub fn build_transcript_chunks(
    session_id: &str,
    events: &[SessionEvent],
    config: TranscriptChunkConfig,
) -> Result<Vec<TranscriptChunk>, TranscriptError> {
    TranscriptChunkBuilder::new(config)?.build(session_id, events)
}

fn make_chunk(session_id: &str, events: &[&SessionEvent]) -> TranscriptChunk {
    let first = events[0];
    let last = events[events.len() - 1];
    let mut content = String::new();
    let mut source_event_ids = Vec::with_capacity(events.len());
    let mut omissions = Vec::new();
    let mut previous: Option<u64> = None;
    for event in events {
        if let Some(previous) = previous {
            if event.seq != previous.saturating_add(1) {
                omissions.push(format!("missing event sequence {}..{}", previous + 1, event.seq));
            }
        }
        previous = Some(event.seq);
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&event_text(event));
        source_event_ids.push(event.event_id.clone());
    }
    let role = payload_string(first, "role").unwrap_or_else(|| first.event_type.clone());
    let speaker = payload_string(first, "speaker")
        .or_else(|| payload_string(first, "author"))
        .unwrap_or_else(|| role.clone());
    let model_provenance = events
        .iter()
        .filter_map(|event| {
            payload_string(event, "modelProvenance").or_else(|| payload_string(event, "model"))
        })
        .next();
    let authority = uniform(events.iter().map(|event| event.authority.as_str()), "mixed");
    let scope_id = uniform(events.iter().map(|event| event.scope_id.as_str()), "mixed");
    let content_hash = sha256(&content);
    let chunk_id = format!("{session_id}:{}:{}", first.seq, last.seq.saturating_add(1));
    TranscriptChunk {
        schema_version: TRANSCRIPT_CHUNK_SCHEMA_VERSION,
        chunk_id,
        session_id: session_id.to_owned(),
        seq_start: first.seq,
        seq_end: last.seq.saturating_add(1),
        role,
        speaker,
        started_at_ms: first.occurred_at_ms,
        ended_at_ms: last.occurred_at_ms,
        authority,
        scope_id,
        content_hash,
        model_provenance,
        source_event_ids,
        content,
        omissions,
    }
}

fn uniform<'a, I>(values: I, mixed: &str) -> String
where
    I: IntoIterator<Item = &'a str>,
{
    let mut values = values.into_iter();
    let Some(first) = values.next() else {
        return mixed.to_owned();
    };
    if values.all(|value| value == first) {
        first.to_owned()
    } else {
        mixed.to_owned()
    }
}

fn payload_string(event: &SessionEvent, key: &str) -> Option<String> {
    event
        .payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn event_text(event: &SessionEvent) -> String {
    for key in ["content", "text", "message", "body"] {
        if let Some(value) = payload_string(event, key) {
            return value;
        }
    }
    serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".to_owned())
}

fn sha256(value: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(value.as_bytes())))
}

#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptRetrievalHit<'a> {
    pub chunk: &'a TranscriptChunk,
    pub score: f64,
}

/// Deterministic lexical retrieval over chunk projections. Scope is checked before ranking.
pub fn retrieve_transcript_chunks<'a>(
    chunks: &'a [TranscriptChunk],
    query: &str,
    scope_id: Option<&str>,
    limit: usize,
) -> Vec<TranscriptRetrievalHit<'a>> {
    if limit == 0 {
        return Vec::new();
    }
    let terms = query
        .to_ascii_lowercase()
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return Vec::new();
    }
    let mut hits = chunks
        .iter()
        .filter(|chunk| scope_id.is_none_or(|scope| chunk.scope_id == scope))
        .filter_map(|chunk| {
            let content = chunk.content.to_ascii_lowercase();
            let score = terms
                .iter()
                .map(|term| content.match_indices(term).count() as f64)
                .sum::<f64>();
            (score > 0.0).then_some(TranscriptRetrievalHit { chunk, score })
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.chunk.chunk_id.cmp(&right.chunk.chunk_id))
    });
    hits.truncate(limit);
    hits
}
