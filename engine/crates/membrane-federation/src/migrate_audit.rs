//! Read-only compatibility reader for serialized audit finding records.
//!
//! The reader accepts the native `AuditFinding` shape and the prior flat
//! candidate record shape.  It never writes, imports code, or treats its path
//! as repository identity.

use membrane_protocol::CandidateV1;
use membrane_provider_sdk::{AuditFinding, SourceResponse, SourceWarning};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_AUDIT_RECORD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditMigrationError {
    Unavailable,
    Oversized,
    InvalidJson,
    Malformed(String),
}
impl std::fmt::Display for AuditMigrationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("audit_records_unavailable"),
            Self::Oversized => formatter.write_str("audit_records_oversized"),
            Self::InvalidJson => formatter.write_str("audit_records_invalid_json"),
            Self::Malformed(detail) => write!(formatter, "audit_records_malformed:{detail}"),
        }
    }
}

impl std::error::Error for AuditMigrationError {}

#[derive(Debug, Clone)]
pub struct AuditFindingReader {
    path: PathBuf,
}

impl AuditFindingReader {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read records without changing the source.  Repeated calls are
    /// idempotent because no migration marker or cache is persisted.
    pub fn read(&self) -> Result<SourceResponse<Vec<AuditFinding>>, AuditMigrationError> {
        let metadata = fs::metadata(&self.path).map_err(|_| AuditMigrationError::Unavailable)?;
        if metadata.len() > MAX_AUDIT_RECORD_BYTES as u64 {
            return Err(AuditMigrationError::Oversized);
        }
        let bytes = fs::read(&self.path).map_err(|_| AuditMigrationError::Unavailable)?;
        if bytes.len() > MAX_AUDIT_RECORD_BYTES {
            return Err(AuditMigrationError::Oversized);
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| AuditMigrationError::InvalidJson)?;
        decode_document(value)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FindingDocument {
    #[serde(default)]
    generation: Option<String>,
    #[serde(default)]
    complete: Option<bool>,
    #[serde(default)]
    warnings: Vec<SourceWarning>,
    #[serde(default)]
    findings: Vec<serde_json::Value>,
    #[serde(default)]
    records: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FlatFinding {
    id: String,
    repository_id: String,
    generation: String,
    source_hash: String,
    #[serde(default)]
    candidate: Option<CandidateV1>,
    #[serde(default)]
    layer: Option<u8>,
    #[serde(default)]
    source_kind: Option<String>,
    #[serde(default)]
    source_ref: Option<String>,
    #[serde(default)]
    trust_class: Option<String>,
    #[serde(default)]
    instruction_policy: Option<String>,
    #[serde(default)]
    provider_score: Option<f64>,
    #[serde(default)]
    score_components: BTreeMap<String, f64>,
    #[serde(default)]
    estimated_tokens: Option<u32>,
    #[serde(default)]
    protected: Option<bool>,
    #[serde(default)]
    exact: Option<bool>,
    #[serde(default)]
    recoverable: Option<bool>,
    #[serde(default)]
    resolver: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

fn decode_document(value: serde_json::Value) -> Result<SourceResponse<Vec<AuditFinding>>, AuditMigrationError> {
    if let Ok(finding) = serde_json::from_value::<AuditFinding>(value.clone()) {
        return Ok(SourceResponse { generation: Some(finding.generation.clone()), complete: true, warnings: Vec::new(), value: vec![finding] });
    }
    let (generation, complete, warnings, records) = if let Some(values) = value.as_array() {
        (None, true, Vec::new(), values.clone())
    } else {
        let document: FindingDocument = serde_json::from_value(value)
            .map_err(|error| AuditMigrationError::Malformed(error.to_string()))?;
        let records = if document.findings.is_empty() { document.records } else { document.findings };
        (document.generation, document.complete.unwrap_or(true), document.warnings, records)
    };
    let mut findings = Vec::with_capacity(records.len());
    for record in records {
        findings.push(decode_finding(record)?);
    }
    let generation = generation.or_else(|| {
        findings
            .first()
            .map(|finding: &AuditFinding| finding.generation.clone())
    });
    if findings.iter().any(|finding| generation.as_deref() != Some(finding.generation.as_str())) {
        return Err(AuditMigrationError::Malformed("mixed_generations".into()));
    }
    Ok(SourceResponse { value: findings, generation, complete, warnings })
}

fn decode_finding(value: serde_json::Value) -> Result<AuditFinding, AuditMigrationError> {
    if let Ok(finding) = serde_json::from_value::<AuditFinding>(value.clone()) {
        return Ok(finding);
    }
    let flat: FlatFinding = serde_json::from_value(value)
        .map_err(|error| AuditMigrationError::Malformed(error.to_string()))?;
    let candidate = flat.candidate.unwrap_or_else(|| CandidateV1 {
        id: flat.id.clone(),
        layer: flat.layer.unwrap_or(4),
        provider: None,
        source_kind: flat.source_kind.clone().unwrap_or_else(|| "audit_finding".into()),
        source_ref: flat.source_ref.clone().unwrap_or_else(|| flat.id.clone()),
        source_hash: flat.source_hash.clone(),
        trust_class: flat.trust_class.clone().unwrap_or_else(|| "agent_verified".into()),
        instruction_policy: flat.instruction_policy.clone().unwrap_or_else(|| "data_only".into()),
        provider_score: flat.provider_score.unwrap_or(0.5),
        score_components: flat.score_components.clone(),
        base_commit: None,
        overlay_digest: None,
        freshness_class: None,
        snapshot_id: None,
        estimated_tokens: flat.estimated_tokens.unwrap_or(80),
        protected: flat.protected.unwrap_or(false),
        exact: flat.exact.unwrap_or(true),
        recoverable: flat.recoverable.unwrap_or(true),
        resolver: flat.resolver.clone().unwrap_or_default(),
        text: flat.text.clone().unwrap_or_else(|| flat.id.clone()),
    });
    Ok(AuditFinding {
        id: flat.id,
        repository_id: flat.repository_id,
        generation: flat.generation,
        source_hash: flat.source_hash,
        candidate,
    })
}
