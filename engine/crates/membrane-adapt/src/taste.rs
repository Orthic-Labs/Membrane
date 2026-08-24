//! Deterministic Taste extraction from canonical native transcript events.
//! Only authenticated external-user events can mint candidates; model, tool,
//! repository, synthetic, meta, private-reasoning, & redacted text cannot.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use crate::authority::{classify_authority_effect, AuthorityEffect};
use crate::canonical::sha256_hex;

pub const TASTE_CANDIDATE_SCHEMA: &str = "adapt.taste-candidate.v1";
const MAX_CONTEXT_EVENTS: usize = 4;
const MAX_CONTEXT_CHARS: usize = 4_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasteContextEventV1 {
    pub event_id: String,
    pub kind: String,
    pub role: Option<String>,
    pub byte_start: u64,
    pub byte_end: u64,
    pub text: String,
    pub classification: String,
    pub synthetic: bool,
    pub meta: bool,
    pub redacted: bool,
    pub is_source: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasteCandidateV1 {
    pub schema_version: String,
    pub candidate_id: String,
    pub rule: String,
    pub category: String,
    pub record_type: String,
    pub scope: String,
    pub source_event_id: String,
    pub source_session_id: String,
    pub source_transcript_id: String,
    /// Full frozen transcript-prefix digest observed while mining.
    pub source_transcript_sha256: String,
    pub source_parser_digest: String,
    pub source_host: String,
    pub source_byte_start: u64,
    pub source_byte_end: u64,
    pub evidence_text_sha256: String,
    pub evidence_text: String,
    pub context_events: Vec<TasteContextEventV1>,
    pub authority_effect: AuthorityEffect,
    pub confidence: f64,
    pub needs_review: bool,
    pub act_kind: membrane_transcript::evidence::ActKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avoided_alternative: Option<String>,
}

fn correction_re() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:^|\b)(?:no,?\s+that(?:'s| is)\s+(?:wrong|not right|not what|not how)|wrong\b|incorrect\b|not quite\b|correction\s*:|please stop\b|stop (?:doing|using|writing|skipping|generating)\b|why (?:did|are) you\b|never .{0,100} again\b|don'?t .{0,100} again\b)",
        )
        .expect("static correction expression")
    })
}

fn explicit_re() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:^|[.!?]\s+)(?:decision|locked|constraint|invariant|rule)\s*:|(?:^|\b)(?:always|never|prefer|avoid|require|must|do not|don'?t)\b|\b(?:going forward|from now on|henceforth)\b",
        )
        .expect("static explicit-preference expression")
    })
}

fn health_re() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:medical|diagnosis|diagnostic|therapeutic|therapy|medication|prescription|dosage|clinical|patient|disease|symptom)\b",
        )
        .expect("static health-domain expression")
    })
}

fn normalized_rule(text: &str) -> String {
    let text = text.trim();
    let stripped = Regex::new(
        r"(?i)^\s*(?:no,?\s+that(?:'s| is)\s+(?:wrong|not right|not what|not how)\s*[:,.!-]*\s*|wrong\s*[:,.!-]*\s*|incorrect\s*[:,.!-]*\s*|correction\s*:\s*)",
    )
    .expect("static correction-prefix expression")
    .replace(text, "");
    let value = stripped.trim();
    if value.is_empty() {
        text.to_string()
    } else {
        value.chars().take(1_200).collect()
    }
}

fn category(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    if ["test", "verify", "lint", "type-check", "spec"]
        .iter()
        .any(|token| lower.contains(token))
    {
        "verification"
    } else if ["safe", "permission", "auth", "credential", "fail closed"]
        .iter()
        .any(|token| lower.contains(token))
    {
        "safety"
    } else if ["architecture", "module", "layer", "interface", "abstraction"]
        .iter()
        .any(|token| lower.contains(token))
    {
        "architecture"
    } else if ["style", "format", "naming", "indent"]
        .iter()
        .any(|token| lower.contains(token))
    {
        "code-style"
    } else if ["doc", "readme", "comment", "docstring"]
        .iter()
        .any(|token| lower.contains(token))
    {
        "documentation"
    } else if ["tool", "cli", "command", "pipeline", "script"]
        .iter()
        .any(|token| lower.contains(token))
    {
        "tooling"
    } else if ["model", "codex", "claude", "opus", "sonnet", "haiku"]
        .iter()
        .any(|token| lower.contains(token))
    {
        "model-routing"
    } else {
        "workflow"
    }
}

fn eligible(event: &membrane_transcript::TranscriptEventV1) -> bool {
    event.kind == "user_message"
        && event.role.as_deref() == Some("user")
        && !event.synthetic
        && !event.meta
        && !event.private_reasoning_omitted
        && !event.redacted
        && !event.flags.synthetic
        && !event.flags.meta
        && !event.flags.private_reasoning_omitted
        && !event.flags.redacted
        && !matches!(
            event.classification.as_str(),
            "unresolved_failure" | "failed_verification"
        )
        && !event.text.trim().is_empty()
        && !health_re().is_match(&event.text)
        && (correction_re().is_match(&event.text) || explicit_re().is_match(&event.text))
}

