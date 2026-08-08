//! Typed, read-only projection for the Hub's source explorer.
//!
//! This view exposes source evidence only. In particular, process existence is
//! never promoted to readiness or liveness; readiness must be supplied by an
//! authoritative provider observation.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SOURCES_EXPLORER_SCHEMA_VERSION: &str = "sources-explorer.v1";
pub const MAX_PATHS: usize = 64;
pub const MAX_NEIGHBORHOODS: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourcesExplorerV1 {
    pub schema_version: String,
    pub repository: RepositoryIdentityV1,
    pub generation: GenerationV1,
    pub clocks: ClocksV1,
    pub readiness: ReadinessV1,
    pub parser: ParserCapabilityV1,
    pub recent_contribution: ContributionV1,
    pub paths: Vec<PathProjectionV1>,
    pub neighborhoods: Vec<NeighborhoodV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryIdentityV1 {
    pub id: String,
    pub origin: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerationV1 {
    pub id: String,
    pub release: String,
    pub index: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClocksV1 {
    pub observed_at_unix_ms: Option<u64>,
    pub contribution_at_unix_ms: Option<u64>,
    pub source_age_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessV1 {
    pub state: String,
    pub reason: String,
    pub authoritative: bool,
    pub observed_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ParserCapabilityV1 {
    pub name: String,
    pub version: String,
    pub languages: Vec<String>,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContributionV1 {
    pub id: String,
    pub summary: String,
    pub paths: Vec<String>,
    pub observed_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PathProjectionV1 {
    pub path: String,
    pub kind: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NeighborhoodV1 {
    pub path: String,
    pub relation: String,
    pub neighbors: Vec<String>,
}

fn text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned()
}
fn clock(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}
fn list(value: Option<&Value>, limit: usize) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .map(|v| v.iter().take(limit).cloned().collect())
        .unwrap_or_default()
}

/// Project an untrusted operation payload into a bounded, typed explorer view.
/// Missing fields remain explicit `unknown`; no health or liveness is inferred.
pub fn project_sources_explorer(payload: &Value) -> SourcesExplorerV1 {
    let repository = payload.get("repository").unwrap_or(payload);
    let generation = payload.get("generation").unwrap_or(payload);
    let clocks = payload.get("clocks").unwrap_or(payload);
    let readiness = payload
        .get("readiness")
        .unwrap_or(payload.get("providerReadiness").unwrap_or(payload));
    let parser = payload.get("parser").unwrap_or(payload);
    let contribution = payload
        .get("recentContribution")
        .unwrap_or(payload.get("contribution").unwrap_or(payload));
    let paths = list(payload.get("paths"), MAX_PATHS)
        .into_iter()
        .map(|item| PathProjectionV1 {
            path: text(item.get("path").or_else(|| item.get("sourceRef"))),
            kind: text(item.get("kind").or_else(|| item.get("sourceKind"))),
            evidence: text(item.get("evidence")),
        })
        .collect();
    let neighborhoods = list(payload.get("neighborhoods"), MAX_NEIGHBORHOODS)
        .into_iter()
        .map(|item| NeighborhoodV1 {
            path: text(item.get("path")),
            relation: text(item.get("relation")),
            neighbors: item
                .get("neighbors")
                .and_then(Value::as_array)
                .map(|v| {
                    v.iter()
                        .take(MAX_PATHS)
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect();
    SourcesExplorerV1 {
        schema_version: SOURCES_EXPLORER_SCHEMA_VERSION.into(),
        repository: RepositoryIdentityV1 {
            id: text(
                repository
                    .get("id")
                    .or_else(|| repository.get("repositoryId")),
            ),
            origin: text(repository.get("origin")),
            revision: text(repository.get("revision")),
        },
        generation: GenerationV1 {
            id: text(
                generation
                    .get("id")
                    .or_else(|| generation.get("generationId")),
            ),
            release: text(generation.get("release")),
            index: text(generation.get("index")),
        },
        clocks: ClocksV1 {
            observed_at_unix_ms: clock(
                clocks
                    .get("observedAtUnixMs")
                    .or_else(|| clocks.get("observed_at_unix_ms")),
            ),
            contribution_at_unix_ms: clock(clocks.get("contributionAtUnixMs")),
            source_age_ms: clock(clocks.get("sourceAgeMs")),
        },
        readiness: ReadinessV1 {
            state: text(readiness.get("state")),
            reason: text(readiness.get("reason")),
            authoritative: readiness
                .get("authoritative")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            observed_at_unix_ms: clock(
                readiness
                    .get("observedAtUnixMs")
                    .or_else(|| readiness.get("observed_at_unix_ms")),
            ),
        },
        parser: ParserCapabilityV1 {
            name: text(parser.get("name")),
            version: text(
                parser
                    .get("version")
                    .or_else(|| parser.get("parserVersion")),
            ),
            languages: parser
                .get("languages")
                .and_then(Value::as_array)
                .map(|v| {
                    v.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .take(MAX_PATHS)
                        .collect()
                })
                .unwrap_or_default(),
            state: text(parser.get("state")),
        },
        recent_contribution: ContributionV1 {
            id: text(contribution.get("id")),
            summary: text(contribution.get("summary")),
            paths: contribution
                .get("paths")
                .and_then(Value::as_array)
                .map(|v| {
                    v.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .take(MAX_PATHS)
                        .collect()
                })
                .unwrap_or_default(),
            observed_at_unix_ms: clock(contribution.get("observedAtUnixMs")),
        },
        paths,
        neighborhoods,
    }
}

pub fn project_sources_explorer_json(payload: &Value) -> Value {
    serde_json::to_value(project_sources_explorer(payload))
        .expect("sources explorer projection serializes")
}
