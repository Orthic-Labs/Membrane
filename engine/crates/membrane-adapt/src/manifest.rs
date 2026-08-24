//! Deterministic preference manifest build, validation, and idempotent
//! deterministic apply (Gate-1a contract ported and hardened).
//!
//! The manifest is the only path from mined candidates to durable apply. The
//! loader refuses: unknown schema versions, records whose `payload_sha256`
//! mismatches content, any `pending` record at apply time, missing batch
//! identity, evidence contexts without exactly one authority-eligible
/// external-user source event, and semantic-validation receipts that do not
/// exactly cover the manifest's records.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::canonical::{canonical_object, sha256_canonical};

pub const MANIFEST_SCHEMA_VERSION: &str = "1.3.0";
pub const SEMANTIC_VALIDATION_CONTRACT: &str = "direct-evidence-global-pool-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    UnsupportedSchema(String),
    PayloadMismatch { record_id: String },
    PendingAtApplyTime { record_id: String },
    MissingBatchIdentity,
    DuplicateSourceRef(String),
    SourceSessionMismatch,
    UnknownSourceId { record_id: String, source_id: String },
    EvidenceContextInvalid { record_id: String, reason: String },
    SemanticValidation(String),
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
            ManifestError::UnknownSourceId { record_id, source_id } => {
                write!(f, "record {record_id} references unknown source {source_id}")
            }
            ManifestError::EvidenceContextInvalid { record_id, reason } => {
                write!(f, "record {record_id} evidence context invalid: {reason}")
            }
            ManifestError::SemanticValidation(msg) => write!(f, "semantic validation: {msg}"),
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
        return Err(ManifestError::UnsupportedSchema(manifest.schema_version.clone()));
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
    let session_ids: Vec<String> = manifest.source_refs.iter().map(|r| r.source_id.clone()).collect();
    if manifest.source_session_ids != session_ids {
        return Err(ManifestError::SourceSessionMismatch);
    }
    let hashes: std::collections::BTreeMap<&str, &str> = manifest
        .source_refs
        .iter()
        .map(|r| (r.source_id.as_str(), r.sha256.as_str()))
        .collect();
    for rec in &manifest.records {
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
            if s.provenance != "external_user"
                || s.kind != "user_message"
                || s.role != "user"
            {
                return Err(ManifestError::EvidenceContextInvalid {
                    record_id: rec.id.clone(),
                    reason: "source event must be an external user message".into(),
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

fn semantic_validation_errors(
    manifest: &PreferenceManifestV1,
) -> Result<(), ManifestError> {
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
    let covered: std::collections::BTreeSet<&str> =
        receipt.record_results.iter().map(|r| r.id.as_str()).collect();
    if covered != record_ids {
        errors.push("coverage does not exactly match manifest records".into());
    }
    let by_id: std::collections::BTreeMap<&str, &SemanticRecordResult> = receipt
        .record_results
        .iter()
        .map(|r| (r.id.as_str(), r))
        .collect();
    for rec in &manifest.records {
        let Some(result) = by_id.get(rec.id.as_str()) else { continue };
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
