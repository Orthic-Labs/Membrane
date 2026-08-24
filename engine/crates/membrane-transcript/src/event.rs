//! `TranscriptEventV1` and receipt shapes.

use serde::{Deserialize, Serialize};

pub use crate::redact::{
    MAX_ASSISTANT_CHARS, MAX_EVENT_CHARS, MAX_TOOL_CALL_CHARS, MAX_TOOL_RESULT_CHARS,
};

/// Per-event boolean evidence flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EventFlags {
    #[serde(rename = "synthetic", default)]
    pub synthetic: bool,
    #[serde(rename = "meta", default)]
    pub meta: bool,
    #[serde(rename = "privateReasoningOmitted", default)]
    pub private_reasoning_omitted: bool,
    #[serde(rename = "redacted", default)]
    pub redacted: bool,
    #[serde(rename = "isError", default)]
    pub is_error: bool,
    #[serde(rename = "isSidechain", default)]
    pub is_sidechain: bool,
}

/// One normalized transcript event (`TranscriptEventV1`, internal domain
/// contract — not one of Membrane's five public protocol shapes).
///
/// Every byte of the source produces either an event or a recorded omission;
/// field names mirror the retired Python normalizer's dict keys exactly
/// (including the snake_case `call_id`) so downstream projections ingest both
/// identically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptEventV1 {
    /// Deterministic id: `evt_ + sha256(canonical seed)[:32]`.
    #[serde(rename = "eventId")]
    pub event_id: String,
    /// 1-indexed JSONL row number.
    #[serde(rename = "rowIndex")]
    pub row_index: u64,
    /// Byte span of the source row, trailing newline included; slicing the
    /// original bytes `[byte_start..byte_end]` reproduces the row exactly.
    #[serde(rename = "byteStart")]
    pub byte_start: u64,
    #[serde(rename = "byteEnd")]
    pub byte_end: u64,
    /// Index of this event within its row (multi-block rows emit several).
    #[serde(rename = "blockIndex")]
    pub block_index: usize,
    /// Global 1-based emission order over the whole transcript.
    #[serde(rename = "sequence")]
    pub sequence: u64,
    /// One of `user_message`, `assistant_message`, `tool_call`, `tool_result`,
    /// `thinking`, `meta`.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    /// Per-`call_id` occurrence counter (0-based); later results never trample
    /// earlier ones.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<u64>,
    /// Linkage: for a `tool_result`, the eventId of the matching `tool_call`
    /// (same call_id, same occurrence), when that call was already seen.
    #[serde(rename = "toolCallEventId", skip_serializing_if = "Option::is_none")]
    pub tool_call_event_id: Option<String>,
    /// Redacted + compacted text (NUL-stripped, capped at `MAX_EVENT_CHARS`).
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Class-priority admission class.
    pub classification: String,
    /// Alias of `classification` (compat with the Python surface).
    #[serde(rename = "class")]
    pub class_alias: String,
    /// Projection label (`default` unless caller overrides).
    pub projection: String,
    // ---- Provenance ----
    pub host: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "transcriptId")]
    pub transcript_id: String,
    #[serde(rename = "parserDigest")]
    pub parser_digest: String,
    // ---- Optional thread/scope passthrough (generic adapter) ----
    #[serde(rename = "agentRole", skip_serializing_if = "Option::is_none")]
    pub agent_role: Option<String>,
    #[serde(rename = "threadSource", skip_serializing_if = "Option::is_none")]
    pub thread_source: Option<String>,
    #[serde(rename = "parentThreadId", skip_serializing_if = "Option::is_none")]
    pub parent_thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    // ---- Evidence flags: top-level booleans plus the nested set ----
    #[serde(default)]
    pub synthetic: bool,
    #[serde(default)]
    pub meta: bool,
    #[serde(rename = "privateReasoningOmitted", default)]
    pub private_reasoning_omitted: bool,
    #[serde(default)]
    pub redacted: bool,
    pub flags: EventFlags,
}

impl TranscriptEventV1 {
    /// The admission class parsed back from `classification`.
    pub fn classification(&self) -> crate::classify::Classification {
        crate::classify::Classification::ALL
            .iter()
            .copied()
            .find(|c| c.as_str() == self.classification)
            .unwrap_or(crate::classify::Classification::SuccessfulReadonly)
    }
}

/// Frozen prefix receipt binding what a consumer saw: host/session/transcript
/// identity, prefix length + digest, and the parser implementation digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixReceipt {
    pub host: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "transcriptId")]
    pub transcript_id: String,
    #[serde(rename = "prefixLength")]
    pub prefix_length: u64,
    #[serde(rename = "prefixDigest")]
    pub prefix_digest: String,
    #[serde(rename = "parserDigest")]
    pub parser_digest: String,
    #[serde(rename = "parserVersion")]
    pub parser_version: String,
}

/// Receipt as returned by [`crate::parse_prefix_receipt`] — adds the number of
/// events observed while parsing the bound prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixReceiptObserved {
    #[serde(flatten)]
    pub receipt: PrefixReceipt,
    #[serde(rename = "eventsObserved")]
    pub events_observed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_default_to_false() {
        let f = EventFlags::default();
        assert!(!f.synthetic && !f.meta && !f.private_reasoning_omitted);
        assert!(!f.redacted && !f.is_error && !f.is_sidechain);
    }

    #[test]
    fn classification_roundtrip() {
        let ev_json = r#"{"eventId":"evt_x","rowIndex":1,"byteStart":0,"byteEnd":5,"blockIndex":0,"sequence":1,"kind":"tool_call","classification":"mutation","class":"mutation","projection":"default","host":"pi","sessionId":"s","transcriptId":"t","parserDigest":"sha256:x","text":"y","synthetic":false,"meta":false,"privateReasoningOmitted":false,"redacted":false,"flags":{}}"#;
        let ev: TranscriptEventV1 = serde_json::from_str(ev_json).unwrap();
        assert_eq!(
            ev.classification(),
            crate::classify::Classification::Mutation
        );
    }
}
