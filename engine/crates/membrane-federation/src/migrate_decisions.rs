//! Read-only compatibility reader for Architect's legacy decision JSONL.
//!
//! The reader accepts the historical append-only format, reports malformed
//! lines with line numbers, and never rewrites or imports Python state.  Its
//! output is a typed source projection suitable for the native architect lane.

use membrane_protocol::{canonical_json_of, digest_str};
use membrane_provider_sdk::{DecisionRecord, DecisionRecordSource, SourceQuery, SourceResponse, SourceResult, SourceWarning};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::future::Future;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;

pub const STORE_RELATIVE_PATH: &str = ".audit/architect/decisions.jsonl";
const REVISIT_MARKER: &str = "__membrane_revisit_trigger__:";
const PROVENANCE_MARKER: &str = "__membrane_provenance__:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionLineDiagnostic {
    pub line: usize,
    pub code: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionEvidence {
    pub id: String,
    pub repository_id: String,
    pub scope_id: Option<String>,
    pub lifecycle: Option<String>,
    pub mode: Option<String>,
    pub provenance: Vec<String>,
    pub revisit_triggers: Vec<String>,
}

/// Exact source-side match dimensions.  Empty optional dimensions are not
/// guessed: callers that need lifecycle/mode matching must provide them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecisionMatch {
    pub repository_id: String,
    pub scope_id: String,
    pub linked_generation: Option<String>,
    pub lifecycle: Option<String>,
    pub mode: Option<String>,
}

impl DecisionMatch {
    pub fn new(repository_id: impl Into<String>, scope_id: impl Into<String>) -> Self {
        Self { repository_id: repository_id.into(), scope_id: scope_id.into(), ..Self::default() }
    }
    pub fn generation(mut self, value: impl Into<String>) -> Self { self.linked_generation = Some(value.into()); self }
    pub fn lifecycle(mut self, value: impl Into<String>) -> Self { self.lifecycle = Some(value.into()); self }
    pub fn mode(mut self, value: impl Into<String>) -> Self { self.mode = Some(value.into()); self }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionReadReport {
    pub records: Vec<DecisionRecord>,
    pub evidence: Vec<DecisionEvidence>,
    pub diagnostics: Vec<DecisionLineDiagnostic>,
    pub source_hash: Option<String>,
    pub complete: bool,
}

#[derive(Debug)]
pub enum DecisionReadError {
    Io(io::Error),
}

impl std::fmt::Display for DecisionReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::Io(error) => write!(formatter, "decision JSONL read failed: {error}") }
    }
}

impl std::error::Error for DecisionReadError {}

#[derive(Clone, Copy, Debug, Default)]
pub struct DecisionJsonlReader;

impl DecisionJsonlReader {
    pub const fn new() -> Self { Self }

    pub fn read<P: AsRef<Path>>(&self, path: P) -> Result<DecisionReadReport, DecisionReadError> {
        read_decisions(path)
    }

    pub fn read_matching<P: AsRef<Path>>(&self, path: P, matching: &DecisionMatch) -> Result<DecisionReadReport, DecisionReadError> {
        Ok(filter_report(read_decisions(path)?, matching))
    }
}

/// Typed, read-only source backed by the legacy JSONL file.  It exists only
/// as a compatibility adapter; source contents are never written or loaded
/// as executable code.
#[derive(Clone, Debug)]
pub struct JsonlDecisionSource {
    path: PathBuf,
    matching: Option<DecisionMatch>,
}

impl JsonlDecisionSource {
    pub fn new(path: impl Into<PathBuf>) -> Self { Self { path: path.into(), matching: None } }

    pub fn with_matching(path: impl Into<PathBuf>, matching: DecisionMatch) -> Self {
        Self { path: path.into(), matching: Some(matching) }
    }

    pub fn path(&self) -> &Path { &self.path }
}

