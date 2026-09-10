//! Native port of `blueprint/src/graph/conformance-verifier.mjs`.
//!
//! Deterministic, reviewable semantic conformance verifier. Assertions
//! deliberately describe public fact meaning (node/edge + semantic fields)
//! rather than storage rows or provider-private serialization, so positive,
//! negative and ambiguity cases survive storage/index refactors.
//!
//! This module operates on plain `serde_json::Value` node/edge records so it
//! stays independent of any one generation representation: callers convert
//! their native `GraphNode`/`GraphEdge` (or any other fact shape) to JSON via
//! `serde_json::to_value` before calling [`verify_semantic_conformance`].

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One provider layer's identity, as surfaced in a conformance report
/// (`providerVersions` in the legacy module).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderVersion {
    pub id: Option<String>,
    pub version: Option<String>,
    pub precision_tier: Option<String>,
    pub state: Option<String>,
}

/// The generation under test: bounded node/edge facts plus the identity
/// fields a report echoes back.
#[derive(Debug, Clone, Default)]
pub struct ConformanceGeneration {
    pub nodes: Vec<Value>,
    pub edges: Vec<Value>,
    pub generation_id: Option<String>,
    pub schema_version: Option<Value>,
    pub providers: Vec<ProviderVersion>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssertionCount {
    pub exactly: Option<u64>,
    #[serde(rename = "atLeast")]
    pub at_least: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConformanceAssertion {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub r#where: serde_json::Map<String, Value>,
    #[serde(default)]
    pub count: Option<AssertionCount>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConformanceFixture {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub assertions: Vec<ConformanceAssertion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssertionResult {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub description: Option<String>,
    pub passed: bool,
    pub matched: usize,
    #[serde(rename = "sampleIds")]
    pub sample_ids: Vec<Option<String>>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConformanceReport {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub kind: &'static str,
    pub fixture: String,
    pub status: &'static str,
    #[serde(rename = "generationId")]
    pub generation_id: Option<String>,
    #[serde(rename = "generationSchemaVersion")]
    pub generation_schema_version: Option<Value>,
    pub providers: Vec<ProviderVersion>,
    pub assertions: Vec<AssertionResult>,
    pub failures: Vec<String>,
}

impl ConformanceReport {
    pub fn passed(&self) -> bool {
        self.status == "passed"
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConformanceError {
    #[error("semantic conformance assertion {index} is missing id")]
    MissingId { index: usize },
    #[error("unknown semantic conformance assertion type: {0}")]
    UnknownType(String),
}

/// Resolve a dot-path against a JSON value, honoring both object keys and
/// array indices (mirrors the legacy `getPath`, which relies on JS arrays
/// being indexable by numeric-string keys). Returns `None` when the path is
/// absent -- distinct from a present JSON `null` -- matching JS
/// `undefined` vs `null`.
fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = match current {
            Value::Object(map) => map.get(segment)?,
            Value::Array(items) => {
                let index: usize = segment.parse().ok()?;
                items.get(index)?
            }
            _ => return None,
        };
    }
    Some(current)
}

/// `String(x)` for the subset of JSON values this verifier's `$matches`
/// operator needs (legacy: `String(actual ?? "")`).
fn js_string(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn matches_expected(actual: Option<&Value>, expected: &Value) -> bool {
    if let Value::Object(map) = expected {
        if let Some(exists) = map.get("$exists") {
            let is_present = matches!(actual, Some(v) if !v.is_null());
            return exists.as_bool().unwrap_or(false) == is_present;
        }
        if let Some(needle) = map.get("$includes") {
            return matches!(actual, Some(Value::Array(items)) if items.contains(needle));
        }
        if let Some(Value::Array(all)) = map.get("$containsAll") {
            return matches!(actual, Some(Value::Array(items)) if all.iter().all(|item| items.contains(item)));
        }
        if let Some(pattern) = map.get("$matches").and_then(Value::as_str) {
            return regex::Regex::new(pattern)
                .map(|re| re.is_match(&js_string(actual)))
                .unwrap_or(false);
        }
    }
    // Plain equality: `Object.is(actual, expected)`. A missing path (`None`)
    // never equals a present JSON `null`, exactly as `undefined !== null`.
    matches!(actual, Some(v) if v == expected)
}

fn matches_where(record: &Value, r#where: &serde_json::Map<String, Value>) -> bool {
    r#where
        .iter()
        .all(|(path, expected)| matches_expected(get_path(record, path), expected))
}

fn assertion_source<'a>(generation: &'a ConformanceGeneration, kind: &str) -> Result<&'a [Value], ConformanceError> {
    if kind.contains("node") {
        Ok(&generation.nodes)
    } else if kind.contains("edge") {
        Ok(&generation.edges)
    } else {
        Err(ConformanceError::UnknownType(kind.to_string()))
    }
}

fn count_pass(matches: usize, assertion: &ConformanceAssertion) -> bool {
    if assertion.kind.ends_with("_absent") {
        return matches == 0;
    }
    if let Some(count) = &assertion.count {
        if let Some(exactly) = count.exactly {
            return matches as u64 == exactly;
        }
        if let Some(at_least) = count.at_least {
            return matches as u64 >= at_least;
        }
    }
    matches >= 1
}

fn explain(assertion: &ConformanceAssertion, matches: usize, passed: bool) -> Option<String> {
    if passed {
        return None;
    }
    let expectation = if assertion.kind.ends_with("_absent") {
        "no matching facts".to_string()
    } else if let Some(exactly) = assertion.count.as_ref().and_then(|c| c.exactly) {
        format!("exactly {exactly} matching fact(s)")
    } else {
        let at_least = assertion.count.as_ref().and_then(|c| c.at_least).unwrap_or(1);
        format!("at least {at_least} matching fact(s)")
    };
    let where_json = serde_json::to_string(&assertion.r#where).unwrap_or_else(|_| "{}".to_string());
    Some(format!(
        "{}: expected {}, observed {}; where={}",
        assertion.id, expectation, matches, where_json
    ))
}

fn record_id(record: &Value) -> Option<String> {
    record.get("id").and_then(Value::as_str).map(str::to_owned)
}

/// Evaluate every assertion in `fixture` against `generation`, returning a
/// full report (never throwing on a failed assertion -- only on a malformed
/// fixture, mirroring `verifySemanticConformance`).
pub fn verify_semantic_conformance(
    generation: &ConformanceGeneration,
    fixture: &ConformanceFixture,
) -> Result<ConformanceReport, ConformanceError> {
    let mut results = Vec::with_capacity(fixture.assertions.len());
    for (index, assertion) in fixture.assertions.iter().enumerate() {
        if assertion.id.is_empty() {
            return Err(ConformanceError::MissingId { index });
        }
        let source = assertion_source(generation, &assertion.kind)?;
        let matches: Vec<&Value> = source
            .iter()
            .filter(|record| matches_where(record, &assertion.r#where))
            .collect();
        let passed = count_pass(matches.len(), assertion);
        results.push(AssertionResult {
            id: assertion.id.clone(),
            kind: assertion.kind.clone(),
            description: assertion.description.clone(),
            passed,
            matched: matches.len(),
            sample_ids: matches.iter().take(5).map(|record| record_id(record)).collect(),
            failure: explain(assertion, matches.len(), passed),
        });
    }
    let failures: Vec<String> = results
        .iter()
        .filter(|result| !result.passed)
        .filter_map(|result| result.failure.clone())
        .collect();
    let status = if failures.is_empty() { "passed" } else { "failed" };
    Ok(ConformanceReport {
        schema_version: 1,
        kind: "BlueprintSemanticConformanceReport",
        fixture: fixture.name.clone().unwrap_or_else(|| "unnamed".to_string()),
        status,
        generation_id: generation.generation_id.clone(),
        generation_schema_version: generation.schema_version.clone(),
        providers: generation.providers.clone(),
        assertions: results,
        failures,
    })
}

#[derive(Debug, thiserror::Error)]
#[error("semantic conformance failed for {fixture}:\n{}", failures.join("\n"))]
pub struct SemanticConformanceFailed {
    pub fixture: String,
    pub failures: Vec<String>,
    pub report: ConformanceReport,
}

impl SemanticConformanceFailed {
    pub const CODE: &'static str = "semantic_conformance_failed";
}

/// Verify, then fail closed (typed error carrying the full report) unless
/// every assertion passed -- mirrors `assertSemanticConformance`.
pub fn assert_semantic_conformance(
    generation: &ConformanceGeneration,
    fixture: &ConformanceFixture,
) -> Result<ConformanceReport, SemanticConformanceFailed> {
    let report = match verify_semantic_conformance(generation, fixture) {
        Ok(report) => report,
        Err(error) => {
            return Err(SemanticConformanceFailed {
                fixture: fixture.name.clone().unwrap_or_else(|| "unnamed".to_string()),
                failures: vec![error.to_string()],
                report: ConformanceReport {
                    schema_version: 1,
                    kind: "BlueprintSemanticConformanceReport",
                    fixture: fixture.name.clone().unwrap_or_else(|| "unnamed".to_string()),
                    status: "failed",
                    generation_id: generation.generation_id.clone(),
                    generation_schema_version: generation.schema_version.clone(),
                    providers: generation.providers.clone(),
                    assertions: Vec::new(),
                    failures: vec![error.to_string()],
                },
            });
        }
    };
    if report.status == "failed" {
        return Err(SemanticConformanceFailed {
            fixture: report.fixture.clone(),
            failures: report.failures.clone(),
            report,
        });
    }
    Ok(report)
}
