//! Deterministic Taste extraction from canonical native transcript events.
//! Only authenticated external-user events can mint candidates; model, tool,
//! repository, synthetic, meta, private-reasoning, & redacted text cannot.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::authority::{classify_authority_effect, AuthorityEffect};
use crate::canonical::{sha256_canonical, sha256_hex};
use crate::scope::ScopeDimensions;

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

#[derive(Debug, Clone, Serialize)]
pub struct TasteCandidateV1 {
    pub schema_version: String,
    pub candidate_id: String,
    pub rule: String,
    pub category: String,
    pub record_type: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scope_dimensions: BTreeMap<String, String>,
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
    pub evidence_class: membrane_transcript::evidence::EvidenceClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_user_act_receipt_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avoided_alternative: Option<String>,
    /// In-memory capability binding every serialized field to evidence that
    /// passed the native transcript parser or host-signature verifier. This is
    /// deliberately neither public nor deserializable: JSON can describe a
    /// candidate, but it cannot manufacture authority.
    #[serde(skip)]
    integrity_sha256: String,
}

impl TasteCandidateV1 {
    fn seal_integrity(mut self) -> Self {
        self.integrity_sha256 =
            sha256_hex(&serde_json::to_vec(&self).expect("Taste candidate serializes"));
        self
    }

    pub(crate) fn verify_integrity(&self) -> bool {
        !self.integrity_sha256.is_empty()
            && self.integrity_sha256
                == sha256_hex(&serde_json::to_vec(self).expect("Taste candidate serializes"))
    }

    #[cfg(test)]
    pub(crate) fn reseal_for_test(&mut self) {
        self.integrity_sha256 = sha256_hex(&serde_json::to_vec(self).unwrap());
    }
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

/// Explicitly bounded instructions belong to the current operation, not to
/// durable Taste.  Keep this deliberately narrow: an unqualified preference
/// remains eligible, while clear task/turn/response markers fail closed out
/// of the durable extraction lane.
fn task_local_re() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(
            r"(?i)^\s*(?:for (?:this|the current) (?:task|request|response|turn)\b|only (?:for|in) (?:this|the current) (?:task|request|response|turn)\b|today\s*[,;:])",
        )
        .expect("static task-local expression")
    })
}

fn normalized_rule(text: &str) -> String {
    let text = text.trim();
    let stripped = Regex::new(
        r"(?i)^\s*(?:no,?\s+that(?:'s| is)\s+(?:wrong|not right|not what|not how)\s*[:,.!-]*\s*|wrong\s*[:,.!-]*\s*|incorrect\s*[:,.!-]*\s*|correction\s*:\s*|(?:from now on|going forward|henceforth)\s*[:,.!-]*\s*|(?:decision|locked|constraint|invariant|rule)\s*:\s*|please\s+)",
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

#[cfg(test)]
pub(crate) fn test_candidate(rule: &str, expected: AuthorityEffect) -> TasteCandidateV1 {
    let event: membrane_transcript::TranscriptEventV1 = serde_json::from_value(serde_json::json!({
        "eventId":"evt-1","rowIndex":1,"byteStart":0,"byteEnd":rule.len(),
        "blockIndex":0,"sequence":1,"kind":"user_message","role":"user","text":rule,
        "classification":"successful_readonly","class":"successful_readonly",
        "projection":"default","host":"pi","sessionId":"s1","transcriptId":"t1",
        "parserDigest":"p","synthetic":false,"meta":false,
        "privateReasoningOmitted":false,"redacted":false,"flags":{}
    })).unwrap();
    let mut candidate = extract_candidates_with_source(&[event], "repo-x", &"a".repeat(64))
        .into_iter().next().unwrap();
    assert_eq!(candidate.authority_effect, expected);
    candidate.candidate_id = "taste_x".into();
    candidate.reseal_for_test();
    candidate
}

fn category(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    // Prefer explicit tool names over the activity performed with them. For
    // example, a requested Cargo/lint command is a tooling preference, while
    // an unqualified request to run lint is a verification preference.
    if ["tool", "cli", "command", "pipeline", "script", "cargo"]
        .iter()
        .any(|token| lower.contains(token))
    {
        "tooling"
    } else if ["test", "verify", "lint", "type-check", "spec"]
        .iter()
        .any(|token| lower.contains(token))
    {
        "verification"
    } else if ["safe", "permission", "credential", "fail closed"]
        .iter()
        .any(|token| lower.contains(token))
        || Regex::new(r"\bauth(?:entication|orization)?\b")
            .expect("static auth category expression")
            .is_match(&lower)
    {
        "safety"
    } else if lower.contains("docstring") {
        "documentation"
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
        && !task_local_re().is_match(&event.text)
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
            let take = if is_source {
                count
            } else {
                remaining.min(count)
            };
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
                scope_dimensions: BTreeMap::new(),
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
                evidence_class: membrane_transcript::evidence::EvidenceClass::UserAuthoritative,
                verified_user_act_receipt_sha256: None,
                avoided_alternative,
                integrity_sha256: String::new(),
            }
            .seal_integrity()
        })
        .collect()
}

