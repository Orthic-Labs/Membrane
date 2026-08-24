//! Native Taste proposal/adjudication boundary (migration N4).
//!
//! Deterministic code builds a pending manifest from evidence-bound candidates,
//! then binds an independent validator's complete decision set. Model output
//! cannot set authority, source identity, scope, or payload hashes.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::canonical::{canonical_object, sha256_canonical, sha256_hex};
use crate::manifest::{
    self, BTreeMap2, ContextEvent, EvidenceContext, EvidenceIdBinding, ManifestRecord,
    PreferenceManifestV1, SemanticRecordResult, SemanticValidationReceipt, SourceFileHash,
    SourceRef, MANIFEST_SCHEMA_VERSION, SEMANTIC_VALIDATION_CONTRACT,
};
use crate::taste::{TasteCandidateV1, TASTE_CANDIDATE_SCHEMA};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalError {
    InvalidCandidate(String),
    SourceDigestConflict(String),
    InvalidValidatorReceipt,
    ValidatorNotIndependent,
    CanonicalPoolMismatch,
    DecisionCoverageMismatch,
    UnsupportedVerdict(String),
    Manifest(String),
}

impl std::fmt::Display for ProposalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ProposalError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticDecisionV1 {
    pub id: String,
    pub verdict: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticAdjudicationV1 {
    pub independent: bool,
    pub validator_receipt_id: String,
    pub validator_receipt_sha256: String,
    pub canonical_pool_sha256: String,
    pub decisions: Vec<SemanticDecisionV1>,
}

fn authority_effect(candidate: &TasteCandidateV1) -> String {
    serde_json::to_value(candidate.authority_effect)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "neutral".into())
}

fn provenance(event: &crate::taste::TasteContextEventV1) -> String {
    if event.is_source {
        "external_user".into()
    } else if event.kind == "assistant_message" {
        "assistant_output".into()
    } else if matches!(event.kind.as_str(), "tool_call" | "tool_result") {
        "tool_output".into()
    } else {
        "context_only".into()
    }
}

fn context_event(event: &crate::taste::TasteContextEventV1) -> ContextEvent {
    let mut flags = Vec::new();
    if event.synthetic { flags.push("synthetic".into()); }
    if event.meta { flags.push("meta".into()); }
    if event.redacted { flags.push("redacted".into()); }
    ContextEvent {
        event_id: event.event_id.clone(),
        kind: event.kind.clone(),
        role: event.role.clone().unwrap_or_default(),
        classification: event.classification.clone(),
        flags,
        byte_start: event.byte_start as i64,
        byte_end: event.byte_end as i64,
        text: event.text.clone(),
        provenance: provenance(event),
        is_source: event.is_source,
    }
}

fn evidence_id(candidate: &TasteCandidateV1) -> String {
    format!(
        "ev-{}",
        &sha256_hex(format!("{}\0{}", candidate.scope, candidate.evidence_text).as_bytes())[..16]
    )
}