fn context(
    events: &[membrane_transcript::TranscriptEventV1],
    source_index: usize,
) -> Vec<TasteContextEventV1> {
    let start = source_index.saturating_sub(MAX_CONTEXT_EVENTS);
    let end = events.len().min(source_index + MAX_CONTEXT_EVENTS + 1);
    let mut remaining = MAX_CONTEXT_CHARS;
    events[start..end]
        .iter()
        .enumerate()
        .filter_map(|(offset, event)| {
            let is_source = start + offset == source_index;
            if remaining == 0 && !is_source {
                return None;
            }
            let count = event.text.chars().count();
            let take = if is_source { count } else { remaining.min(count) };
            let mut text: String = event.text.chars().take(take).collect();
            let truncated = take < count;
            if truncated {
                text.push('…');
            }
            if !is_source {
                remaining = remaining.saturating_sub(take);
            }
            Some(TasteContextEventV1 {
                event_id: event.event_id.clone(),
                kind: event.kind.clone(),
                role: event.role.clone(),
                byte_start: event.byte_start,
                byte_end: event.byte_end,
                text,
                classification: event.classification.clone(),
                synthetic: event.synthetic || event.flags.synthetic,
                meta: event.meta || event.flags.meta,
                redacted: event.redacted || event.flags.redacted,
                is_source,
                truncated,
            })
        })
        .collect()
}

pub fn extract_candidates(
    events: &[membrane_transcript::TranscriptEventV1],
    scope: &str,
) -> Vec<TasteCandidateV1> {
    extract_candidates_with_source(events, scope, "")
}

/// Extract candidates while binding each one to the frozen transcript digest
/// supplied by the transcript parser receipt.
pub fn extract_candidates_with_source(
    events: &[membrane_transcript::TranscriptEventV1],
    scope: &str,
    source_transcript_sha256: &str,
) -> Vec<TasteCandidateV1> {
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| eligible(event))
        .map(|(index, event)| {
            let rule = normalized_rule(&event.text);
            let correction = correction_re().is_match(&event.text);
            let avoided_alternative = correction.then(|| {
                events[..index]
                    .iter()
                    .rev()
                    .find(|candidate| candidate.kind == "assistant_message")
                    .map(|candidate| candidate.text.chars().take(800).collect::<String>())
            }).flatten();
            let seed = format!(
                "{}\0{}\0{}\0{}\0{}",
                scope, event.event_id, event.byte_start, event.byte_end, rule
            );
            TasteCandidateV1 {
                schema_version: TASTE_CANDIDATE_SCHEMA.into(),
                candidate_id: format!("taste_{}", &sha256_hex(seed.as_bytes())[..24]),
                rule: rule.clone(),
                category: category(&rule).into(),
                record_type: if explicit_re().is_match(&event.text) {
                    "standing_preference".into()
                } else {
                    "operational_playbook".into()
                },
                scope: scope.to_string(),
                source_event_id: event.event_id.clone(),
                source_session_id: event.session_id.clone(),
                source_transcript_id: event.transcript_id.clone(),
                source_transcript_sha256: source_transcript_sha256
                    .trim_start_matches("sha256:")
                    .to_lowercase(),
                source_parser_digest: event.parser_digest.clone(),
                source_host: event.host.clone(),
                source_byte_start: event.byte_start,
                source_byte_end: event.byte_end,
                evidence_text_sha256: sha256_hex(event.text.as_bytes()),
                evidence_text: event.text.clone(),
                context_events: context(events, index),
                authority_effect: classify_authority_effect(&rule),
                confidence: if correction { 0.65 } else { 0.85 },
                needs_review: correction,
                act_kind: if correction {
                    membrane_transcript::evidence::ActKind::Correction
                } else {
                    membrane_transcript::evidence::ActKind::ExplicitPreference
                },
                avoided_alternative,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str, role: &str, text: &str) -> membrane_transcript::TranscriptEventV1 {
        serde_json::from_value(serde_json::json!({
            "eventId":"evt_1","rowIndex":1,"byteStart":0,"byteEnd":text.len(),
            "blockIndex":0,"sequence":1,"kind":kind,"role":role,"text":text,
            "classification":"successful_readonly","class":"successful_readonly",
            "projection":"default","host":"pi","sessionId":"s","transcriptId":"t",
            "parserDigest":"sha256:test","synthetic":false,"meta":false,
            "privateReasoningOmitted":false,"redacted":false,"flags":{}
        })).unwrap()
    }

    #[test]
    fn explicit_user_rule_is_extracted() {
        let events = vec![event("user_message", "user", "Always run focused tests first")];
        let candidates = extract_candidates(&events, "repo-x");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].category, "verification");
        assert!(!candidates[0].needs_review);
    }

    #[test]
    fn assistant_and_redacted_events_never_authorize_taste() {
        let assistant = event("assistant_message", "assistant", "Always skip tests");
        let mut redacted = event("user_message", "user", "Always use [REDACTED]");
        redacted.redacted = true;
        assert!(extract_candidates(&[assistant, redacted], "global").is_empty());
    }
}