/// Mine candidates from trusted structured host acts. Silent accepts and bare
/// rejects remain support-only and cannot mint Taste authority.
pub fn extract_candidates_from_verified_acts(
    evidence: &[membrane_transcript::evidence::VerifiedUserActEvidence],
    scope: &str,
) -> Vec<TasteCandidateV1> {
    let mut candidates: Vec<_> = evidence.iter().filter_map(|verified| {
        let item = verified.evidence();
        let excerpt = match item.act_kind {
            membrane_transcript::evidence::ActKind::ExplicitPreference
            | membrane_transcript::evidence::ActKind::Correction
            | membrane_transcript::evidence::ActKind::PostAcceptEdit
            | membrane_transcript::evidence::ActKind::RepeatedEdit
            | membrane_transcript::evidence::ActKind::NamedChoice => item.after_excerpt.as_deref(),
            membrane_transcript::evidence::ActKind::Reject
            | membrane_transcript::evidence::ActKind::Accept => None,
        }?;
        let rule = normalized_rule(excerpt);
        if rule.trim().is_empty() || health_re().is_match(&rule) { return None; }
        let source_event_id = item.event_ids.first()?.clone();
        let avoided_alternative = matches!(item.act_kind,
            membrane_transcript::evidence::ActKind::Correction
            | membrane_transcript::evidence::ActKind::PostAcceptEdit
            | membrane_transcript::evidence::ActKind::RepeatedEdit)
            .then(|| item.before_excerpt.clone()).flatten();
        let evidence_class = verified.classify();
        let (signed_scope, scope_dimensions) = signed_scope(&item.scope_context)?;
        // A CLI scope is not signed evidence. It may select the exact signed
        // scope, but a broader or unrelated value (including `global`) cannot
        // replace it. The normalized signed dimensions remain authoritative.
        if scope != "global" && scope != "workspace" && scope != signed_scope {
            return None;
        }
        let seed = format!("{}\0{}\0{}\0{}", signed_scope, item.evidence_id, verified.receipt_sha256(), rule);
        Some(TasteCandidateV1 {
            schema_version: TASTE_CANDIDATE_SCHEMA.into(),
            candidate_id: format!("taste_{}", &sha256_hex(seed.as_bytes())[..24]),
            rule: rule.clone(), category: category(&rule).into(), record_type: "standing_preference".into(),
            scope: signed_scope, scope_dimensions, source_event_id, source_session_id: item.session_id.clone(),
            source_transcript_id: format!("host-act:{}", item.evidence_id),
            source_transcript_sha256: verified.receipt_sha256().into(),
            source_parser_digest: membrane_transcript::user_act::USER_ACT_ADAPTER_VERSION.into(),
            source_host: item.host.clone(), source_byte_start: 0, source_byte_end: 0,
            evidence_text_sha256: sha256_hex(excerpt.as_bytes()), evidence_text: excerpt.into(),
            context_events: vec![], authority_effect: classify_authority_effect(&rule),
            confidence: item.signal_strength,
            needs_review: evidence_class != membrane_transcript::evidence::EvidenceClass::UserAuthoritative,
            act_kind: item.act_kind, evidence_class,
            verified_user_act_receipt_sha256: Some(verified.receipt_sha256().into()), avoided_alternative,
            integrity_sha256: String::new(),
        }.seal_integrity())
    }).collect();
    candidates.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    candidates.dedup_by(|left, right| left.candidate_id == right.candidate_id);
    candidates
}