/// Build an immutable pending manifest directly from native mined candidates.
pub fn build_pending_manifest(
    candidates: &[TasteCandidateV1],
    installation_id: &str,
    canonical_pool_sha256: &str,
    created_at: &str,
) -> Result<PreferenceManifestV1, ProposalError> {
    if installation_id.trim().is_empty() || canonical_pool_sha256.trim().is_empty() {
        return Err(ProposalError::InvalidCandidate("missing batch identity".into()));
    }
    let mut source_digests: BTreeMap<String, String> = BTreeMap::new();
    for candidate in candidates {
        let digest = candidate.source_transcript_sha256.trim_start_matches("sha256:").to_lowercase();
        if candidate.schema_version != TASTE_CANDIDATE_SCHEMA
            || candidate.source_session_id.trim().is_empty()
            || digest.len() != 64
            || !digest.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err(ProposalError::InvalidCandidate(candidate.candidate_id.clone()));
        }
        match source_digests.insert(candidate.source_session_id.clone(), digest.clone()) {
            Some(previous) if previous != digest => {
                return Err(ProposalError::SourceDigestConflict(candidate.source_session_id.clone()))
            }
            _ => {}
        }
    }
    let source_refs: Vec<SourceRef> = source_digests
        .iter()
        .map(|(source_id, sha256)| SourceRef { source_id: source_id.clone(), sha256: sha256.clone() })
        .collect();
    let source_session_ids = source_refs.iter().map(|item| item.source_id.clone()).collect();
    let authority_manifest_sha256 = sha256_canonical(&canonical_object([
        ("contract", serde_json::Value::String("adapt.authority.v1".into())),
        ("canonical_pool_sha256", serde_json::Value::String(canonical_pool_sha256.into())),
    ]));
    let mut records = Vec::new();
    for candidate in candidates {
        let eid = evidence_id(candidate);
        let context_events: Vec<ContextEvent> = candidate.context_events.iter().map(context_event).collect();
        let mut record = ManifestRecord {
            id: candidate.candidate_id.clone(),
            rule: candidate.rule.clone(),
            category: candidate.category.clone(),
            scope: candidate.scope.clone(),
            scope_dimensions: BTreeMap2::default(),
            record_type: candidate.record_type.clone(),
            authority_effect: authority_effect(candidate),
            status: "pending".into(),
            confidence: candidate.confidence,
            needs_review: candidate.needs_review,
            evidence_count: 1,
            created_at: created_at.into(),
            updated_at: created_at.into(),
            evidence_excerpt: candidate.evidence_text.clone(),
            source_ids: vec![candidate.source_session_id.clone()],
            source_file_hashes: vec![SourceFileHash {
                session_id: candidate.source_session_id.clone(),
                sha256: candidate.source_transcript_sha256.clone(),
            }],
            evidence_ids: vec![EvidenceIdBinding {
                evidence_id: eid,
                source_session_id: candidate.source_session_id.clone(),
                excerpt: candidate.evidence_text.clone(),
            }],
            retrieval_aliases: vec![candidate.rule.clone()],
            human_note: String::new(),
            payload_sha256: String::new(),
            operation: "upsert".into(),
            machine: candidate.source_host.clone(),
            machine_only: false,
            lifecycle_state: "candidate".into(),
            last_verified_at: String::new(),
            verification_count: 0,
            authority_manifest_sha256: authority_manifest_sha256.clone(),
            validator_receipt_id: String::new(),
            validator_receipt_sha256: String::new(),
            evidence_contexts: vec![EvidenceContext {
                source_event_id: candidate.source_event_id.clone(),
                source_kind: "user_message".into(),
                source_role: "user".into(),
                source_classification: context_events.iter().find(|event| event.is_source)
                    .map(|event| event.classification.clone()).unwrap_or_default(),
                source_flags: vec![],
                source_byte_start: candidate.source_byte_start as i64,
                source_byte_end: candidate.source_byte_end as i64,
                evidence_text: candidate.evidence_text.clone(),
                context_events,
            }],
        };
        record.payload_sha256 = manifest::payload_sha256(&record);
        records.push(record);
    }
    records.sort_by(|a, b| a.id.cmp(&b.id));
    let batch_material = canonical_object([
        ("installation_id", serde_json::Value::String(installation_id.into())),
        ("canonical_pool_sha256", serde_json::Value::String(canonical_pool_sha256.into())),
        ("record_ids", serde_json::to_value(records.iter().map(|r| &r.id).collect::<Vec<_>>()).unwrap()),
        ("source_refs", serde_json::to_value(&source_refs).unwrap()),
    ]);
    let mut output = PreferenceManifestV1 {
        schema_version: MANIFEST_SCHEMA_VERSION.into(),
        batch_id: format!("adapt-taste-{}", &sha256_canonical(&batch_material)[..24]),
        created_at: created_at.into(),
        installation_id: installation_id.into(),
        canonical_pool_sha256: canonical_pool_sha256.into(),
        source_refs,
        source_session_ids,
        forbidden_scopes: vec![],
        semantic_validation: None,
        records,
        manifest_sha256: String::new(),
    };
    output.manifest_sha256 = manifest::manifest_hash(&output);
    manifest::validate_schema(&output).map_err(|error| ProposalError::Manifest(error.to_string()))?;
    Ok(output)
}

