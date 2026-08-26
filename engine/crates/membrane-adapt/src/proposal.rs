//! Native Taste proposal/adjudication boundary (migration N4).
//!
//! Deterministic code builds a pending manifest from evidence-bound candidates,
//! then binds an independent validator's complete decision set. Model output
//! cannot set authority, source identity, scope, or payload hashes.

use ring::signature;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::canonical::{canonical_object, sha256_canonical, sha256_hex};
use crate::duplicate_groups::{self, DuplicateCandidateV1};
use crate::manifest::{
    self, BTreeMap2, ContextEvent, EvidenceContext, EvidenceIdBinding, ManifestRecord,
    PreferenceManifestV1, SemanticRecordResult, SemanticValidationReceipt, SourceFileHash,
    SourceRef, MANIFEST_SCHEMA_VERSION, SEMANTIC_VALIDATION_CONTRACT,
};
use crate::taste::{TasteCandidateV1, TASTE_CANDIDATE_SCHEMA};

pub const GATE1_REVIEW_CONTEXT_CONTRACT: &str = "adapt.gate1-review-context.v1";
pub use crate::admission::GATE1_POLICY_CONTRACT;

/// One fully verified canonical Taste record projected from Cortex for Gate 1.
///
/// `payload_sha256` binds the complete immutable manifest record, while the
/// explicit semantic payload/digest and current Cortex envelope fields keep
/// the pool definition reviewable and prevent applicability or authority
/// changes from hiding behind an unchanged id/rule/scope tuple.
#[derive(Debug, Clone, Serialize)]
pub struct CanonicalTasteRecordV1 {
    pub stored_rule: crate::authority::StoredRule,
    pub payload_sha256: String,
    pub semantic_digest: String,
    pub semantic_payload: crate::seal::SemanticPayloadV1,
    pub authority_manifest_sha256: String,
    pub validator_receipt_id: String,
    pub validator_receipt_sha256: String,
    pub current_authority: String,
    pub current_influence_class: String,
}

#[derive(Debug, Clone, Serialize)]
struct Gate1PolicyBanDefinitionV1 {
    reason: String,
    pattern: String,
}

/// Complete, trusted inputs used by deterministic proposal eligibility.
///
/// This value must be assembled from the canonical Cortex inventory, not from
/// the review request. Keeping it as an explicit argument prevents production
/// review from silently evaluating against an empty rule universe.
pub struct Gate1ReviewContextV1 {
    contract_version: &'static str,
    index: crate::admission::RuleIndex,
    stored_rules: Vec<crate::authority::StoredRule>,
    policy_bans: Vec<(String, regex::Regex)>,
    canonical_pool_sha256: String,
}

impl Gate1ReviewContextV1 {
    /// Construct a complete context after the caller has verified and loaded
    /// every live canonical Taste record from its trusted store.
    pub fn from_verified_canonical_inventory(records: Vec<CanonicalTasteRecordV1>) -> Self {
        Self::from_verified_canonical_inventory_with_policy(
            records,
            crate::admission::executable_gate1_policy_sha256(),
            crate::admission::NATIVE_GATE1_POLICY_BANS,
        )
    }

    fn from_verified_canonical_inventory_with_policy(
        mut records: Vec<CanonicalTasteRecordV1>,
        executable_policy_sha256: String,
        policy_definitions: &[(&str, &str)],
    ) -> Self {
        records.sort_by(|left, right| {
            left.stored_rule
                .id
                .cmp(&right.stored_rule.id)
                .then(left.payload_sha256.cmp(&right.payload_sha256))
                .then(left.semantic_digest.cmp(&right.semantic_digest))
        });
        let policy_bans = crate::admission::compile_gate1_policy_bans(policy_definitions);
        let policy_definitions = policy_definitions
            .iter()
            .map(|(reason, pattern)| Gate1PolicyBanDefinitionV1 {
                reason: (*reason).into(),
                pattern: (*pattern).into(),
            })
            .collect::<Vec<_>>();
        let canonical_pool_sha256 = sha256_canonical(&canonical_object([
            (
                "contract",
                serde_json::Value::String(GATE1_REVIEW_CONTEXT_CONTRACT.into()),
            ),
            (
                "policy",
                canonical_object([
                    (
                        "contract",
                        serde_json::Value::String(GATE1_POLICY_CONTRACT.into()),
                    ),
                    (
                        "bans",
                        serde_json::to_value(&policy_definitions)
                            .expect("Gate 1 policy definitions serialize"),
                    ),
                    (
                        "executable_policy_sha256",
                        serde_json::Value::String(executable_policy_sha256),
                    ),
                ]),
            ),
            (
                "canonical_records",
                serde_json::to_value(&records).expect("canonical Taste records serialize"),
            ),
        ]));
        let stored_rules = records
            .into_iter()
            .map(|record| record.stored_rule)
            .collect::<Vec<_>>();
        let mut index = crate::admission::RuleIndex::default();
        for stored in &stored_rules {
            if !matches!(
                stored.lifecycle_state.as_str(),
                "retired" | "deprecated" | "superseded"
            ) {
                index.insert(crate::record::RuleKey::new(&stored.scope, &stored.rule));
            }
        }
        Self {
            contract_version: GATE1_REVIEW_CONTEXT_CONTRACT,
            index,
            stored_rules,
            policy_bans,
            canonical_pool_sha256,
        }
    }