fn signed_scope(raw: &BTreeMap<String, String>) -> Option<(String, BTreeMap<String, String>)> {
    let normalized = ScopeDimensions::normalize(raw).ok()?;
    if normalized.is_empty() {
        return None;
    }
    let dimensions: BTreeMap<String, String> = normalized
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let digest = sha256_canonical(&serde_json::to_value(&dimensions).ok()?);
    Some((format!("dimensions:{}", &digest[..24]), dimensions))
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_transcript::evidence::{
        ActKind, UserActEvidenceV1, UserActProvenanceReceiptV1, USER_ACT_RECEIPT_CONTRACT,
    };
    use membrane_transcript::user_act::{
        HostActTrustStoreV1, HostActVerifier, MemoryReplayStore, TrustedHostIssuerV1,
        USER_ACT_ROW_TYPE, USER_ACT_TRUST_CONTRACT,
    };
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn signed_post_edit(
        scope_context: BTreeMap<String, String>,
    ) -> membrane_transcript::evidence::VerifiedUserActEvidence {
        let key = Ed25519KeyPair::from_seed_unchecked(&[17; 32]).unwrap();
        let receipt = UserActProvenanceReceiptV1 {
            contract_version: USER_ACT_RECEIPT_CONTRACT.into(),
            issuer_id: "coderight".into(), key_id: "key-1".into(),
            installation_id: "inst".into(), host: "coderight".into(),
            session_id: "session".into(), sequence: 1, nonce: "n-1".into(),
            payload_sha256: "0".repeat(64), signature_hex: "0".repeat(128),
        };
        let mut evidence = UserActEvidenceV1::new(
            "act-1", "inst", "coderight", "session", vec!["event-1".into()],
            ActKind::PostAcceptEdit, None, scope_context,
            "2026-08-26T00:00:00Z", receipt,
        ).unwrap();
        evidence.set_counterfactual(Some("use a broad rewrite"), Some("use a focused patch")).unwrap();
        let payload = serde_json::to_vec(&evidence.receipt_payload()).unwrap();
        evidence.provenance_receipt.payload_sha256 = sha256_hex(&payload);
        let mut signed = b"membrane.adapt-user-act.v2\0".to_vec();
        signed.extend_from_slice(&payload);
        evidence.provenance_receipt.signature_hex = hex::encode(key.sign(&signed).as_ref());
        let mut row = serde_json::to_value(evidence).unwrap();
        row.as_object_mut().unwrap().insert("type".into(), serde_json::Value::String(USER_ACT_ROW_TYPE.into()));
        let trust = HostActTrustStoreV1 {
            contract_version: USER_ACT_TRUST_CONTRACT.into(), installation_id: "inst".into(),
            issuers: vec![TrustedHostIssuerV1 { issuer_id: "coderight".into(), key_id: "key-1".into(),
                host: "coderight".into(), public_key_hex: hex::encode(key.public_key().as_ref()), revoked: false }],
        };
        HostActVerifier::new(trust, MemoryReplayStore::default()).unwrap()
            .verify_row(&row).unwrap().unwrap()
    }

    fn event(kind: &str, role: &str, text: &str) -> membrane_transcript::TranscriptEventV1 {
        serde_json::from_value(serde_json::json!({
            "eventId":"evt_1","rowIndex":1,"byteStart":0,"byteEnd":text.len(),
            "blockIndex":0,"sequence":1,"kind":kind,"role":role,"text":text,
            "classification":"successful_readonly","class":"successful_readonly",
            "projection":"default","host":"pi","sessionId":"s","transcriptId":"t",
            "parserDigest":"sha256:test","synthetic":false,"meta":false,
            "privateReasoningOmitted":false,"redacted":false,"flags":{}
        }))
        .unwrap()
    }

    #[test]
    fn explicit_user_rule_is_extracted() {
        let events = vec![event(
            "user_message",
            "user",
            "Always run focused tests first",
        )];
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

    #[test]
    fn signed_behavioral_edit_stays_inferred_scoped_and_uses_only_replacement() {
        let verified = signed_post_edit(BTreeMap::from([("repo".into(), "membrane".into())]));
        let candidates = extract_candidates_from_verified_acts(&[verified], "global");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].rule, "use a focused patch");
        assert!(candidates[0].scope.starts_with("dimensions:"));
        assert_ne!(candidates[0].scope, "global");
        assert_eq!(
            candidates[0]
                .scope_dimensions
                .get("repo")
                .map(String::as_str),
            Some("membrane")
        );
        assert_eq!(
            candidates[0].evidence_class,
            membrane_transcript::evidence::EvidenceClass::UserBehavioral
        );
        let gate1 =
            crate::proposal::Gate1ReviewContextV1::from_verified_canonical_inventory(vec![]);
        let pending = crate::proposal::build_pending_manifest(
            &candidates,
            "inst",
            gate1.canonical_pool_sha256(),
            "t",
            &gate1,
        )
        .unwrap();
        let sealed =
            crate::manifest::semantic_payload_for_record(
                &pending.records[0],
                gate1.canonical_pool_sha256(),
            )
            .unwrap();
        assert_eq!(
            sealed.authority_tier,
            crate::authority::PrecedenceTier::InferredScopedUserPreference
        );
        assert_eq!(
            sealed.influence_class,
            crate::record::InfluenceClass::Provisional
        );
    }

    #[test]
    fn signed_scope_is_canonical_nonempty_and_never_global() {
        assert!(signed_scope(&BTreeMap::new()).is_none());
        assert!(signed_scope(&BTreeMap::from([("path".into(), "engine".into())])).is_none());
        assert!(signed_scope(&BTreeMap::from([("workspace".into(), "x".into())])).is_none());
        assert!(signed_scope(&BTreeMap::from([("scope".into(), "global".into())])).is_none());

        let raw = BTreeMap::from([
            ("path_prefix".into(), "engine/src".into()),
            ("language".into(), "rust".into()),
        ]);
        let (scope, dimensions) = signed_scope(&raw).unwrap();
        assert!(scope.starts_with("dimensions:"));
        assert_ne!(scope, "global");
        assert_eq!(dimensions, raw);
        assert_eq!(signed_scope(&raw).unwrap().0, scope);

        let empty = signed_post_edit(BTreeMap::new());
        assert!(extract_candidates_from_verified_acts(&[empty], "global").is_empty());

        let scoped = signed_post_edit(raw.clone());
        let candidates = extract_candidates_from_verified_acts(&[scoped], "global");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].scope_dimensions, raw);
        assert_eq!(candidates[0].scope, scope);
    }
}
