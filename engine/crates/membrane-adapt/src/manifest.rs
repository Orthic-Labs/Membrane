//! Deterministic preference manifest build, validation, and idempotent
//! deterministic apply (Gate-1a contract ported and hardened).
//!
//! The manifest is the only path from mined candidates to durable apply. The
//! loader refuses: unknown schema versions, records whose `payload_sha256`
//! mismatches content, any `pending` record at apply time, missing batch
//! identity, evidence contexts without exactly one eligible external-user
//! source event, and semantic-validation receipts that do
//! not exactly cover the manifest's records.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::authority::{AuthorityEffect, PrecedenceTier};
use crate::canonical::{canonical_object, sha256_canonical};
use crate::duplicate_groups::{
    build_group, verify_reviewed_resolution, DuplicateCandidateV1, DuplicateGroupV1,
    DuplicateResolutionV1, DETERMINISTIC_EXACT_ALGORITHM, DUPLICATE_GROUP_CONTRACT,
};
use crate::record::{InfluenceClass, RecordClass};
use crate::scope::ScopeDimensions;
use crate::seal::{
    verify_seal, SemanticPayloadV1, ADMISSION_POLICY_VERSION, PROVENANCE_CONTRACT_VERSION,
    REDACTION_CONTRACT_VERSION, SEAL_CONTRACT_VERSION,
};

pub const MANIFEST_SCHEMA_VERSION: &str = "1.4.0";
pub const SEMANTIC_VALIDATION_CONTRACT: &str = "direct-evidence-global-pool-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    UnsupportedSchema(String),
    PayloadMismatch {
        record_id: String,
    },
    PendingAtApplyTime {
        record_id: String,
    },
    MissingBatchIdentity,
    DuplicateSourceRef(String),
    SourceSessionMismatch,
    UnknownSourceId {
        record_id: String,
        source_id: String,
    },
    EvidenceContextInvalid {
        record_id: String,
        reason: String,
    },
    SemanticValidation(String),
    SemanticSeal {
        record_id: String,
        reason: String,
    },
    NotJson(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::UnsupportedSchema(v) => write!(f, "unsupported manifest schema: {v}"),
            ManifestError::PayloadMismatch { record_id } => {
                write!(f, "payload_sha256 mismatch on record {record_id}")
            }
            ManifestError::PendingAtApplyTime { record_id } => {
                write!(f, "record {record_id} still pending at apply time")
            }
            ManifestError::MissingBatchIdentity => write!(f, "missing batch identity"),
            ManifestError::DuplicateSourceRef(id) => write!(f, "duplicate source_ref: {id}"),
            ManifestError::SourceSessionMismatch => {
                write!(f, "source_session_ids must match source_refs order")
            }
            ManifestError::UnknownSourceId {
                record_id,
                source_id,
            } => {
                write!(
                    f,
                    "record {record_id} references unknown source {source_id}"
                )
            }
            ManifestError::EvidenceContextInvalid { record_id, reason } => {
                write!(f, "record {record_id} evidence context invalid: {reason}")
            }
            ManifestError::SemanticValidation(msg) => write!(f, "semantic validation: {msg}"),
            ManifestError::SemanticSeal { record_id, reason } => {
                write!(f, "semantic seal {record_id}: {reason}")
            }
            ManifestError::NotJson(msg) => write!(f, "manifest is not valid JSON: {msg}"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// One bound transcript source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub source_id: String,
    pub sha256: String,
}

/// A byte-span evidence context entry. `is_source` marks the single
/// authority-bearing external-user event every record must carry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEvent {
    pub event_id: String,
    pub kind: String,
    pub role: String,
    pub classification: String,
    pub flags: Vec<String>,
    pub byte_start: i64,
    pub byte_end: i64,
    pub text: String,
    pub provenance: String,
    pub is_source: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceContext {
    pub source_event_id: String,
    pub source_kind: String,
    pub source_role: String,
    pub source_classification: String,
    pub source_flags: Vec<String>,
    pub source_byte_start: i64,
    pub source_byte_end: i64,
    pub evidence_text: String,
    pub context_events: Vec<ContextEvent>,
}

/// One candidate in the manifest. Immutable fields are hashed into
/// `payload_sha256`; adjudication may flip only `status`/`human_note`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestRecord {
    pub id: String,
    pub rule: String,
    pub category: String,
    pub scope: String,
    #[serde(default)]
    pub scope_dimensions: BTreeMap2,
    pub record_type: String,
    /// Evidence authority class computed before proposal construction.
    pub evidence_class: String,
    pub authority_effect: String,
    pub status: String,
    pub confidence: f64,
    pub needs_review: bool,
    pub evidence_count: u32,
    pub created_at: String,
    pub updated_at: String,
    pub evidence_excerpt: String,
    pub source_ids: Vec<String>,
    pub source_file_hashes: Vec<SourceFileHash>,
    pub evidence_ids: Vec<EvidenceIdBinding>,
    pub retrieval_aliases: Vec<String>,
    #[serde(default)]
    pub human_note: String,
    pub payload_sha256: String,
    pub operation: String,
    pub machine: String,
    pub machine_only: bool,
    pub lifecycle_state: String,
    pub last_verified_at: String,
    pub verification_count: u32,
    pub authority_manifest_sha256: String,
    /// Semantic-validation receipt binding; required non-empty on accepted
    /// records at apply time.
    pub validator_receipt_id: String,
    pub validator_receipt_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_payload: Option<SemanticPayloadV1>,
    #[serde(default)]
    pub semantic_digest: String,
    pub evidence_contexts: Vec<EvidenceContext>,
}