    pub fn canonical_pool_sha256(&self) -> &str {
        &self.canonical_pool_sha256
    }

    fn validate(&self) -> Result<(), ProposalError> {
        if self.contract_version != GATE1_REVIEW_CONTEXT_CONTRACT {
            return Err(ProposalError::InvalidReviewContext);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalError {
    InvalidCandidate(String),
    InvalidReviewContext,
    EligibilityRefused {
        candidate_id: String,
        reason: String,
    },
    SourceDigestConflict(String),
    InvalidValidatorReceipt,
    UntrustedValidator,
    ValidatorSignatureInvalid,
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
    pub contract_version: String,
    pub independent: bool,
    pub issuer_id: String,
    pub key_id: String,
    pub installation_id: String,
    pub validator_receipt_id: String,
    pub pending_manifest_sha256: String,
    pub canonical_pool_sha256: String,
    pub validated_at: String,
    pub decisions: Vec<SemanticDecisionV1>,
    pub signature_hex: String,
}

pub const SEMANTIC_ADJUDICATION_CONTRACT: &str = "adapt.semantic-adjudication.v1";
/// Local, explicit transcript review. This intentionally carries no issuer,
/// key, or signature: caller selection plus an independent human review is
/// the authority boundary for the local workflow.
pub const USER_TASTE_REVIEW_CONTRACT: &str = "adapt.user-taste-review.v1";
pub const SEMANTIC_ADJUDICATOR_TRUST_CONTRACT: &str = "adapt.semantic-adjudicator-trust.v1";
const ADJUDICATION_SIGNATURE_DOMAIN: &[u8] = b"Membrane Adapt semantic adjudication v1\0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedSemanticAdjudicatorV1 {
    pub issuer_id: String,
    pub key_id: String,
    pub public_key_hex: String,
    #[serde(default)]
    pub revoked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticAdjudicatorTrustStoreV1 {
    pub contract_version: String,
    pub installation_id: String,
    pub issuers: Vec<TrustedSemanticAdjudicatorV1>,
}

impl SemanticAdjudicatorTrustStoreV1 {
    pub fn load(path: &Path) -> Result<Self, ProposalError> {
        let bytes = std::fs::read(path).map_err(|_| ProposalError::UntrustedValidator)?;
        serde_json::from_slice(&bytes).map_err(|_| ProposalError::UntrustedValidator)
    }

    fn validate(&self) -> Result<(), ProposalError> {
        if self.contract_version != SEMANTIC_ADJUDICATOR_TRUST_CONTRACT
            || self.installation_id.trim().is_empty()
        {
            return Err(ProposalError::UntrustedValidator);
        }
        let mut seen = BTreeSet::new();
        for issuer in &self.issuers {
            if issuer.issuer_id.trim().is_empty()
                || issuer.key_id.trim().is_empty()
                || issuer.public_key_hex.len() != 64
                || !issuer
                    .public_key_hex
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                || !seen.insert((issuer.issuer_id.as_str(), issuer.key_id.as_str()))
            {
                return Err(ProposalError::UntrustedValidator);
            }
        }
        Ok(())
    }
}

/// Signature-verified adjudication capability. It has no public constructor
/// and cannot be deserialized from model/operator JSON.
#[derive(Debug, Clone)]
pub struct VerifiedSemanticAdjudication {
    adjudication: SemanticAdjudicationV1,
    receipt_sha256: String,
}

fn adjudication_payload_bytes(value: &SemanticAdjudicationV1) -> Result<Vec<u8>, ProposalError> {
    let mut payload =
        serde_json::to_value(value).map_err(|_| ProposalError::InvalidValidatorReceipt)?;
    payload
        .as_object_mut()
        .ok_or(ProposalError::InvalidValidatorReceipt)?
        .remove("signature_hex");
    Ok(crate::canonical::to_canonical_json(&payload).into_bytes())
}

/// Canonical, domain-separated bytes an external trusted validator signs.
pub fn semantic_adjudication_signing_bytes(
    value: &SemanticAdjudicationV1,
) -> Result<Vec<u8>, ProposalError> {
    let mut signed = ADJUDICATION_SIGNATURE_DOMAIN.to_vec();
    signed.extend_from_slice(&adjudication_payload_bytes(value)?);
    Ok(signed)
}

pub fn verify_semantic_adjudication(
    pending: &PreferenceManifestV1,
    value: SemanticAdjudicationV1,
    trust: &SemanticAdjudicatorTrustStoreV1,
) -> Result<VerifiedSemanticAdjudication, ProposalError> {
    trust.validate()?;
    if value.contract_version != SEMANTIC_ADJUDICATION_CONTRACT
        || !value.independent
        || value.installation_id != trust.installation_id
        || value.installation_id != pending.installation_id
        || value.pending_manifest_sha256 != pending.manifest_sha256
        || value.canonical_pool_sha256 != pending.canonical_pool_sha256
        || value.validator_receipt_id.trim().is_empty()
        || value.validated_at.trim().is_empty()
        || value.signature_hex.len() != 128
        || !value
            .signature_hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(ProposalError::InvalidValidatorReceipt);
    }
    let issuer = trust
        .issuers
        .iter()
        .find(|issuer| {
            !issuer.revoked && issuer.issuer_id == value.issuer_id && issuer.key_id == value.key_id
        })
        .ok_or(ProposalError::UntrustedValidator)?;
    let key = hex::decode(&issuer.public_key_hex).map_err(|_| ProposalError::UntrustedValidator)?;
    let signature_bytes =
        hex::decode(&value.signature_hex).map_err(|_| ProposalError::ValidatorSignatureInvalid)?;
    let signed = semantic_adjudication_signing_bytes(&value)?;
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(&signed, &signature_bytes)
        .map_err(|_| ProposalError::ValidatorSignatureInvalid)?;
    let receipt_sha256 = sha256_hex(
        &serde_json::to_vec(&value).map_err(|_| ProposalError::InvalidValidatorReceipt)?,
    );
    Ok(VerifiedSemanticAdjudication {
        adjudication: value,
        receipt_sha256,
    })
}

/// Verify an explicitly selected local transcript review.
///
/// Local review is deliberately not authenticated as an enterprise
/// adjudicator. Its binding is instead the exact pending manifest,
/// installation, canonical pool, complete decision set, and a non-empty
/// human review receipt. The returned capability is the same sealed type used
/// by the signed validator path, so downstream admission cannot distinguish
/// an unverified model/operator object from a completed review.
pub fn verify_user_taste_review(
    pending: &PreferenceManifestV1,
    value: SemanticAdjudicationV1,
) -> Result<VerifiedSemanticAdjudication, ProposalError> {
    if value.contract_version != USER_TASTE_REVIEW_CONTRACT
        || !value.independent
        || !value.issuer_id.is_empty()
        || !value.key_id.is_empty()
        || !value.signature_hex.is_empty()
        || value.installation_id.trim().is_empty()
        || value.installation_id != pending.installation_id
        || value.pending_manifest_sha256 != pending.manifest_sha256
        || value.canonical_pool_sha256 != pending.canonical_pool_sha256
        || value.validator_receipt_id.trim().is_empty()
        || value.validated_at.trim().is_empty()
    {
        return Err(ProposalError::InvalidValidatorReceipt);
    }
    let mut decisions = BTreeMap::new();
    for decision in &value.decisions {
        if !matches!(decision.verdict.as_str(), "valid" | "invalid")
            || decision.reason.trim().is_empty()
            || decisions.insert(decision.id.clone(), decision).is_some()
        {
            return Err(ProposalError::DecisionCoverageMismatch);
        }
    }
    let expected: BTreeSet<&str> = pending
        .records
        .iter()
        .map(|record| record.id.as_str())
        .collect();
    let found: BTreeSet<&str> = decisions.keys().map(String::as_str).collect();
    if expected != found {
        return Err(ProposalError::DecisionCoverageMismatch);
    }
    let receipt_sha256 = sha256_hex(
        &serde_json::to_vec(&value).map_err(|_| ProposalError::InvalidValidatorReceipt)?,
    );
    Ok(VerifiedSemanticAdjudication {
        adjudication: value,
        receipt_sha256,
    })
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
    if event.synthetic {
        flags.push("synthetic".into());
    }
    if event.meta {
        flags.push("meta".into());
    }
    if event.redacted {
        flags.push("redacted".into());
    }
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
    gate1: &Gate1ReviewContextV1,
) -> Result<PreferenceManifestV1, ProposalError> {
    if installation_id.trim().is_empty() || canonical_pool_sha256.trim().is_empty() {
        return Err(ProposalError::InvalidCandidate(
            "missing batch identity".into(),
        ));
    }
    gate1.validate()?;
    if canonical_pool_sha256 != gate1.canonical_pool_sha256() {
        return Err(ProposalError::CanonicalPoolMismatch);
    }
    // This builder is reached only from explicit transcript review. The
    // selected source digest plus external-user source event bind authority.
    let candidates: Vec<&TasteCandidateV1> = candidates.iter().collect();
    let mut source_digests: BTreeMap<String, String> = BTreeMap::new();
    for candidate in &candidates {
        let digest = candidate
            .source_transcript_sha256
            .trim_start_matches("sha256:")
            .to_lowercase();
        if !candidate.verify_integrity()
            || candidate.schema_version != TASTE_CANDIDATE_SCHEMA
            || candidate.source_session_id.trim().is_empty()
            || candidate.evidence_text_sha256 != sha256_hex(candidate.evidence_text.as_bytes())
            || digest.len() != 64
            || !digest.chars().all(|c| c.is_ascii_hexdigit())
            || candidate
                .context_events
                .iter()
                .filter(|event| event.is_source)
                .count()
                != 1
            || candidate.context_events.iter().any(|event| {
                event.is_source
                    && (event.kind != "user_message"
                        || event.role.as_deref() != Some("user")
                        || event.synthetic
                        || event.meta
                        || event.redacted
                        || event.text != candidate.evidence_text
                        || event.event_id != candidate.source_event_id
                        || event.byte_start != candidate.source_byte_start
                        || event.byte_end != candidate.source_byte_end)
            })
        {
            return Err(ProposalError::InvalidCandidate(
                candidate.candidate_id.clone(),
            ));
        }
        let declared_effect = authority_effect(candidate);
        let eligibility =
            crate::admission::evaluate_eligibility(&crate::admission::EligibilityInput {
                operation: "add",
                rule: &candidate.rule,
                category: &candidate.category,
                scope: &candidate.scope,
                scope_dimensions_raw: &candidate.scope_dimensions,
                record_class: &candidate.record_type,
                origin: crate::authority::Origin::UserTurn,
                evidence_text: &candidate.evidence_text,
                declared_authority_effect: Some(&declared_effect),
                policy_bans: &gate1.policy_bans,
                index: &gate1.index,
                stored_rules: &gate1.stored_rules,
            });
        if let crate::admission::EligibilityDecision::Refused { reason } = eligibility {
            return Err(ProposalError::EligibilityRefused {
                candidate_id: candidate.candidate_id.clone(),
                reason,
            });
        }
        match source_digests.insert(candidate.source_session_id.clone(), digest.clone()) {
            Some(previous) if previous != digest => {
                return Err(ProposalError::SourceDigestConflict(
                    candidate.source_session_id.clone(),
                ))
            }
            _ => {}
        }
    }
    let source_refs: Vec<SourceRef> = source_digests
        .iter()
        .map(|(source_id, sha256)| SourceRef {
            source_id: source_id.clone(),
            sha256: sha256.clone(),
        })
        .collect();
    let source_session_ids = source_refs
        .iter()
        .map(|item| item.source_id.clone())
        .collect();
    let authority_manifest_sha256 = sha256_canonical(&canonical_object([
        (
            "contract",
            serde_json::Value::String("adapt.authority.v1".into()),
        ),
        (
            "canonical_pool_sha256",
            serde_json::Value::String(canonical_pool_sha256.into()),
        ),
    ]));
    let mut records = Vec::new();
    for candidate in &candidates {
        let eid = evidence_id(candidate);
        let mut context_events: Vec<ContextEvent> =
            candidate.context_events.iter().map(context_event).collect();
        let mut record = ManifestRecord {
            id: candidate.candidate_id.clone(),
            rule: candidate.rule.clone(),
            category: candidate.category.clone(),
            scope: candidate.scope.clone(),
            scope_dimensions: BTreeMap2(candidate.scope_dimensions.clone()),
            record_type: candidate.record_type.clone(),
            evidence_class: serde_json::to_value(candidate.evidence_class)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or_else(|| ProposalError::InvalidCandidate(candidate.candidate_id.clone()))?,
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
            semantic_payload: None,
            semantic_digest: String::new(),
            evidence_contexts: vec![EvidenceContext {
                source_event_id: candidate.source_event_id.clone(),
                source_kind: "user_message".into(),
                source_role: "user".into(),
                source_classification: context_events
                    .iter()
                    .find(|event| event.is_source)
                    .map(|event| event.classification.clone())
                    .unwrap_or_default(),
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
        (
            "installation_id",
            serde_json::Value::String(installation_id.into()),
        ),
        (
            "canonical_pool_sha256",
            serde_json::Value::String(canonical_pool_sha256.into()),
        ),
        (
            "record_ids",
            serde_json::to_value(records.iter().map(|r| &r.id).collect::<Vec<_>>()).unwrap(),
        ),
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
        semantic_adjudication: None,
        duplicate_groups: vec![],
        duplicate_resolutions: vec![],
        records,
        manifest_sha256: String::new(),
    };
    output.manifest_sha256 = manifest::manifest_hash(&output);
    manifest::validate_schema(&output)
        .map_err(|error| ProposalError::Manifest(error.to_string()))?;
    Ok(output)
}

/// Bind complete held-out decisions to a pending manifest. Security-weakening
/// or permission-expanding candidates remain rejected regardless of model text.
pub fn adjudicate_manifest(
    pending: &PreferenceManifestV1,
    verified: &VerifiedSemanticAdjudication,
) -> Result<PreferenceManifestV1, ProposalError> {
    let adjudication = &verified.adjudication;
    let validated_at = adjudication.validated_at.as_str();
    manifest::validate_schema(pending)
        .map_err(|error| ProposalError::Manifest(error.to_string()))?;
    if !adjudication.independent {
        return Err(ProposalError::ValidatorNotIndependent);
    }
    if adjudication.canonical_pool_sha256 != pending.canonical_pool_sha256 {
        return Err(ProposalError::CanonicalPoolMismatch);
    }
    let mut decisions = BTreeMap::new();
    for decision in &adjudication.decisions {
        if decisions.insert(decision.id.clone(), decision).is_some() {
            return Err(ProposalError::DecisionCoverageMismatch);
        }
    }
    let expected: BTreeSet<&str> = pending
        .records
        .iter()
        .map(|record| record.id.as_str())
        .collect();
    let found: BTreeSet<&str> = decisions.keys().map(String::as_str).collect();
    if expected != found {
        return Err(ProposalError::DecisionCoverageMismatch);
    }

    let mut output = pending.clone();
    let canonical_pool_sha256 = output.canonical_pool_sha256.clone();
    for record in &mut output.records {
        let decision = decisions[record.id.as_str()];
        if !matches!(decision.verdict.as_str(), "valid" | "invalid") {
            return Err(ProposalError::UnsupportedVerdict(decision.verdict.clone()));
        }
        let unsafe_effect = matches!(
            record.authority_effect.as_str(),
            "permission_expanding" | "security_weakening"
        );
        record.status = if decision.verdict == "valid" && !unsafe_effect {
            "accepted"
        } else {
            "rejected"
        }
        .into();
        record.human_note = decision.reason.clone();
        record.updated_at = validated_at.into();
        record.last_verified_at = validated_at.into();
        record.verification_count = 1;
        record.validator_receipt_id = adjudication.validator_receipt_id.clone();
        record.validator_receipt_sha256 = verified.receipt_sha256.clone();
        manifest::seal_manifest_record(record, &canonical_pool_sha256)
            .map_err(|error| ProposalError::Manifest(error.to_string()))?;
        record.payload_sha256 = manifest::payload_sha256(record);
    }
    let duplicate_candidates: Vec<_> = output
        .records
        .iter()
        .filter(|record| record.status == "accepted")
        .map(|record| DuplicateCandidateV1 {
            record_id: record.id.clone(),
            canonical_text: record.rule.clone(),
            scope: record.scope.clone(),
            semantic_seal: record.semantic_digest.clone(),
            semantic_equivalence_digest: manifest::semantic_equivalence_digest(
                record
                    .semantic_payload
                    .as_ref()
                    .expect("adjudication seals records"),
            ),
            evidence_count: record.evidence_count,
            existing_canonical: false,
        })
        .collect();
    output.duplicate_groups =
        duplicate_groups::deterministic_exact_groups(&duplicate_candidates)
            .map_err(|error| ProposalError::Manifest(format!("duplicate groups: {error:?}")))?;
    output.duplicate_resolutions = output
        .duplicate_groups
        .iter()
        .map(|group| {
            duplicate_groups::verify_reviewed_resolution(group, None, &BTreeMap::new()).map_err(
                |error| ProposalError::Manifest(format!("duplicate abstention: {error:?}")),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let record_results = output
        .records
        .iter()
        .map(|record| SemanticRecordResult {
            id: record.id.clone(),
            payload_sha256: record.payload_sha256.clone(),
            status: record.status.clone(),
            verdict: if record.status == "accepted" {
                "valid".into()
            } else {
                "invalid".into()
            },
        })
        .collect();
    let mut receipt = SemanticValidationReceipt {
        contract: SEMANTIC_VALIDATION_CONTRACT.into(),
        complete: true,
        independent: true,
        canonical_pool_sha256: output.canonical_pool_sha256.clone(),
        record_results,
        receipt_sha256: String::new(),
    };
    let mut value = serde_json::to_value(&receipt).expect("semantic receipt serializes");
    value
        .as_object_mut()
        .expect("receipt object")
        .remove("receipt_sha256");
    receipt.receipt_sha256 = sha256_canonical(&value);
    output.semantic_validation = Some(receipt);
    output.semantic_adjudication = Some(adjudication.clone());
    output.manifest_sha256 = manifest::manifest_hash(&output);
    manifest::apply_time_validate(&output)
        .map_err(|error| ProposalError::Manifest(error.to_string()))?;
    Ok(output)
}

fn reconstruct_pending_manifest(finalised: &PreferenceManifestV1) -> PreferenceManifestV1 {
    let mut pending = finalised.clone();
    pending.semantic_validation = None;
    pending.semantic_adjudication = None;
    pending.duplicate_groups.clear();
    pending.duplicate_resolutions.clear();
    for record in &mut pending.records {
        record.status = "pending".into();
        record.human_note.clear();
        record.updated_at = record.created_at.clone();
        record.last_verified_at.clear();
        record.verification_count = 0;
        record.validator_receipt_id.clear();
        record.validator_receipt_sha256.clear();
        record.semantic_payload = None;
        record.semantic_digest.clear();
        record.payload_sha256 = manifest::payload_sha256(record);
    }
    pending.manifest_sha256 = manifest::manifest_hash(&pending);
    pending
}

pub fn verify_final_manifest_adjudication(
    finalised: &PreferenceManifestV1,
    trust: &SemanticAdjudicatorTrustStoreV1,
) -> Result<(), ProposalError> {
    verify_final_manifest_adjudication_with_verified(
        finalised,
        &verify_semantic_adjudication(
            &reconstruct_pending_manifest(finalised),
            finalised
                .semantic_adjudication
                .clone()
                .ok_or(ProposalError::InvalidValidatorReceipt)?,
            trust,
        )?,
    )
}

/// Verify a final manifest produced by either enterprise signed adjudication
/// or explicit local user review. Pass `None` for local-only verification; a
/// signed adjudication still requires its trust store.
pub fn verify_final_manifest_adjudication_or_local(
    finalised: &PreferenceManifestV1,
    trust: Option<&SemanticAdjudicatorTrustStoreV1>,
) -> Result<(), ProposalError> {
    let value = finalised
        .semantic_adjudication
        .clone()
        .ok_or(ProposalError::InvalidValidatorReceipt)?;
    let pending = reconstruct_pending_manifest(finalised);
    let verified = if value.contract_version == USER_TASTE_REVIEW_CONTRACT {
        verify_user_taste_review(&pending, value)?
    } else {
        verify_semantic_adjudication(
            &pending,
            value,
            trust.ok_or(ProposalError::UntrustedValidator)?,
        )?
    };
    verify_final_manifest_adjudication_with_verified(finalised, &verified)
}

fn verify_final_manifest_adjudication_with_verified(
    finalised: &PreferenceManifestV1,
    verified: &VerifiedSemanticAdjudication,
) -> Result<(), ProposalError> {
    let expected: BTreeMap<&str, &str> = verified
        .adjudication
        .decisions
        .iter()
        .map(|decision| (decision.id.as_str(), decision.verdict.as_str()))
        .collect();
    if expected.len() != finalised.records.len() {
        return Err(ProposalError::DecisionCoverageMismatch);
    }
    for record in &finalised.records {
        let verdict = expected
            .get(record.id.as_str())
            .ok_or(ProposalError::DecisionCoverageMismatch)?;
        let unsafe_effect = matches!(
            record.authority_effect.as_str(),
            "permission_expanding" | "security_weakening"
        );
        let expected_status = if *verdict == "valid" && !unsafe_effect {
            "accepted"
        } else {
            "rejected"
        };
        if record.status != expected_status
            || record.validator_receipt_sha256 != verified.receipt_sha256
        {
            return Err(ProposalError::InvalidValidatorReceipt);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::AuthorityEffect;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn candidate(rule: &str, effect: AuthorityEffect) -> TasteCandidateV1 {
        crate::taste::test_candidate(rule, effect)
    }

    fn gate1() -> Gate1ReviewContextV1 {
        Gate1ReviewContextV1::from_verified_canonical_inventory(vec![])
    }

    fn verified_adjudication(
        pending: &PreferenceManifestV1,
        decisions: Vec<SemanticDecisionV1>,
    ) -> VerifiedSemanticAdjudication {
        let key = Ed25519KeyPair::from_seed_unchecked(&[41; 32]).unwrap();
        let trust = SemanticAdjudicatorTrustStoreV1 {
            contract_version: SEMANTIC_ADJUDICATOR_TRUST_CONTRACT.into(),
            installation_id: pending.installation_id.clone(),
            issuers: vec![TrustedSemanticAdjudicatorV1 {
                issuer_id: "validator".into(),
                key_id: "key-1".into(),
                public_key_hex: hex::encode(key.public_key().as_ref()),
                revoked: false,
            }],
        };
        let mut value = SemanticAdjudicationV1 {
            contract_version: SEMANTIC_ADJUDICATION_CONTRACT.into(),
            independent: true,
            issuer_id: "validator".into(),
            key_id: "key-1".into(),
            installation_id: pending.installation_id.clone(),
            validator_receipt_id: "validator-1".into(),
            pending_manifest_sha256: pending.manifest_sha256.clone(),
            canonical_pool_sha256: pending.canonical_pool_sha256.clone(),
            validated_at: "t2".into(),
            decisions,
            signature_hex: String::new(),
        };
        value.signature_hex = hex::encode(
            key.sign(&semantic_adjudication_signing_bytes(&value).unwrap())
                .as_ref(),
        );
        verify_semantic_adjudication(pending, value, &trust).unwrap()
    }

    fn adjudicate(candidates: &[TasteCandidateV1]) -> PreferenceManifestV1 {
        let gate1 = gate1();
        let pending =
            build_pending_manifest(candidates, "i", gate1.canonical_pool_sha256(), "t", &gate1)
                .unwrap();
        let verified = verified_adjudication(
            &pending,
            candidates
                .iter()
                .map(|candidate| SemanticDecisionV1 {
                    id: candidate.candidate_id.clone(),
                    verdict: "valid".into(),
                    reason: "direct".into(),
                })
                .collect(),
        );
        adjudicate_manifest(&pending, &verified).unwrap()
    }

    fn canonical_record(stored_rule: crate::authority::StoredRule) -> CanonicalTasteRecordV1 {
        let finalised = adjudicate(&[candidate(&stored_rule.rule, AuthorityEffect::Neutral)]);
        let record = &finalised.records[0];
        CanonicalTasteRecordV1 {
            stored_rule,
            payload_sha256: record.payload_sha256.clone(),
            semantic_digest: record.semantic_digest.clone(),
            semantic_payload: record.semantic_payload.clone().unwrap(),
            authority_manifest_sha256: record.authority_manifest_sha256.clone(),
            validator_receipt_id: record.validator_receipt_id.clone(),
            validator_receipt_sha256: record.validator_receipt_sha256.clone(),
            current_authority: "A2".into(),
            current_influence_class: "behavioral_directive".into(),
        }
    }

    fn pool(records: Vec<CanonicalTasteRecordV1>) -> String {
        Gate1ReviewContextV1::from_verified_canonical_inventory(records)
            .canonical_pool_sha256()
            .to_string()
    }

    #[test]
    fn pending_to_adjudicated_manifest_is_fully_bound() {
        let gate1 = gate1();
        let pending = build_pending_manifest(
            &[candidate("Always run tests", AuthorityEffect::Neutral)],
            "i",
            gate1.canonical_pool_sha256(),
            "t",
            &gate1,
        )
        .unwrap();
        assert_eq!(pending.records[0].status, "pending");
        let verified = verified_adjudication(
            &pending,
            vec![SemanticDecisionV1 {
                id: "taste_x".into(),
                verdict: "valid".into(),
                reason: "direct".into(),
            }],
        );
        let finalised = adjudicate_manifest(&pending, &verified).unwrap();
        assert_eq!(manifest::apply_plan(&finalised).unwrap(), vec!["taste_x"]);
    }

    #[test]
    fn explicit_user_taste_review_is_bound_without_login_or_trust_store() {
        let gate1 = gate1();
        let pending = build_pending_manifest(
            &[candidate(
                "Always run focused tests",
                AuthorityEffect::Neutral,
            )],
            "installation-1",
            gate1.canonical_pool_sha256(),
            "2026-08-26T00:00:00Z",
            &gate1,
        )
        .unwrap();
        let review = SemanticAdjudicationV1 {
            contract_version: USER_TASTE_REVIEW_CONTRACT.into(),
            independent: true,
            issuer_id: String::new(),
            key_id: String::new(),
            installation_id: pending.installation_id.clone(),
            validator_receipt_id: "local-review-1".into(),
            pending_manifest_sha256: pending.manifest_sha256.clone(),
            canonical_pool_sha256: pending.canonical_pool_sha256.clone(),
            validated_at: "2026-08-26T00:01:00Z".into(),
            decisions: vec![SemanticDecisionV1 {
                id: pending.records[0].id.clone(),
                verdict: "valid".into(),
                reason: "Explicitly reviewed by user".into(),
            }],
            signature_hex: String::new(),
        };
        let verified = verify_user_taste_review(&pending, review.clone()).unwrap();
        let finalised = adjudicate_manifest(&pending, &verified).unwrap();
        verify_final_manifest_adjudication_or_local(&finalised, None).unwrap();

        let mut incomplete = review;
        incomplete.decisions.clear();
        assert!(matches!(
            verify_user_taste_review(&pending, incomplete),
            Err(ProposalError::DecisionCoverageMismatch)
        ));
    }

    #[test]
    fn gate_one_refuses_security_weakening_before_adjudication() {
        let gate1 = gate1();
        let refused = build_pending_manifest(
            &[candidate(
                "Never validate TLS certificates",
                AuthorityEffect::SecurityWeakening,
            )],
            "i",
            gate1.canonical_pool_sha256(),
            "t",
            &gate1,
        )
        .unwrap_err();
        assert!(matches!(refused, ProposalError::EligibilityRefused { .. }));
    }

    #[test]
    fn gate_one_uses_canonical_inventory_but_not_pending_batch_for_duplicates() {
        let existing = crate::authority::StoredRule {
            id: crate::record::RuleKey::new("repo-x", "Always run tests").record_id,
            rule: "Always run tests".into(),
            scope: "repo-x".into(),
            lifecycle_state: "active".into(),
        };
        let context =
            Gate1ReviewContextV1::from_verified_canonical_inventory(vec![canonical_record(
                existing,
            )]);
        let pool = context.canonical_pool_sha256().to_string();
        let refused = build_pending_manifest(
            &[candidate("Always run tests", AuthorityEffect::Neutral)],
            "i",
            &pool,
            "t",
            &context,
        )
        .unwrap_err();
        assert!(matches!(
            refused,
            ProposalError::EligibilityRefused { reason, .. } if reason == "rule-duplicate"
        ));

        let mut first = candidate("Always run tests", AuthorityEffect::Neutral);
        first.candidate_id = "pending-a".into();
        first.reseal_for_test();
        let mut second = first.clone();
        second.candidate_id = "pending-b".into();
        second.reseal_for_test();
        let gate1 = gate1();
        let pending = build_pending_manifest(
            &[first, second],
            "i",
            gate1.canonical_pool_sha256(),
            "t",
            &gate1,
        )
        .unwrap();
        assert_eq!(pending.records.len(), 2);
        let verified = verified_adjudication(
            &pending,
            pending
                .records
                .iter()
                .map(|record| SemanticDecisionV1 {
                    id: record.id.clone(),
                    verdict: "valid".into(),
                    reason: "direct".into(),
                })
                .collect(),
        );
        let grouped = adjudicate_manifest(&pending, &verified).unwrap();
        assert_eq!(grouped.duplicate_groups.len(), 1);
        assert_eq!(
            grouped.duplicate_resolutions[0].disposition,
            crate::duplicate_groups::DuplicateDispositionV1::Abstain
        );
    }

    #[test]
    fn canonical_pool_binds_semantics_applicability_authority_and_adjudication() {
        let stored = crate::authority::StoredRule {
            id: "stored-1".into(),
            rule: "Always run focused tests".into(),
            scope: "repo-x".into(),
            lifecycle_state: "active".into(),
        };
        let base = canonical_record(stored);
        let base_pool = pool(vec![base.clone()]);
        let mut mutations = Vec::new();

        let mut category = base.clone();
        category.semantic_payload.category = "security".into();
        mutations.push(("category", category));

        let mut class = base.clone();
        class.semantic_payload.record_class = Some(crate::record::RecordClass::ScopedPreference);
        mutations.push(("record_class", class));

        let mut dimensions = base.clone();
        dimensions.semantic_payload.scope_dimensions = crate::scope::ScopeDimensions::normalize(
            &BTreeMap::from([("repo".into(), "other".into())]),
        )
        .unwrap();
        mutations.push(("scope_dimensions", dimensions));

        let mut authority = base.clone();
        authority.semantic_payload.authority_tier =
            crate::authority::PrecedenceTier::ProvisionalCandidate;
        mutations.push(("authority_tier", authority));

        let mut influence = base.clone();
        influence.semantic_payload.influence_class =
            crate::record::InfluenceClass::BehavioralDirective;
        mutations.push(("influence_class", influence));

        let mut digest = base.clone();
        digest.semantic_digest = "sha256:changed".into();
        mutations.push(("semantic_digest", digest));

        let mut adjudication = base.clone();
        adjudication.validator_receipt_sha256 = "sha256:other-adjudication".into();
        mutations.push(("adjudication_provenance", adjudication));

        let mut envelope = base;
        envelope.current_authority = "A3".into();
        mutations.push(("current_authority", envelope));

        for (field, changed) in mutations {
            assert_ne!(base_pool, pool(vec![changed]), "pool omitted {field}");
        }
    }

    #[test]
    fn canonical_pool_binds_executable_policy_ban_definitions() {
        let first = Gate1ReviewContextV1::from_verified_canonical_inventory_with_policy(
            vec![],
            "sha256:fixed-executable-policy".into(),
            &[("blocked", r"(?i)\bsecret\b")],
        );
        let changed_pattern = Gate1ReviewContextV1::from_verified_canonical_inventory_with_policy(
            vec![],
            "sha256:fixed-executable-policy".into(),
            &[("blocked", r"(?i)\bcredential\b")],
        );
        let changed_reason = Gate1ReviewContextV1::from_verified_canonical_inventory_with_policy(
            vec![],
            "sha256:fixed-executable-policy".into(),
            &[("different-reason", r"(?i)\bsecret\b")],
        );
        assert_ne!(
            first.canonical_pool_sha256(),
            changed_pattern.canonical_pool_sha256()
        );
        assert_ne!(
            first.canonical_pool_sha256(),
            changed_reason.canonical_pool_sha256()
        );
    }

    #[test]
    fn canonical_pool_binds_non_ban_executable_policy_fingerprint() {
        let base = Gate1ReviewContextV1::from_verified_canonical_inventory_with_policy(
            vec![],
            "sha256:rule-shape-min-15".into(),
            &[("blocked", r"(?i)\bsecret\b")],
        );
        for (policy, changed_fingerprint) in [
            ("rule-shape", "sha256:rule-shape-min-16"),
            ("taxonomy", "sha256:taxonomy-with-new-category"),
            ("scope", "sha256:scope-with-new-dimension"),
            ("authority", "sha256:authority-effect-order-changed"),
            ("contradiction", "sha256:contradiction-overlap-changed"),
        ] {
            let changed = Gate1ReviewContextV1::from_verified_canonical_inventory_with_policy(
                vec![],
                changed_fingerprint.into(),
                &[("blocked", r"(?i)\bsecret\b")],
            );
            assert_ne!(
                base.canonical_pool_sha256(),
                changed.canonical_pool_sha256(),
                "pool omitted {policy} executable policy"
            );
        }
    }

    #[test]
    fn duplicates_abstain_and_cross_semantics_never_group() {
        let mut a = candidate("Always run tests", AuthorityEffect::Neutral);
        a.candidate_id = "a".into();
        a.reseal_for_test();
        let mut b = a.clone();
        b.candidate_id = "b".into();
        b.reseal_for_test();
        let exact = adjudicate(&[a.clone(), b.clone()]);
        assert_eq!(exact.duplicate_groups.len(), 1);
        assert_eq!(
            exact.duplicate_resolutions[0].disposition,
            crate::duplicate_groups::DuplicateDispositionV1::Abstain
        );
        assert_eq!(manifest::apply_plan(&exact).unwrap(), vec!["a", "b"]);

        b.category = "code-style".into();
        b.reseal_for_test();
        let cross_category = adjudicate(&[a.clone(), b.clone()]);
        assert!(cross_category.duplicate_groups.is_empty());
        b.category = a.category.clone();
        b.record_type = "scoped_preference".into();
        b.reseal_for_test();
        let cross_class = adjudicate(&[a, b]);
        assert!(cross_class.duplicate_groups.is_empty());

        let mut a = candidate("Always run tests", AuthorityEffect::Neutral);
        a.candidate_id = "a".into();
        a.reseal_for_test();
        let mut b = a.clone();
        b.candidate_id = "b".into();
        b.reseal_for_test();
        let gate1 = gate1();
        let mut pending =
            build_pending_manifest(&[a, b], "i", gate1.canonical_pool_sha256(), "t", &gate1)
                .unwrap();
        pending.records[1]
            .scope_dimensions
            .0
            .insert("repo".into(), "other".into());
        pending.records[1].payload_sha256 = manifest::payload_sha256(&pending.records[1]);
        pending.manifest_sha256 = manifest::manifest_hash(&pending);
        let verified = verified_adjudication(
            &pending,
            vec![
                SemanticDecisionV1 {
                    id: "a".into(),
                    verdict: "valid".into(),
                    reason: String::new(),
                },
                SemanticDecisionV1 {
                    id: "b".into(),
                    verdict: "valid".into(),
                    reason: String::new(),
                },
            ],
        );
        let cross_dimensions = adjudicate_manifest(&pending, &verified).unwrap();
        assert!(cross_dimensions.duplicate_groups.is_empty());
    }

    #[test]
    fn mutable_lifecycle_stays_outside_semantic_seal() {
        let mut finalised = adjudicate(&[candidate("Always run tests", AuthorityEffect::Neutral)]);
        finalised.records[0].lifecycle_state = "active".into();
        finalised.records[0].verification_count += 1;
        manifest::verify_manifest_record_seal(
            &finalised.records[0],
            &finalised.canonical_pool_sha256,
        )
        .unwrap();
    }
}