impl DecisionRecordSource for JsonlDecisionSource {
    fn decisions<'life0, 'life1, 'async_trait>(
        &'life0 self,
        query: &'life1 SourceQuery,
    ) -> Pin<Box<dyn Future<Output = SourceResult<Vec<DecisionRecord>>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let repository_id = query.repository_id.clone();
        let expected_generation = query.generation.clone();
        let path = self.path.clone();
        Box::pin(async move {
            let mut report = read_decisions(path).map_err(|error| {
                membrane_provider_sdk::ProviderError::Unavailable(error.to_string())
            })?;
            let mut warnings = Vec::new();
            for diagnostic in &report.diagnostics {
                warnings.push(SourceWarning {
                    code: if diagnostic.code == "source_unavailable" { "provider_unavailable".to_owned() } else { "provider_malformed".to_owned() },
                    detail_id: Some(format!("{}:line-{}", diagnostic.code, diagnostic.line)),
                });
            }
            let matching = self.matching.clone().unwrap_or_else(|| DecisionMatch {
                repository_id: repository_id.clone(),
                scope_id: repository_id.clone(),
                linked_generation: expected_generation.clone(),
                // Legacy source has no mode on SourceQuery.  A configured
                // source can still demand one with `with_matching`.
                lifecycle: None,
                mode: None,
            });
            if matching.repository_id != repository_id
                || (expected_generation.is_some() && matching.linked_generation != expected_generation)
            {
                return Ok(SourceResponse { value: Vec::new(), generation: expected_generation, complete: true, warnings });
            }
            if matching.repository_id.is_empty() { return Ok(SourceResponse { value: Vec::new(), generation: None, complete: report.complete, warnings }); }
            report = filter_report(report, &matching);
            let generation = report.records.first().map(|record| record.generation.clone());
            let complete = report.complete;
            let evidence = std::mem::take(&mut report.evidence);
            let records = std::mem::take(&mut report.records).into_iter().map(|mut record| {
                if let Some(evidence) = evidence.iter().find(|evidence| evidence.id == record.id) {
                    for trigger in &evidence.revisit_triggers {
                        let marker = format!("{REVISIT_MARKER}{trigger}");
                        if !record.risks.iter().any(|risk| risk == &marker) { record.risks.push(marker); }
                    }
                    for provenance in &evidence.provenance {
                        let marker = format!("{PROVENANCE_MARKER}{provenance}");
                        if !record.risks.iter().any(|risk| risk == &marker) { record.risks.push(marker); }
                    }
                }
                record
            }).collect();
            Ok(SourceResponse { value: records, generation, complete, warnings })
        })
    }
}

pub fn read_decisions<P: AsRef<Path>>(path: P) -> Result<DecisionReadReport, DecisionReadError> {
    let path = path.as_ref();
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(DecisionReadReport {
                records: Vec::new(), evidence: Vec::new(),
                diagnostics: vec![diagnostic(0, "source_unavailable")],
                source_hash: None, complete: false,
            });
        }
        Err(error) => return Err(DecisionReadError::Io(error)),
    };
    let source_hash = Some(digest_str(&String::from_utf8_lossy(&bytes)));
    let mut report = DecisionReadReport {
        records: Vec::new(), evidence: Vec::new(), diagnostics: Vec::new(),
        source_hash, complete: true,
    };
    for (index, raw) in String::from_utf8_lossy(&bytes).lines().enumerate() {
        let line = index + 1;
        if raw.trim().is_empty() { continue; }
        let value: Value = match serde_json::from_str(raw) {
            Ok(value) => value,
            Err(_) => { report.diagnostics.push(diagnostic(line, "invalid_json")); continue; }
        };
        let Some(object) = value.as_object() else {
            report.diagnostics.push(diagnostic(line, "record_not_object"));
            continue;
        };
        if object.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
            report.diagnostics.push(diagnostic(line, "unsupported_schema_version"));
            continue;
        }
        let Some(id) = string_field(object, &["id", "decisionId"]) else {
            report.diagnostics.push(diagnostic(line, "missing_id"));
            continue;
        };
        let Some(repository_id) = string_field(object, &["repositoryId", "repository_id"]) else {
            report.diagnostics.push(diagnostic(line, "missing_repository_id"));
            continue;
        };
        let Some(generation) = string_field(object, &["linkedGraphGeneration", "linked_generation", "generation"]) else {
            report.diagnostics.push(diagnostic(line, "missing_linked_generation"));
            continue;
        };
        let Some(rationale) = string_field(object, &["rationale", "reason"]) else {
            report.diagnostics.push(diagnostic(line, "missing_rationale"));
            continue;
        };
        let alternatives = match string_array(object, &["alternatives"]) {
            Ok(values) => values,
            Err(code) => { report.diagnostics.push(diagnostic(line, code)); continue; }
        };
        let risks = match string_array(object, &["risks", "residualRisks"]) {
            Ok(values) => values,
            Err(code) => { report.diagnostics.push(diagnostic(line, code)); continue; }
        };
        let source_hash = string_field(object, &["sourceHash", "source_hash"])
            .unwrap_or_else(|| stable_record_hash(object));
        let record = DecisionRecord {
            id: id.clone(), repository_id, generation,
            source_hash, rationale,
            alternatives, risks,
        };
        let evidence = DecisionEvidence {
            id,
            repository_id: record.repository_id.clone(),
            scope_id: string_field(object, &["scopeId", "scope_id"]),
            lifecycle: string_field(object, &["currentStatus", "lifecycle", "status"]),
            mode: string_field(object, &["mode", "decisionMode"]),
            provenance: provenance_values(object),
            revisit_triggers: string_array(object, &["revisitTriggers", "reviewTriggers", "review_triggers"])
                .unwrap_or_default(),
        };
        report.records.push(record);
        report.evidence.push(evidence);
    }
    report.complete = report.diagnostics.is_empty();
    Ok(report)
}