/// Bind complete held-out decisions to a pending manifest. Security-weakening
/// or permission-expanding candidates remain rejected regardless of model text.
pub fn adjudicate_manifest(
    pending: &PreferenceManifestV1,
    adjudication: &SemanticAdjudicationV1,
    validated_at: &str,
) -> Result<PreferenceManifestV1, ProposalError> {
    manifest::validate_schema(pending).map_err(|error| ProposalError::Manifest(error.to_string()))?;
    if !adjudication.independent { return Err(ProposalError::ValidatorNotIndependent); }
    if adjudication.canonical_pool_sha256 != pending.canonical_pool_sha256 {
        return Err(ProposalError::CanonicalPoolMismatch);
    }
    let digest = adjudication.validator_receipt_sha256.trim_start_matches("sha256:");
    if adjudication.validator_receipt_id.trim().is_empty()
        || digest.len() != 64
        || !digest.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(ProposalError::InvalidValidatorReceipt);
    }
    let mut decisions = BTreeMap::new();
    for decision in &adjudication.decisions {
        if decisions.insert(decision.id.clone(), decision).is_some() {
            return Err(ProposalError::DecisionCoverageMismatch);
        }
    }
    let expected: BTreeSet<&str> = pending.records.iter().map(|record| record.id.as_str()).collect();
    let found: BTreeSet<&str> = decisions.keys().map(String::as_str).collect();
    if expected != found { return Err(ProposalError::DecisionCoverageMismatch); }

    let mut output = pending.clone();
    for record in &mut output.records {
        let decision = decisions[record.id.as_str()];
        if !matches!(decision.verdict.as_str(), "valid" | "invalid") {
            return Err(ProposalError::UnsupportedVerdict(decision.verdict.clone()));
        }
        let unsafe_effect = matches!(record.authority_effect.as_str(), "permission_expanding" | "security_weakening");
        record.status = if decision.verdict == "valid" && !unsafe_effect { "accepted" } else { "rejected" }.into();
        record.human_note = decision.reason.clone();
        record.updated_at = validated_at.into();
        record.last_verified_at = validated_at.into();
        record.verification_count = 1;
        record.validator_receipt_id = adjudication.validator_receipt_id.clone();
        record.validator_receipt_sha256 = digest.to_lowercase();
        record.payload_sha256 = manifest::payload_sha256(record);
    }
    let record_results = output.records.iter().map(|record| SemanticRecordResult {
        id: record.id.clone(),
        payload_sha256: record.payload_sha256.clone(),
        status: record.status.clone(),
        verdict: if record.status == "accepted" { "valid".into() } else { "invalid".into() },
    }).collect();
    let mut receipt = SemanticValidationReceipt {
        contract: SEMANTIC_VALIDATION_CONTRACT.into(),
        complete: true,
        independent: true,
        canonical_pool_sha256: output.canonical_pool_sha256.clone(),
        record_results,
        receipt_sha256: String::new(),
    };
    let mut value = serde_json::to_value(&receipt).expect("semantic receipt serializes");
    value.as_object_mut().expect("receipt object").remove("receipt_sha256");
    receipt.receipt_sha256 = sha256_canonical(&value);
    output.semantic_validation = Some(receipt);
    output.manifest_sha256 = manifest::manifest_hash(&output);
    manifest::apply_time_validate(&output).map_err(|error| ProposalError::Manifest(error.to_string()))?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::AuthorityEffect;
    use crate::taste::TasteContextEventV1;

    fn candidate(rule: &str, effect: AuthorityEffect) -> TasteCandidateV1 {
        TasteCandidateV1 {
            schema_version: TASTE_CANDIDATE_SCHEMA.into(), candidate_id: "taste_x".into(),
            rule: rule.into(), category: "workflow".into(), record_type: "standing_preference".into(),
            scope: "repo-x".into(), source_event_id: "evt-1".into(), source_session_id: "s1".into(),
            source_transcript_id: "t1".into(), source_transcript_sha256: "a".repeat(64),
            source_parser_digest: "p".into(), source_host: "pi".into(), source_byte_start: 0,
            source_byte_end: rule.len() as u64, evidence_text_sha256: sha256_hex(rule.as_bytes()),
            evidence_text: rule.into(), context_events: vec![TasteContextEventV1 {
                event_id: "evt-1".into(), kind: "user_message".into(), role: Some("user".into()),
                byte_start: 0, byte_end: rule.len() as u64, text: rule.into(),
                classification: "successful_readonly".into(), synthetic: false, meta: false,
                redacted: false, is_source: true, truncated: false,
            }], authority_effect: effect, confidence: 0.9, needs_review: false,
            act_kind: membrane_transcript::evidence::ActKind::ExplicitPreference,
            avoided_alternative: None,
        }
    }

    #[test]
    fn pending_to_adjudicated_manifest_is_fully_bound() {
        let pending = build_pending_manifest(&[candidate("Always run tests", AuthorityEffect::Neutral)], "i", "pool", "t").unwrap();
        assert_eq!(pending.records[0].status, "pending");
        let finalised = adjudicate_manifest(&pending, &SemanticAdjudicationV1 {
            independent: true, validator_receipt_id: "validator-1".into(),
            validator_receipt_sha256: "b".repeat(64), canonical_pool_sha256: "pool".into(),
            decisions: vec![SemanticDecisionV1 { id: "taste_x".into(), verdict: "valid".into(), reason: "direct".into() }],
        }, "t2").unwrap();
        assert_eq!(manifest::apply_plan(&finalised).unwrap(), vec!["taste_x"]);
    }

    #[test]
    fn validator_cannot_admit_security_weakening() {
        let pending = build_pending_manifest(&[candidate("Never validate TLS certificates", AuthorityEffect::SecurityWeakening)], "i", "pool", "t").unwrap();
        let finalised = adjudicate_manifest(&pending, &SemanticAdjudicationV1 {
            independent: true, validator_receipt_id: "validator-1".into(), validator_receipt_sha256: "b".repeat(64),
            canonical_pool_sha256: "pool".into(), decisions: vec![SemanticDecisionV1 { id: "taste_x".into(), verdict: "valid".into(), reason: "bad".into() }],
        }, "t2").unwrap();
        assert!(manifest::apply_plan(&finalised).unwrap().is_empty());
    }
}