/// Wrapper so scope_dimensions round-trips as a JSON object of strings while
/// staying an ordered map internally.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BTreeMap2(pub std::collections::BTreeMap<String, String>);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceFileHash {
    pub session_id: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceIdBinding {
    pub evidence_id: String,
    pub source_session_id: String,
    pub excerpt: String,
}

/// The full manifest document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceManifestV1 {
    pub schema_version: String,
    pub batch_id: String,
    pub created_at: String,
    pub installation_id: String,
    pub canonical_pool_sha256: String,
    pub source_refs: Vec<SourceRef>,
    pub source_session_ids: Vec<String>,
    #[serde(default)]
    pub forbidden_scopes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_validation: Option<SemanticValidationReceipt>,
    /// Trusted signed adjudication carried through to the irreversible apply
    /// boundary; a digest-shaped validator field alone is never authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_adjudication: Option<crate::proposal::SemanticAdjudicationV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicate_groups: Vec<DuplicateGroupV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicate_resolutions: Vec<DuplicateResolutionV1>,
    pub records: Vec<ManifestRecord>,
    /// Digest over everything above except this field itself.
    pub manifest_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticValidationReceipt {
    pub contract: String,
    pub complete: bool,
    pub independent: bool,
    pub canonical_pool_sha256: String,
    pub record_results: Vec<SemanticRecordResult>,
    pub receipt_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticRecordResult {
    pub id: String,
    pub payload_sha256: String,
    pub status: String,
    pub verdict: String,
}

fn normalize_source_file_hashes(rec: &ManifestRecord) -> Vec<Value> {
    let mut items: Vec<(String, Value)> = rec
        .source_file_hashes
        .iter()
        .map(|h| {
            (
                h.session_id.clone(),
                canonical_object([
                    ("session_id", Value::String(h.session_id.clone())),
                    ("sha256", Value::String(h.sha256.to_lowercase())),
                ]),
            )
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items.into_iter().map(|(_, v)| v).collect()
}

fn normalize_evidence_ids(rec: &ManifestRecord) -> Vec<Value> {
    let mut items: Vec<(String, Value)> = rec
        .evidence_ids
        .iter()
        .map(|e| {
            (
                e.evidence_id.clone(),
                canonical_object([
                    ("evidence_id", Value::String(e.evidence_id.clone())),
                    (
                        "source_session_id",
                        Value::String(e.source_session_id.clone()),
                    ),
                    ("excerpt", Value::String(e.excerpt.clone())),
                ]),
            )
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items.into_iter().map(|(_, v)| v).collect()
}

/// Stable projection used to compute `payload_sha256`. Lists are sorted so
/// list-order edits cannot change the hash.
pub fn candidate_payload(rec: &ManifestRecord) -> Value {
    let mut source_ids = rec.source_ids.clone();
    source_ids.sort();
    let mut aliases: Vec<String> = rec
        .retrieval_aliases
        .iter()
        .map(|a| a.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|a| !a.is_empty())
        .collect();
    aliases.sort();
    aliases.dedup();
    let mut pairs: Vec<(&str, Value)> = vec![
        ("id", Value::String(rec.id.clone())),
        ("rule", Value::String(rec.rule.clone())),
        ("category", Value::String(rec.category.clone())),
        ("scope", Value::String(rec.scope.clone())),
        (
            "scope_dimensions",
            serde_json::to_value(&rec.scope_dimensions).unwrap_or(Value::Null),
        ),
        ("source_ids", serde_json::to_value(source_ids).unwrap()),
        (
            "source_file_hashes",
            Value::Array(normalize_source_file_hashes(rec)),
        ),
        ("evidence_ids", Value::Array(normalize_evidence_ids(rec))),
        ("evidence_count", Value::from(rec.evidence_count)),
        (
            "evidence_excerpt",
            Value::String(rec.evidence_excerpt.clone()),
        ),
        ("record_type", Value::String(rec.record_type.clone())),
        ("evidence_class", Value::String(rec.evidence_class.clone())),
        ("confidence", Value::from(rec.confidence)),
        ("needs_review", Value::Bool(rec.needs_review)),
        (
            "authority_effect",
            Value::String(rec.authority_effect.clone()),
        ),
        (
            "authority_manifest_sha256",
            Value::String(rec.authority_manifest_sha256.clone()),
        ),
        ("retrieval_aliases", serde_json::to_value(aliases).unwrap()),
        ("operation", Value::String(rec.operation.clone())),
        ("machine", Value::String(rec.machine.clone())),
        ("machine_only", Value::Bool(rec.machine_only)),
        (
            "validator_receipt_id",
            Value::String(rec.validator_receipt_id.clone()),
        ),
        (
            "validator_receipt_sha256",
            Value::String(rec.validator_receipt_sha256.clone()),
        ),
        (
            "semantic_payload",
            serde_json::to_value(&rec.semantic_payload).unwrap(),
        ),
        (
            "semantic_digest",
            Value::String(rec.semantic_digest.clone()),
        ),
        (
            "evidence_contexts",
            serde_json::to_value(&rec.evidence_contexts).unwrap_or(Value::Null),
        ),
    ];
    // Canonical ordering by key happens inside canonical_object serialization;
    // build a map directly.
    let mut map = serde_json::Map::new();
    for (k, v) in pairs.drain(..) {
        map.insert(k.to_string(), v);
    }
    Value::Object(map)
}

pub fn payload_sha256(rec: &ManifestRecord) -> String {
    sha256_canonical(&candidate_payload(rec))
}

fn authority_effect(value: &str) -> Result<AuthorityEffect, ManifestError> {
    serde_json::from_value(Value::String(value.to_string())).map_err(|_| {
        ManifestError::SemanticSeal {
            record_id: "<authority-effect>".into(),
            reason: "unknown authority effect".into(),
        }
    })
}

pub fn semantic_payload_for_record(
    rec: &ManifestRecord,
    canonical_pool_sha256: &str,
) -> Result<SemanticPayloadV1, ManifestError> {
    let record_class =
        RecordClass::parse(&rec.record_type).ok_or_else(|| ManifestError::SemanticSeal {
            record_id: rec.id.clone(),
            reason: "unknown record class".into(),
        })?;
    let scope_dimensions =
        ScopeDimensions::normalize(&rec.scope_dimensions.0).map_err(|error| {
            ManifestError::SemanticSeal {
                record_id: rec.id.clone(),
                reason: format!("scope: {error}"),
            }
        })?;
    let mut evidence = rec
        .source_file_hashes
        .iter()
        .map(|item| item.sha256.trim_start_matches("sha256:").to_lowercase())
        .collect::<Vec<_>>();
    evidence.push(crate::canonical::sha256_hex(
        rec.evidence_excerpt.as_bytes(),
    ));
    evidence.sort();
    evidence.dedup();
    let user_authoritative = rec.evidence_class == "user_authoritative";
    let user_behavioral = rec.evidence_class == "user_behavioral";
    if !user_authoritative && !user_behavioral {
        return Err(ManifestError::SemanticSeal {
            record_id: rec.id.clone(),
            reason: "non-Taste evidence class".into(),
        });
    }
    Ok(SemanticPayloadV1 {
        seal_contract_version: SEAL_CONTRACT_VERSION.into(),
        record_kind: "preference".into(),
        category: rec.category.clone(),
        canonical_text: crate::canonical::normalize_text(&rec.rule),
        scope: rec.scope.clone(),
        scope_dimensions,
        authority_tier: match (
            user_authoritative,
            rec.scope.trim().eq_ignore_ascii_case("global"),
        ) {
            (true, true) => PrecedenceTier::ExplicitGlobalUserPreference,
            (true, false) => PrecedenceTier::ExplicitScopedUserPreference,
            (false, true) => PrecedenceTier::InferredGlobalUserPreference,
            (false, false) => PrecedenceTier::InferredScopedUserPreference,
        },
        authority_effect: authority_effect(&rec.authority_effect)?,
        influence_class: if user_behavioral || rec.needs_review {
            InfluenceClass::Provisional
        } else {
            InfluenceClass::BehavioralDirective
        },
        record_class: Some(record_class),
        machine_binding: rec.machine_only.then(|| rec.machine.clone()),
        source_evidence_digests: evidence,
        canonical_pool_sha256: canonical_pool_sha256.into(),
        admission_policy_version: ADMISSION_POLICY_VERSION.into(),
        validator_receipt_id: rec.validator_receipt_id.clone(),
        validator_receipt_sha256: rec.validator_receipt_sha256.clone(),
        redaction_contract_version: REDACTION_CONTRACT_VERSION.into(),
        provenance_contract_version: PROVENANCE_CONTRACT_VERSION.into(),
    })
}

pub fn seal_manifest_record(
    rec: &mut ManifestRecord,
    canonical_pool_sha256: &str,
) -> Result<(), ManifestError> {
    let payload = semantic_payload_for_record(rec, canonical_pool_sha256)?;
    rec.semantic_digest = payload.seal_digest();
    rec.semantic_payload = Some(payload);
    Ok(())
}

/// Digest of sealed meaning/applicability fields only. Evidence, validation,
/// and provenance bindings remain in the full semantic seal but cannot make
/// otherwise-identical semantics look different during duplicate grouping.
pub fn semantic_equivalence_digest(payload: &SemanticPayloadV1) -> String {
    sha256_canonical(&canonical_object([
        (
            "seal_contract_version",
            Value::String(payload.seal_contract_version.clone()),
        ),
        ("record_kind", Value::String(payload.record_kind.clone())),
        ("category", Value::String(payload.category.clone())),
        (
            "canonical_text",
            Value::String(payload.canonical_text.clone()),
        ),
        ("scope", Value::String(payload.scope.clone())),
        (
            "scope_dimensions",
            serde_json::to_value(&payload.scope_dimensions).expect("scope serializes"),
        ),
        (
            "authority_tier",
            serde_json::to_value(payload.authority_tier).expect("authority serializes"),
        ),
        (
            "authority_effect",
            serde_json::to_value(payload.authority_effect).expect("effect serializes"),
        ),
        (
            "influence_class",
            serde_json::to_value(payload.influence_class).expect("influence serializes"),
        ),
        (
            "record_class",
            serde_json::to_value(payload.record_class).expect("class serializes"),
        ),
        (
            "machine_binding",
            serde_json::to_value(&payload.machine_binding).expect("machine serializes"),
        ),
        (
            "admission_policy_version",
            Value::String(payload.admission_policy_version.clone()),
        ),
        (
            "redaction_contract_version",
            Value::String(payload.redaction_contract_version.clone()),
        ),
    ]))
}

pub fn verify_manifest_record_seal<'a>(
    rec: &'a ManifestRecord,
    canonical_pool_sha256: &str,
) -> Result<&'a SemanticPayloadV1, ManifestError> {
    let payload = rec
        .semantic_payload
        .as_ref()
        .ok_or_else(|| ManifestError::SemanticSeal {
            record_id: rec.id.clone(),
            reason: "missing payload".into(),
        })?;
    let expected = semantic_payload_for_record(rec, canonical_pool_sha256)?;
    if payload != &expected || verify_seal(payload, &rec.semantic_digest).is_err() {
        return Err(ManifestError::SemanticSeal {
            record_id: rec.id.clone(),
            reason: "payload or digest mismatch".into(),
        });
    }
    Ok(payload)
}

/// Hash a full manifest over everything except its own `manifest_sha256`.
pub fn manifest_hash(manifest: &PreferenceManifestV1) -> String {
    let mut value = serde_json::to_value(manifest).expect("manifest serializes");
    if let Value::Object(map) = &mut value {
        map.remove("manifest_sha256");
    }
    sha256_canonical(&value)
}

fn validate_structure(manifest: &PreferenceManifestV1) -> Result<(), ManifestError> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(ManifestError::UnsupportedSchema(
            manifest.schema_version.clone(),
        ));
    }
    if manifest.batch_id.trim().is_empty() || manifest.installation_id.trim().is_empty() {
        return Err(ManifestError::MissingBatchIdentity);
    }
    if manifest.manifest_sha256 != manifest_hash(manifest) {
        return Err(ManifestError::PayloadMismatch {
            record_id: "<manifest>".into(),
        });
    }
    let mut seen_sources = std::collections::BTreeSet::new();
    for r in &manifest.source_refs {
        if !seen_sources.insert(r.source_id.clone()) {
            return Err(ManifestError::DuplicateSourceRef(r.source_id.clone()));
        }
    }
    let session_ids: Vec<String> = manifest
        .source_refs
        .iter()
        .map(|r| r.source_id.clone())
        .collect();
    if manifest.source_session_ids != session_ids {
        return Err(ManifestError::SourceSessionMismatch);
    }
    let hashes: std::collections::BTreeMap<&str, &str> = manifest
        .source_refs
        .iter()
        .map(|r| (r.source_id.as_str(), r.sha256.as_str()))
        .collect();
    let record_by_id: std::collections::BTreeMap<&str, &ManifestRecord> = manifest
        .records
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect();
    if record_by_id.len() != manifest.records.len() {
        return Err(ManifestError::SemanticValidation(
            "duplicate record id".into(),
        ));
    }
    for rec in &manifest.records {
        if rec.semantic_payload.is_some() || !rec.semantic_digest.is_empty() {
            verify_manifest_record_seal(rec, &manifest.canonical_pool_sha256)?;
        }
        let mut uniq = rec.source_ids.clone();
        uniq.sort();
        uniq.dedup();
        if uniq.len() != rec.source_ids.len() {
            return Err(ManifestError::UnknownSourceId {
                record_id: rec.id.clone(),
                source_id: "<duplicate>".into(),
            });
        }
        for sid in &rec.source_ids {
            let Some(h) = hashes.get(sid.as_str()) else {
                return Err(ManifestError::UnknownSourceId {
                    record_id: rec.id.clone(),
                    source_id: sid.clone(),
                });
            };
            let binding = rec
                .source_file_hashes
                .iter()
                .find(|b| b.session_id == *sid)
                .ok_or(ManifestError::UnknownSourceId {
                    record_id: rec.id.clone(),
                    source_id: sid.clone(),
                })?;
            if binding.sha256.to_lowercase() != h.trim_start_matches("sha256:") {
                return Err(ManifestError::PayloadMismatch {
                    record_id: rec.id.clone(),
                });
            }
        }
        for ctx in &rec.evidence_contexts {
            let sources: Vec<&ContextEvent> =
                ctx.context_events.iter().filter(|e| e.is_source).collect();
            if sources.len() != 1 {
                return Err(ManifestError::EvidenceContextInvalid {
                    record_id: rec.id.clone(),
                    reason: format!("expected exactly one source event, found {}", sources.len()),
                });
            }
            let s = sources[0];
            let transcript_user =
                s.provenance == "external_user" && s.kind == "user_message" && s.role == "user";
            if !transcript_user {
                return Err(ManifestError::EvidenceContextInvalid {
                    record_id: rec.id.clone(),
                    reason: "source event must be an eligible external user message".into(),
                });
            }
            if ctx.evidence_text != s.text
                || ctx.source_event_id != s.event_id
                || ctx.source_byte_start != s.byte_start
                || ctx.source_byte_end != s.byte_end
            {
                return Err(ManifestError::EvidenceContextInvalid {
                    record_id: rec.id.clone(),
                    reason: "context header disagrees with source event".into(),
                });
            }
        }
    }
    let mut group_digests = std::collections::BTreeSet::new();
    for group in &manifest.duplicate_groups {
        if group.contract_version != DUPLICATE_GROUP_CONTRACT
            || group.algorithm != DETERMINISTIC_EXACT_ALGORITHM
            || !group_digests.insert(group.group_digest.as_str())
        {
            return Err(ManifestError::SemanticValidation(
                "invalid or duplicate automatic duplicate group".into(),
            ));
        }
        let rebuilt = build_group(group.members.clone(), &group.algorithm).map_err(|error| {
            ManifestError::SemanticValidation(format!("invalid duplicate group: {error:?}"))
        })?;
        if rebuilt != *group {
            return Err(ManifestError::SemanticValidation(
                "duplicate group digest mismatch".into(),
            ));
        }
        let candidates = group
            .members
            .iter()
            .map(|member| {
                let record = record_by_id.get(member.record_id.as_str()).ok_or_else(|| {
                    ManifestError::SemanticValidation(format!(
                        "duplicate member missing: {}",
                        member.record_id
                    ))
                })?;
                if record.semantic_digest != member.semantic_seal {
                    return Err(ManifestError::SemanticValidation(format!(
                        "duplicate member seal mismatch: {}",
                        member.record_id
                    )));
                }
                let payload = record.semantic_payload.as_ref().ok_or_else(|| {
                    ManifestError::SemanticValidation(format!(
                        "duplicate member is not sealed: {}",
                        member.record_id
                    ))
                })?;
                Ok(DuplicateCandidateV1 {
                    record_id: record.id.clone(),
                    canonical_text: record.rule.clone(),
                    scope: record.scope.clone(),
                    semantic_seal: record.semantic_digest.clone(),
                    semantic_equivalence_digest: semantic_equivalence_digest(payload),
                    evidence_count: record.evidence_count,
                    existing_canonical: false,
                })
            })
            .collect::<Result<Vec<_>, ManifestError>>()?;
        let regrouped =
            crate::duplicate_groups::deterministic_exact_groups(&candidates).map_err(|error| {
                ManifestError::SemanticValidation(format!(
                    "invalid deterministic duplicate group: {error:?}"
                ))
            })?;
        if regrouped.as_slice() != std::slice::from_ref(group) {
            return Err(ManifestError::SemanticValidation(
                "duplicate group is not semantically equivalent".into(),
            ));
        }
        let expected = verify_reviewed_resolution(group, None, &std::collections::BTreeMap::new())
            .map_err(|error| {
                ManifestError::SemanticValidation(format!(
                    "invalid abstaining duplicate group: {error:?}"
                ))
            })?;
        let matches: Vec<_> = manifest
            .duplicate_resolutions
            .iter()
            .filter(|resolution| resolution.group_digest == group.group_digest)
            .collect();
        if matches.len() != 1 || matches.first().copied() != Some(&expected) {
            return Err(ManifestError::SemanticValidation(
                "duplicate resolution is absent or noncanonical".into(),
            ));
        }
    }
    if manifest.duplicate_resolutions.len() != manifest.duplicate_groups.len() {
        return Err(ManifestError::SemanticValidation(
            "orphan duplicate resolution".into(),
        ));
    }
    Ok(())
}

/// Schema-level validation: accepts pending manifests (freshly emitted).
pub fn validate_schema(manifest: &PreferenceManifestV1) -> Result<(), ManifestError> {
    validate_structure(manifest)?;
    for rec in &manifest.records {
        if rec.payload_sha256 != payload_sha256(rec) {
            return Err(ManifestError::PayloadMismatch {
                record_id: rec.id.clone(),
            });
        }
    }
    Ok(())
}

fn semantic_validation_errors(manifest: &PreferenceManifestV1) -> Result<(), ManifestError> {
    if manifest.records.is_empty() {
        return Ok(());
    }
    let Some(receipt) = &manifest.semantic_validation else {
        return Err(ManifestError::SemanticValidation(
            "receipt is missing".into(),
        ));
    };
    let mut errors = Vec::new();
    if receipt.contract != SEMANTIC_VALIDATION_CONTRACT {
        errors.push("unsupported contract".into());
    }
    if !receipt.complete || !receipt.independent {
        errors.push("validation must be complete and independent".into());
    }
    if receipt.canonical_pool_sha256 != manifest.canonical_pool_sha256 {
        errors.push("canonical pool does not match manifest".into());
    }
    let mut payload_for_receipt = serde_json::to_value(receipt).unwrap();
    if let Value::Object(map) = &mut payload_for_receipt {
        map.remove("receipt_sha256");
    }
    if sha256_canonical(&payload_for_receipt) != receipt.receipt_sha256 {
        errors.push("receipt hash mismatch".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for result in &receipt.record_results {
        if !seen.insert(result.id.clone()) {
            errors.push(format!("duplicate coverage for {}", result.id));
        }
    }
    let record_ids: std::collections::BTreeSet<&str> =
        manifest.records.iter().map(|r| r.id.as_str()).collect();
    let covered: std::collections::BTreeSet<&str> = receipt
        .record_results
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    if covered != record_ids {
        errors.push("coverage does not exactly match manifest records".into());
    }
    let by_id: std::collections::BTreeMap<&str, &SemanticRecordResult> = receipt
        .record_results
        .iter()
        .map(|r| (r.id.as_str(), r))
        .collect();
    for rec in &manifest.records {
        let Some(result) = by_id.get(rec.id.as_str()) else {
            continue;
        };
        if result.payload_sha256 != rec.payload_sha256 {
            errors.push(format!("payload mismatch: {}", rec.id));
        }
        if result.status != rec.status {
            errors.push(format!("status mismatch: {}", rec.id));
        }
        if rec.status == "accepted" && result.verdict != "valid" {
            errors.push(format!("accepted record lacks valid verdict: {}", rec.id));
        }
        if rec.status == "rejected" && result.verdict != "invalid" {
            errors.push(format!("rejected record lacks invalid verdict: {}", rec.id));
        }
        if rec.status == "accepted"
            && (rec.verification_count < 1 || rec.last_verified_at.trim().is_empty())
        {
            errors.push(format!(
                "accepted record lacks verification stamp: {}",
                rec.id
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ManifestError::SemanticValidation(errors.join("; ")))
    }
}

/// Apply-time validation: refuses pending records, edited payloads, and
/// incomplete/inconsistent semantic validation. Zero-record manifests are
/// valid committed no-ops.
pub fn apply_time_validate(manifest: &PreferenceManifestV1) -> Result<(), ManifestError> {
    validate_schema(manifest)?;
    for rec in &manifest.records {
        if rec.payload_sha256 != payload_sha256(rec) {
            return Err(ManifestError::PayloadMismatch {
                record_id: rec.id.clone(),
            });
        }
        if rec.status == "pending" {
            return Err(ManifestError::PendingAtApplyTime {
                record_id: rec.id.clone(),
            });
        }
        if rec.status == "accepted" {
            verify_manifest_record_seal(rec, &manifest.canonical_pool_sha256)?;
        }
    }
    semantic_validation_errors(manifest)
}

/// Deterministic apply plan: accepted records sorted by ID; rejected dropped.
/// Applying the same manifest twice yields the same plan (idempotence), and a
/// zero-record manifest commits as an explicit no-op batch.
pub fn apply_plan(manifest: &PreferenceManifestV1) -> Result<Vec<String>, ManifestError> {
    apply_time_validate(manifest)?;
    let mut ids: Vec<String> = manifest
        .records
        .iter()
        .filter(|r| r.status == "accepted")
        .map(|r| r.id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}