fn diagnostic(line: usize, code: &str) -> DecisionLineDiagnostic {
    DecisionLineDiagnostic { line, code: code.to_owned() }
}

fn string_field(object: &Map<String, Value>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| object.get(*name).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned))
}

fn string_array(object: &Map<String, Value>, names: &[&str]) -> Result<Vec<String>, &'static str> {
    let Some(value) = names.iter().find_map(|name| object.get(*name)) else { return Ok(Vec::new()); };
    let Some(array) = value.as_array() else { return Err("array_field_invalid"); };
    let mut values = Vec::with_capacity(array.len());
    for entry in array {
        let Some(value) = entry.as_str().map(str::trim).filter(|value| !value.is_empty()) else { return Err("array_item_invalid"); };
        values.push(value.to_owned());
    }
    Ok(values)
}

fn provenance_values(object: &Map<String, Value>) -> Vec<String> {
    let Some(value) = object.get("provenance") else {
        return string_array(object, &["evidence", "evidenceRefs"]).unwrap_or_default();
    };
    if let Some(values) = value.as_array() {
        return values.iter().filter_map(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned).collect();
    }
    value.as_object().map(|values| values.iter().map(|(key, value)| {
        value.as_str().map(|value| format!("{key}={value}")).unwrap_or_else(|| format!("{key}={value}"))
    }).collect()).unwrap_or_default()
}

fn stable_record_hash(object: &Map<String, Value>) -> String {
    let mut selected = BTreeMap::new();
    for key in [
        "id", "repositoryId", "scopeId", "taskId", "linkedGraphGeneration",
        "rationale", "alternatives", "evidence", "implementationRefs", "supersedes",
        "supersededBy", "currentStatus",
    ] {
        if let Some(value) = object.get(key) { selected.insert(key.to_owned(), value.clone()); }
    }
    digest_str(&canonical_json_of(&selected))
}

fn filter_report(mut report: DecisionReadReport, matching: &DecisionMatch) -> DecisionReadReport {
    let mut records = Vec::new();
    let mut evidence = Vec::new();
    for (record, detail) in report.records.into_iter().zip(report.evidence.into_iter()) {
        if record.repository_id != matching.repository_id || detail.repository_id != matching.repository_id { continue; }
        if !matching.scope_id.is_empty() && detail.scope_id.as_deref() != Some(matching.scope_id.as_str()) { continue; }
        if let Some(generation) = matching.linked_generation.as_deref() {
            if record.generation != generation { continue; }
        }
        if let Some(lifecycle) = matching.lifecycle.as_deref() {
            if detail.lifecycle.as_deref() != Some(lifecycle) { continue; }
        } else if matches!(detail.lifecycle.as_deref(), Some("superseded")) {
            continue;
        }
        if let Some(mode) = matching.mode.as_deref() {
            if detail.mode.as_deref() != Some(mode) { continue; }
        }
        records.push(record);
        evidence.push(detail);
    }
    report.records = records;
    report.evidence = evidence;
    report
}
