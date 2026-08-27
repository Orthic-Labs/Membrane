//! Bounded, read-only adapter for CodeRight host observation endpoints.
//!
//! CodeRight owns `/runtime/resource-metrics` and `/runtime/observations`.
//! This module only accepts snapshots returned by those endpoints; it does
//! not perform network I/O, persist records, admit them, or assign semantic
//! labels.  Missing host values remain typed unavailable fields.

use std::collections::{BTreeMap, BTreeSet};

use membrane_adapt::lineage::{LearningLineageV1, LineageCoverageGapV1, LineageUnavailableReason};
use membrane_protocol::host_observation::{
    ObservationCoverageV1, ObservationUnavailableReasonV1, ObservedFieldV1,
    RemainingContextCeilingV1,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const HOST_OBSERVATION_INGRESS_SCHEMA_V1: &str = "membrane.host-observation-ingress.v1";
pub const CODERIGHT_RESOURCE_METRICS_SCHEMA_V1: &str = "coderight.resource-metrics.v1";
pub const CODERIGHT_EXECUTION_OBSERVATION_SCHEMA_V1: &str = "coderight.execution-observation.v1";
pub const CODERIGHT_TOOL_SCHEMA_SCHEMA_V1: &str = "coderight.tool-schema-observation.v1";
pub const CODERIGHT_PROCEDURAL_ASSET_SCHEMA_V1: &str = "coderight.procedural-asset-observation.v1";
pub const CODERIGHT_EVALUATION_OUTCOME_SCHEMA_V1: &str = "coderight.evaluation-outcome.v1";

/// Limits are applied before any row is materialized.  They match host sink
/// bounds where possible and keep one malformed response from growing a read
/// model without bound.
pub const MAX_HOST_OBSERVATION_ROWS: usize = 4096;
pub const MAX_HOST_OBSERVATION_BYTES: usize = 2 * 1024 * 1024;

pub type HostObservedFieldV1<T> = ObservedFieldV1<T>;
pub type HostObservationCoverageV1 = ObservationCoverageV1;
pub type HostObservationUnavailableReasonV1 = ObservationUnavailableReasonV1;

#[derive(Debug, PartialEq, Eq)]
pub enum HostObservationIngressError {
    ResponseShape {
        source: String,
    },
    ResponseTooLarge {
        source: String,
        max: usize,
    },
    UnsupportedSchema {
        source: String,
        found: Option<String>,
        expected: &'static str,
    },
    RequiredField {
        source: String,
        field: String,
    },
    InvalidField {
        source: String,
        field: String,
        reason: String,
    },
    MissingCoverage {
        source: String,
        field: String,
    },
    ProhibitedSemanticField {
        source: String,
        field: String,
        key: String,
    },
    RowLimitExceeded {
        source: String,
        rows: usize,
        max: usize,
    },
    InvalidIdentity {
        source: String,
        row: usize,
        reason: String,
    },
    EstimatorBasisMismatch {
        left: String,
        right: String,
    },
}

impl std::fmt::Display for HostObservationIngressError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResponseShape { source } => {
                write!(formatter, "{source} response must be a JSON object")
            }
            Self::ResponseTooLarge { source, max } => {
                write!(formatter, "{source} response exceeds {max} bytes")
            }
            Self::UnsupportedSchema {
                source,
                found,
                expected,
            } => write!(
                formatter,
                "{source} response has unsupported schema {found:?}; expected {expected}"
            ),
            Self::RequiredField { source, field } => {
                write!(formatter, "{source} field {field} is required")
            }
            Self::InvalidField {
                source,
                field,
                reason,
            } => write!(
                formatter,
                "{source} field {field} has invalid value: {reason}"
            ),
            Self::MissingCoverage { source, field } => write!(
                formatter,
                "{source} field {field} is missing typed unavailable coverage"
            ),
            Self::ProhibitedSemanticField { source, field, key } => write!(
                formatter,
                "{source} field {field} violates host observation boundary: semantic key {key:?}"
            ),
            Self::RowLimitExceeded { source, rows, max } => {
                write!(formatter, "{source} row count {rows} exceeds {max}")
            }
            Self::InvalidIdentity {
                source,
                row,
                reason,
            } => write!(
                formatter,
                "{source} row {row} has an invalid identity: {reason}"
            ),
            Self::EstimatorBasisMismatch { left, right } => {
                write!(formatter, "token estimator bases differ: {left} != {right}")
            }
        }
    }
}

impl std::error::Error for HostObservationIngressError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostObservationProvenanceV1 {
    pub source: String,
    pub evidence_refs: HostObservedFieldV1<Vec<String>>,
    pub receipt_id: HostObservedFieldV1<String>,
    pub receipt_digest: HostObservedFieldV1<String>,
}

impl HostObservationProvenanceV1 {
    fn from_host_provenance(
        source: &str,
        value: Option<&Value>,
        field: &str,
    ) -> Result<Self, HostObservationIngressError> {
        let Some(object) = value.and_then(Value::as_object) else {
            return Ok(Self {
                source: source.to_string(),
                evidence_refs: unavailable(ObservationUnavailableReasonV1::ProviderOmitted),
                receipt_id: unavailable(ObservationUnavailableReasonV1::ProviderOmitted),
                receipt_digest: unavailable(ObservationUnavailableReasonV1::ProviderOmitted),
            });
        };
        let provenance_source = object_value(object, "source")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(source);
        let evidence_refs = match object_value(object, "evidence_ids")
            .or_else(|| object_value(object, "evidenceIds"))
        {
            Some(value) => string_array_field(value, field, "evidence_ids")?,
            None => unavailable(ObservationUnavailableReasonV1::ProviderOmitted),
        };
        let receipt_reason = object_value(object, "receipt_unavailable_reason")
            .or_else(|| object_value(object, "receiptUnavailableReason"))
            .map(|value| parse_unavailable_reason(value, field, "receipt_unavailable_reason"))
            .transpose()?;
        let receipt_id = match object_value(object, "receipt_id")
            .or_else(|| object_value(object, "receiptId"))
        {
            Some(value) if !value.is_null() => string_field(value, field, "receipt_id")?,
            _ => unavailable(
                receipt_reason.unwrap_or(ObservationUnavailableReasonV1::ProviderOmitted),
            ),
        };
        Ok(Self {
            source: provenance_source.to_string(),
            evidence_refs,
            receipt_id,
            // CodeRight currently supplies a receipt id but no SHA-256
            // receipt digest.  Hashing this response would invent a receipt
            // identity, so absence remains explicit.
            receipt_digest: unavailable(ObservationUnavailableReasonV1::ProviderOmitted),
        })
    }

    fn from_runtime(source: &str, receipt_id: &str, event_id: &str) -> Self {
        Self {
            source: source.to_string(),
            evidence_refs: HostObservedFieldV1::complete(vec![event_id.to_string()]),
            receipt_id: HostObservedFieldV1::complete(receipt_id.to_string()),
            receipt_digest: unavailable(ObservationUnavailableReasonV1::ProviderOmitted),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostH4ObservationKindV1 {
    ToolSchema,
    ProceduralAsset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostH4ObservationV1 {
    pub schema_version: String,
    pub kind: HostH4ObservationKindV1,
    pub observation_id: String,
    pub asset_id: HostObservedFieldV1<String>,
    pub label: HostObservedFieldV1<String>,
    pub digest: HostObservedFieldV1<String>,
    pub bytes: HostObservedFieldV1<u64>,
    pub token_estimate: HostObservedFieldV1<u64>,
    pub invocation_count: HostObservedFieldV1<u64>,
    pub success_count: HostObservedFieldV1<u64>,
    pub failure_count: HostObservedFieldV1<u64>,
    pub exposed_count: HostObservedFieldV1<u64>,
    pub selected_count: HostObservedFieldV1<u64>,
    pub applied_count: HostObservedFieldV1<u64>,
    pub corrections_after_use: HostObservedFieldV1<u64>,
    pub refs: HostObservedFieldV1<Vec<String>>,
    pub estimator_id: HostObservedFieldV1<String>,
    pub estimator_version: HostObservedFieldV1<String>,
    pub provenance: HostObservationProvenanceV1,
}

impl HostH4ObservationV1 {
    fn identity_refs(&self) -> impl Iterator<Item = &str> {
        let mut refs = Vec::new();
        refs.push(self.observation_id.as_str());
        if let Some(value) = self.asset_id.value.as_deref() {
            refs.push(value);
        }
        if let Some(value) = self.provenance.receipt_id.value.as_deref() {
            refs.push(value);
        }
        if let Some(values) = self.refs.value.as_ref() {
            refs.extend(values.iter().map(String::as_str));
        }
        refs.into_iter()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostExecutionObservationV1 {
    pub schema_version: String,
    pub observation_id: String,
    pub session_id: HostObservedFieldV1<String>,
    pub task_id: HostObservedFieldV1<String>,
    pub parent_task_id: HostObservedFieldV1<String>,
    pub agent_id: HostObservedFieldV1<String>,
    pub agent_role: HostObservedFieldV1<String>,
    pub observed_at: HostObservedFieldV1<String>,
    pub model: HostObservedFieldV1<String>,
    pub provider: HostObservedFieldV1<String>,
    pub client: HostObservedFieldV1<String>,
    pub route_policy: HostObservedFieldV1<String>,
    pub observation_kind: HostObservedFieldV1<String>,
    pub subject_id: HostObservedFieldV1<String>,
    pub tool: HostObservedFieldV1<String>,
    pub call_id: HostObservedFieldV1<String>,
    pub outcome: HostObservedFieldV1<String>,
    pub duration_ms: HostObservedFieldV1<u64>,
    pub artifact_refs: HostObservedFieldV1<Vec<String>>,
    pub evidence_refs: HostObservedFieldV1<Vec<String>>,
    pub attributes: HostObservedFieldV1<Map<String, Value>>,
    pub provenance: HostObservationProvenanceV1,
}

impl HostExecutionObservationV1 {
    fn identity_refs(&self) -> impl Iterator<Item = &str> {
        let mut refs = Vec::new();
        refs.push(self.observation_id.as_str());
        for field in [
            &self.session_id,
            &self.task_id,
            &self.parent_task_id,
            &self.agent_id,
            &self.subject_id,
            &self.call_id,
            &self.tool,
        ] {
            if let Some(value) = field.value.as_deref() {
                refs.push(value);
            }
        }
        for field in [&self.artifact_refs, &self.evidence_refs] {
            if let Some(values) = field.value.as_ref() {
                refs.extend(values.iter().map(String::as_str));
            }
        }
        if let Some(value) = self.provenance.receipt_id.value.as_deref() {
            refs.push(value);
        }
        refs.into_iter()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostEvaluationOutcomeV1 {
    pub schema_version: String,
    pub outcome_id: String,
    pub evaluator_id: HostObservedFieldV1<String>,
    pub evaluator_version: HostObservedFieldV1<String>,
    pub dataset_id: HostObservedFieldV1<String>,
    pub dataset_digest: HostObservedFieldV1<String>,
    pub case_id: HostObservedFieldV1<String>,
    pub experiment_id: HostObservedFieldV1<String>,
    pub trace_id: HostObservedFieldV1<String>,
    pub session_id: HostObservedFieldV1<String>,
    pub task_id: HostObservedFieldV1<String>,
    pub model: HostObservedFieldV1<String>,
    pub client: HostObservedFieldV1<String>,
    pub route_policy: HostObservedFieldV1<String>,
    pub score_type: String,
    pub score: HostObservedFieldV1<f64>,
    pub verdict: HostObservedFieldV1<String>,
    pub expected: HostObservedFieldV1<String>,
    pub reference: HostObservedFieldV1<String>,
    pub execution_receipt: HostObservedFieldV1<String>,
    pub baseline_ref: HostObservedFieldV1<String>,
    pub observed_cost_usd: HostObservedFieldV1<f64>,
    pub latency_ms: HostObservedFieldV1<u64>,
    pub tool_call_count: HostObservedFieldV1<u64>,
    pub observed_at_unix_ms: HostObservedFieldV1<u64>,
    pub evidence_source: String,
    pub provenance: HostObservationProvenanceV1,
}

impl HostEvaluationOutcomeV1 {
    fn identity_refs(&self) -> impl Iterator<Item = &str> {
        let mut refs = Vec::new();
        refs.push(self.outcome_id.as_str());
        for field in [
            &self.evaluator_id,
            &self.evaluator_version,
            &self.dataset_id,
            &self.case_id,
            &self.experiment_id,
            &self.trace_id,
            &self.session_id,
            &self.task_id,
            &self.model,
            &self.client,
            &self.route_policy,
            &self.execution_receipt,
        ] {
            if let Some(value) = field.value.as_deref() {
                refs.push(value);
            }
        }
        if let Some(value) = self.provenance.receipt_id.value.as_deref() {
            refs.push(value);
        }
        refs.into_iter()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostObservationIngressV1 {
    pub schema_version: String,
    pub source: String,
    pub h4: Vec<HostH4ObservationV1>,
    pub execution: Vec<HostExecutionObservationV1>,
    pub h6: Vec<HostEvaluationOutcomeV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub h8: Vec<RemainingContextCeilingV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coverage: Vec<LineageCoverageGapV1>,
}

impl Default for HostObservationIngressV1 {
    fn default() -> Self {
        Self {
            schema_version: HOST_OBSERVATION_INGRESS_SCHEMA_V1.to_string(),
            source: "coderight".to_string(),
            h4: Vec::new(),
            execution: Vec::new(),
            h6: Vec::new(),
            h8: Vec::new(),
            coverage: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostObservationJoinRowV1 {
    pub lineage_item_id: String,
    pub node_ids: Vec<String>,
    pub h4_observation_ids: Vec<String>,
    pub execution_observation_ids: Vec<String>,
    pub h6_outcome_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coverage: Vec<LineageCoverageGapV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostObservationJoinV1 {
    pub schema_version: String,
    pub rows: Vec<HostObservationJoinRowV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmatched_h4_observation_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmatched_execution_observation_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmatched_h6_outcome_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coverage: Vec<LineageCoverageGapV1>,
}

fn adapt_observed<T: Clone>(
    field: &HostObservedFieldV1<T>,
) -> membrane_adapt::procedural_effectiveness::Observed<T> {
    use membrane_adapt::procedural_effectiveness::{Coverage, Observed, UnavailableReason};
    let reason = || match field
        .unavailable_reason
        .unwrap_or(ObservationUnavailableReasonV1::ProviderOmitted)
    {
        ObservationUnavailableReasonV1::NotInstrumented => UnavailableReason::NotInstrumented,
        ObservationUnavailableReasonV1::HubInactive => UnavailableReason::HubInactive,
        ObservationUnavailableReasonV1::ProviderOmitted => UnavailableReason::ProviderOmitted,
        ObservationUnavailableReasonV1::HostUnsupported => UnavailableReason::HostUnsupported,
    };
    match (field.coverage, field.value.clone()) {
        (ObservationCoverageV1::Complete, Some(value)) => Observed::complete(value),
        (ObservationCoverageV1::Partial, Some(value)) => Observed {
            coverage: Coverage::Partial,
            value: Some(value),
            unavailable_reason: Some(reason()),
        },
        _ => Observed::unavailable(reason()),
    }
}

/// Produce Adapt effectiveness rows from the exact host/lineage join. This is
/// read-only; unmatched host records never enter the projection.
pub fn project_joined_effectiveness(
    ingress: &HostObservationIngressV1,
    lineages: &[LearningLineageV1],
) -> Vec<membrane_adapt::procedural_effectiveness::ProceduralAssetEffectivenessV1> {
    let join = join_host_observations(ingress, lineages);
    let mut assets = BTreeSet::new();
    for row in &join.rows {
        for id in &row.h4_observation_ids {
            if let Some(h4) = ingress
                .h4
                .iter()
                .find(|candidate| &candidate.observation_id == id)
            {
                if let Some(asset_id) = h4.asset_id.value.as_ref() {
                    assets.insert(asset_id.clone());
                }
            }
        }
    }
    assets.into_iter().map(|asset_id| {
        let relevant_rows = join.rows.iter().filter(|row| {
            row.h4_observation_ids.iter().any(|id| {
                ingress.h4.iter().any(|h4| {
                    h4.observation_id == *id
                        && h4.asset_id.value.as_deref() == Some(asset_id.as_str())
                })
            })
        }).collect::<Vec<_>>();
        let h4_ids: BTreeSet<&str> = relevant_rows
            .iter()
            .flat_map(|row| row.h4_observation_ids.iter().map(String::as_str))
            .collect();
        let h6_ids: BTreeSet<&str> = relevant_rows
            .iter()
            .flat_map(|row| row.h6_outcome_ids.iter().map(String::as_str))
            .collect();
        let observations = ingress.h4.iter().filter(|h4| h4.kind == HostH4ObservationKindV1::ProceduralAsset && h4_ids.contains(h4.observation_id.as_str()) && h4.asset_id.value.as_deref() == Some(asset_id.as_str())).map(|h4| membrane_adapt::procedural_effectiveness::HostProceduralAssetObservationV1 {
            observation_id: h4.observation_id.clone(), asset_id: adapt_observed(&h4.asset_id), assessed_at: membrane_adapt::procedural_effectiveness::Observed::unavailable(membrane_adapt::procedural_effectiveness::UnavailableReason::ProviderOmitted),
            exposures: adapt_observed(&h4.exposed_count), selections: adapt_observed(&h4.selected_count), applications: adapt_observed(&h4.applied_count), successes: adapt_observed(&h4.success_count), failures: adapt_observed(&h4.failure_count), corrections_after_use: adapt_observed(&h4.corrections_after_use), token_cost_per_turn: adapt_observed(&h4.token_estimate), model: adapt_observed(&h4.estimator_id), client: adapt_observed(&h4.estimator_version), evidence_refs: adapt_observed(&h4.refs),
        }).collect::<Vec<_>>();
        let evaluations = ingress.h6.iter().filter(|h6| h6_ids.contains(h6.outcome_id.as_str())).map(|h6| membrane_adapt::procedural_effectiveness::HostEvaluationObservationV1 {
            outcome_id: h6.outcome_id.clone(), asset_id: membrane_adapt::procedural_effectiveness::Observed::complete(asset_id.clone()), evaluator: adapt_observed(&h6.evaluator_id), dataset: adapt_observed(&h6.dataset_id), experiment: adapt_observed(&h6.experiment_id), score: adapt_observed(&h6.score), evidence_refs: adapt_observed(&h6.provenance.evidence_refs),
        }).collect::<Vec<_>>();
        membrane_adapt::procedural_effectiveness::project_host_effectiveness(&asset_id, &observations, &evaluations)
    }).collect()
}

impl Default for HostObservationJoinV1 {
    fn default() -> Self {
        Self {
            schema_version: HOST_OBSERVATION_INGRESS_SCHEMA_V1.to_string(),
            rows: Vec::new(),
            unmatched_h4_observation_ids: Vec::new(),
            unmatched_execution_observation_ids: Vec::new(),
            unmatched_h6_outcome_ids: Vec::new(),
            coverage: Vec::new(),
        }
    }
}

/// Parse one point-in-time response pair from CodeRight's existing read-only
/// endpoints. `None` means caller did not supply that endpoint response; it is
/// surfaced as `provider_omitted` rather than treated as an empty success.
pub fn ingest_coderight_snapshot(
    resource_metrics: Option<&Value>,
    runtime_observations: Option<&Value>,
) -> Result<HostObservationIngressV1, HostObservationIngressError> {
    ingest_coderight_snapshot_with_reason(
        resource_metrics,
        runtime_observations,
        ObservationUnavailableReasonV1::ProviderOmitted,
    )
}

/// Variant used by a caller that knows why an endpoint was unavailable (for
/// example `hub_inactive`); no reason is inferred by this adapter.
pub fn ingest_coderight_snapshot_with_reason(
    resource_metrics: Option<&Value>,
    runtime_observations: Option<&Value>,
    missing_reason: ObservationUnavailableReasonV1,
) -> Result<HostObservationIngressV1, HostObservationIngressError> {
    let mut result = HostObservationIngressV1::default();
    if let Some(value) = resource_metrics {
        parse_resource_metrics(value, &mut result)?;
    } else {
        result.coverage.push(LineageCoverageGapV1 {
            field: "resource_metrics".to_string(),
            reason: to_lineage_reason(missing_reason),
        });
    }
    if let Some(value) = runtime_observations {
        parse_runtime_observations(value, &mut result)?;
    } else {
        result.coverage.push(LineageCoverageGapV1 {
            field: "runtime_observations".to_string(),
            reason: to_lineage_reason(missing_reason),
        });
    }
    result.coverage.sort_by(|left, right| {
        left.field
            .cmp(&right.field)
            .then(left.reason.cmp(&right.reason))
    });
    result.coverage.dedup();
    Ok(result)
}

/// Join host facts to already-built Adapt lineage using exact IDs only. No
/// row is copied into lineage and no effectiveness verdict is derived.
pub fn join_host_observations(
    ingress: &HostObservationIngressV1,
    lineages: &[LearningLineageV1],
) -> HostObservationJoinV1 {
    let mut result = HostObservationJoinV1::default();
    let mut matched_h4 = BTreeSet::new();
    let mut matched_execution = BTreeSet::new();
    let mut matched_h6 = BTreeSet::new();

    for lineage in lineages {
        let node_ids: Vec<String> = lineage
            .nodes
            .iter()
            .flat_map(|node| {
                std::iter::once(node.id.as_str())
                    .chain(node.receipt_ids.iter().map(String::as_str))
                    .chain(node.digest.iter().map(String::as_str))
            })
            .map(str::to_string)
            .collect();
        let ids: BTreeSet<&str> = node_ids.iter().map(String::as_str).collect();
        let h4_ids: Vec<String> = ingress
            .h4
            .iter()
            .filter(|row| row.identity_refs().any(|value| ids.contains(value)))
            .map(|row| {
                matched_h4.insert(row.observation_id.clone());
                row.observation_id.clone()
            })
            .collect();
        let execution_ids: Vec<String> = ingress
            .execution
            .iter()
            .filter(|row| row.identity_refs().any(|value| ids.contains(value)))
            .map(|row| {
                matched_execution.insert(row.observation_id.clone());
                row.observation_id.clone()
            })
            .collect();
        let h6_ids: Vec<String> = ingress
            .h6
            .iter()
            .filter(|row| row.identity_refs().any(|value| ids.contains(value)))
            .map(|row| {
                matched_h6.insert(row.outcome_id.clone());
                row.outcome_id.clone()
            })
            .collect();
        let mut coverage = Vec::new();
        if h4_ids.is_empty() {
            coverage.push(LineageCoverageGapV1 {
                field: "host.h4".to_string(),
                reason: if ingress.h4.is_empty() {
                    LineageUnavailableReason::NotInstrumented
                } else {
                    LineageUnavailableReason::ProviderOmitted
                },
            });
        }
        if execution_ids.is_empty() {
            coverage.push(LineageCoverageGapV1 {
                field: "host.execution".to_string(),
                reason: if ingress.execution.is_empty() {
                    LineageUnavailableReason::NotInstrumented
                } else {
                    LineageUnavailableReason::ProviderOmitted
                },
            });
        }
        if h6_ids.is_empty() {
            coverage.push(LineageCoverageGapV1 {
                field: "host.h6".to_string(),
                reason: if ingress.h6.is_empty() {
                    LineageUnavailableReason::NotInstrumented
                } else {
                    LineageUnavailableReason::ProviderOmitted
                },
            });
        }
        result.rows.push(HostObservationJoinRowV1 {
            lineage_item_id: lineage.item_id.clone(),
            node_ids,
            h4_observation_ids: h4_ids,
            execution_observation_ids: execution_ids,
            h6_outcome_ids: h6_ids,
            coverage,
        });
    }

    result.unmatched_h4_observation_ids = ingress
        .h4
        .iter()
        .filter(|row| !matched_h4.contains(&row.observation_id))
        .map(|row| row.observation_id.clone())
        .collect();
    result.unmatched_execution_observation_ids = ingress
        .execution
        .iter()
        .filter(|row| !matched_execution.contains(&row.observation_id))
        .map(|row| row.observation_id.clone())
        .collect();
    result.unmatched_h6_outcome_ids = ingress
        .h6
        .iter()
        .filter(|row| !matched_h6.contains(&row.outcome_id))
        .map(|row| row.outcome_id.clone())
        .collect();
    if !result.unmatched_h4_observation_ids.is_empty() {
        result.coverage.push(LineageCoverageGapV1 {
            field: "unmatched.host.h4".to_string(),
            reason: LineageUnavailableReason::ProviderOmitted,
        });
    }
    if !result.unmatched_execution_observation_ids.is_empty() {
        result.coverage.push(LineageCoverageGapV1 {
            field: "unmatched.host.execution".to_string(),
            reason: LineageUnavailableReason::ProviderOmitted,
        });
    }
    if !result.unmatched_h6_outcome_ids.is_empty() {
        result.coverage.push(LineageCoverageGapV1 {
            field: "unmatched.host.h6".to_string(),
            reason: LineageUnavailableReason::ProviderOmitted,
        });
    }
    result.coverage.sort_by(|left, right| {
        left.field
            .cmp(&right.field)
            .then(left.reason.cmp(&right.reason))
    });
    result.coverage.dedup();
    result
}

/// Add or compare token estimates only when source estimator identity matches.
pub fn ensure_same_estimator_basis(
    left_id: &HostObservedFieldV1<String>,
    left_version: &HostObservedFieldV1<String>,
    right_id: &HostObservedFieldV1<String>,
    right_version: &HostObservedFieldV1<String>,
) -> Result<(), HostObservationIngressError> {
    let left = estimator_basis(left_id, left_version);
    let right = estimator_basis(right_id, right_version);
    match (left, right) {
        (Some(left), Some(right)) if left == right => Ok(()),
        (Some(left), Some(right)) => {
            Err(HostObservationIngressError::EstimatorBasisMismatch { left, right })
        }
        _ => Err(HostObservationIngressError::InvalidField {
            source: "host_observation".to_string(),
            field: "tokenEstimate.estimatorBasis".to_string(),
            reason: "estimator identity unavailable".to_string(),
        }),
    }
}

fn parse_resource_metrics(
    value: &Value,
    result: &mut HostObservationIngressV1,
) -> Result<(), HostObservationIngressError> {
    check_response(
        value,
        "resource_metrics",
        CODERIGHT_RESOURCE_METRICS_SCHEMA_V1,
    )?;
    let object = value.as_object().expect("checked response object");
    let Some(evidence) = object_value(object, "membraneEvidence") else {
        result.coverage.push(LineageCoverageGapV1 {
            field: "membraneEvidence".to_string(),
            reason: LineageUnavailableReason::ProviderOmitted,
        });
        return Ok(());
    };
    let Some(evidence) = evidence.as_object() else {
        return Err(HostObservationIngressError::InvalidField {
            source: "resource_metrics".to_string(),
            field: "membraneEvidence".to_string(),
            reason: "must be an object".to_string(),
        });
    };
    parse_h4(evidence, result)?;
    parse_h6(evidence, result)?;
    parse_h8(evidence, result)
}

fn parse_h4(
    evidence: &Map<String, Value>,
    result: &mut HostObservationIngressV1,
) -> Result<(), HostObservationIngressError> {
    let Some(h4) = object_value(evidence, "h4") else {
        result.coverage.push(LineageCoverageGapV1 {
            field: "h4".to_string(),
            reason: LineageUnavailableReason::ProviderOmitted,
        });
        return Ok(());
    };
    let Some(h4) = h4.as_object() else {
        return Err(HostObservationIngressError::InvalidField {
            source: "resource_metrics".to_string(),
            field: "membraneEvidence.h4".to_string(),
            reason: "must be an object".to_string(),
        });
    };
    parse_h4_rows(
        h4,
        "toolSchemaObservations",
        HostH4ObservationKindV1::ToolSchema,
        result,
    )?;
    parse_h4_rows(
        h4,
        "proceduralAssetObservations",
        HostH4ObservationKindV1::ProceduralAsset,
        result,
    )?;
    Ok(())
}

fn parse_h6(
    evidence: &Map<String, Value>,
    result: &mut HostObservationIngressV1,
) -> Result<(), HostObservationIngressError> {
    let Some(h6) = object_value(evidence, "h6") else {
        result.coverage.push(LineageCoverageGapV1 {
            field: "h6".to_string(),
            reason: LineageUnavailableReason::ProviderOmitted,
        });
        return Ok(());
    };
    let Some(h6) = h6.as_object() else {
        return Err(HostObservationIngressError::InvalidField {
            source: "resource_metrics".to_string(),
            field: "membraneEvidence.h6".to_string(),
            reason: "must be an object".to_string(),
        });
    };
    let Some(rows) = object_value(h6, "evaluationOutcomes") else {
        result.coverage.push(LineageCoverageGapV1 {
            field: "h6.evaluation_outcomes".to_string(),
            reason: LineageUnavailableReason::ProviderOmitted,
        });
        return Ok(());
    };
    let rows = rows
        .as_array()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: "resource_metrics".to_string(),
            field: "membraneEvidence.h6.evaluationOutcomes".to_string(),
            reason: "must be an array".to_string(),
        })?;
    check_row_limit("h6.evaluation_outcomes", rows.len())?;
    for (index, row) in rows.iter().enumerate() {
        result.h6.push(parse_h6_row(row, index)?);
    }
    Ok(())
}

fn parse_h8(
    evidence: &Map<String, Value>,
    result: &mut HostObservationIngressV1,
) -> Result<(), HostObservationIngressError> {
    let Some(h8) = object_value(evidence, "h8") else {
        result.coverage.push(LineageCoverageGapV1 {
            field: "h8".to_string(),
            reason: LineageUnavailableReason::ProviderOmitted,
        });
        return Ok(());
    };
    let Some(h8) = h8.as_object() else {
        return Err(HostObservationIngressError::InvalidField {
            source: "resource_metrics".to_string(),
            field: "membraneEvidence.h8".to_string(),
            reason: "must be an object".to_string(),
        });
    };
    let Some(rows) = object_value(h8, "remainingContextCeilings") else {
        result.coverage.push(LineageCoverageGapV1 {
            field: "h8.remaining_context_ceilings".to_string(),
            reason: LineageUnavailableReason::ProviderOmitted,
        });
        return Ok(());
    };
    let rows = rows
        .as_array()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: "resource_metrics".to_string(),
            field: "membraneEvidence.h8.remainingContextCeilings".to_string(),
            reason: "must be an array".to_string(),
        })?;
    check_row_limit("h8.remaining_context_ceilings", rows.len())?;
    for (index, row) in rows.iter().enumerate() {
        let parsed: RemainingContextCeilingV1 =
            serde_json::from_value(row.clone()).map_err(|error| {
                HostObservationIngressError::InvalidField {
                    source: "resource_metrics".to_string(),
                    field: format!("membraneEvidence.h8.remainingContextCeilings[{index}]"),
                    reason: error.to_string(),
                }
            })?;
        parsed
            .validate()
            .map_err(|error| HostObservationIngressError::InvalidField {
                source: "resource_metrics".to_string(),
                field: format!("membraneEvidence.h8.remainingContextCeilings[{index}]"),
                reason: error.to_string(),
            })?;
        result.h8.push(parsed);
    }
    Ok(())
}

fn parse_h4_rows(
    h4: &Map<String, Value>,
    key: &str,
    kind: HostH4ObservationKindV1,
    result: &mut HostObservationIngressV1,
) -> Result<(), HostObservationIngressError> {
    let Some(rows) = object_value(h4, key) else {
        result.coverage.push(LineageCoverageGapV1 {
            field: format!("h4.{key}"),
            reason: LineageUnavailableReason::ProviderOmitted,
        });
        return Ok(());
    };
    let rows = rows
        .as_array()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: "resource_metrics".to_string(),
            field: format!("membraneEvidence.h4.{key}"),
            reason: "must be an array".to_string(),
        })?;
    check_row_limit(&format!("h4.{key}"), rows.len())?;
    for (index, row) in rows.iter().enumerate() {
        result.h4.push(parse_h4_row(row, index, kind.clone())?);
    }
    Ok(())
}

fn parse_h4_row(
    value: &Value,
    row: usize,
    kind: HostH4ObservationKindV1,
) -> Result<HostH4ObservationV1, HostObservationIngressError> {
    let source = "resource_metrics";
    reject_semantic_fields(value, source, &format!("h4[{row}]"))?;
    let object = value
        .as_object()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("h4[{row}]"),
            reason: "must be an object".to_string(),
        })?;
    let schema_version = required_string(object, "schema_version", source, &format!("h4[{row}]"))?;
    let expected = match kind {
        HostH4ObservationKindV1::ToolSchema => CODERIGHT_TOOL_SCHEMA_SCHEMA_V1,
        HostH4ObservationKindV1::ProceduralAsset => CODERIGHT_PROCEDURAL_ASSET_SCHEMA_V1,
    };
    if schema_version != expected {
        return Err(HostObservationIngressError::UnsupportedSchema {
            source: source.to_string(),
            found: Some(schema_version),
            expected,
        });
    }
    let id_key = match kind {
        HostH4ObservationKindV1::ToolSchema => "tool_id",
        HostH4ObservationKindV1::ProceduralAsset => "asset_id",
    };
    let observation_id = required_string(object, id_key, source, &format!("h4[{row}]"))?;
    let label_key = match kind {
        HostH4ObservationKindV1::ToolSchema => "tool_name",
        HostH4ObservationKindV1::ProceduralAsset => "asset_kind",
    };
    let digest_key = match kind {
        HostH4ObservationKindV1::ToolSchema => "schema_digest",
        HostH4ObservationKindV1::ProceduralAsset => "digest",
    };
    let bytes_key = match kind {
        HostH4ObservationKindV1::ToolSchema => "schema_bytes",
        HostH4ObservationKindV1::ProceduralAsset => "bytes",
    };
    let token_key = match kind {
        HostH4ObservationKindV1::ToolSchema => "schema_token_estimate",
        HostH4ObservationKindV1::ProceduralAsset => "token_estimate",
    };
    let provenance_key = match kind {
        HostH4ObservationKindV1::ToolSchema => "provenance",
        HostH4ObservationKindV1::ProceduralAsset => "provenance_receipt",
    };
    let provenance = HostObservationProvenanceV1::from_host_provenance(
        "coderight.resource_metrics",
        object_value(object, provenance_key),
        &format!("h4[{row}].{provenance_key}"),
    )?;
    Ok(HostH4ObservationV1 {
        schema_version,
        kind,
        observation_id,
        asset_id: complete_string_or_unavailable(object, id_key, source, &format!("h4[{row}]"))?,
        label: complete_string_or_unavailable(object, label_key, source, &format!("h4[{row}]"))?,
        digest: complete_string_or_unavailable(object, digest_key, source, &format!("h4[{row}]"))?,
        bytes: complete_u64_or_unavailable(object, bytes_key, source, &format!("h4[{row}]"))?,
        token_estimate: token_value(object, token_key, source, &format!("h4[{row}]"))?,
        invocation_count: count_value(object, "invocation_count", source, &format!("h4[{row}]"))?,
        success_count: count_value(object, "success_count", source, &format!("h4[{row}]"))?,
        failure_count: count_value(object, "failure_count", source, &format!("h4[{row}]"))?,
        exposed_count: count_value(object, "exposed_count", source, &format!("h4[{row}]"))?,
        selected_count: count_value(object, "selected_count", source, &format!("h4[{row}]"))?,
        applied_count: count_value(object, "applied_count", source, &format!("h4[{row}]"))?,
        corrections_after_use: count_value(
            object,
            "corrections_after_use",
            source,
            &format!("h4[{row}]"),
        )?,
        refs: refs_value(object, &kind, source, &format!("h4[{row}]"))?,
        estimator_id: token_basis_value(
            object,
            token_key,
            "estimator_id",
            source,
            &format!("h4[{row}]"),
        )?,
        estimator_version: token_basis_value(
            object,
            token_key,
            "estimator_version",
            source,
            &format!("h4[{row}]"),
        )?,
        provenance,
    })
}

fn parse_h6_row(
    value: &Value,
    row: usize,
) -> Result<HostEvaluationOutcomeV1, HostObservationIngressError> {
    let source = "resource_metrics";
    reject_semantic_fields(value, source, &format!("h6[{row}]"))?;
    let object = value
        .as_object()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("h6[{row}]"),
            reason: "must be an object".to_string(),
        })?;
    let schema_version = required_string(object, "schema_version", source, &format!("h6[{row}]"))?;
    if schema_version != CODERIGHT_EVALUATION_OUTCOME_SCHEMA_V1 {
        return Err(HostObservationIngressError::UnsupportedSchema {
            source: source.to_string(),
            found: Some(schema_version),
            expected: CODERIGHT_EVALUATION_OUTCOME_SCHEMA_V1,
        });
    }
    let row_path = format!("h6[{row}]");
    let unavailable = unavailable_fields(object, source, &row_path)?;
    let provenance = HostObservationProvenanceV1::from_host_provenance(
        "coderight.resource_metrics",
        object_value(object, "execution_receipt"),
        &format!("{row_path}.execution_receipt"),
    )?;
    let execution_receipt = object_value(object, "execution_receipt")
        .and_then(Value::as_object)
        .and_then(|receipt| {
            object_value(receipt, "receipt_id").or_else(|| object_value(receipt, "receiptId"))
        })
        .filter(|value| !value.is_null())
        .map(|value| string_field(value, source, &format!("{row_path}.execution_receipt")))
        .transpose()?
        .unwrap_or_else(|| {
            unavailable_field_or_default(
                &unavailable,
                "execution_receipt",
                ObservationUnavailableReasonV1::ProviderOmitted,
            )
        });
    Ok(HostEvaluationOutcomeV1 {
        schema_version,
        outcome_id: required_string(object, "outcome_id", source, &row_path)?,
        evaluator_id: optional_host_string(
            object,
            "evaluator_id",
            &unavailable,
            source,
            &row_path,
        )?,
        evaluator_version: optional_host_string(
            object,
            "evaluator_version",
            &unavailable,
            source,
            &row_path,
        )?,
        dataset_id: optional_host_string(object, "dataset_id", &unavailable, source, &row_path)?,
        dataset_digest: optional_host_string(
            object,
            "dataset_digest",
            &unavailable,
            source,
            &row_path,
        )?,
        case_id: optional_host_string(object, "case_id", &unavailable, source, &row_path)?,
        experiment_id: optional_host_string(
            object,
            "experiment_id",
            &unavailable,
            source,
            &row_path,
        )?,
        trace_id: optional_host_string(object, "trace_id", &unavailable, source, &row_path)?,
        session_id: optional_host_string(object, "session_id", &unavailable, source, &row_path)?,
        task_id: optional_host_string(object, "task_id", &unavailable, source, &row_path)?,
        model: optional_host_string(object, "model", &unavailable, source, &row_path)?,
        client: optional_host_string(object, "client", &unavailable, source, &row_path)?,
        route_policy: optional_host_string(
            object,
            "route_policy",
            &unavailable,
            source,
            &row_path,
        )?,
        score_type: required_string(object, "score_type", source, &row_path)?,
        score: optional_f64(object, "score", &unavailable, source, &row_path)?,
        verdict: required_string(object, "verdict", source, &row_path)
            .map(HostObservedFieldV1::complete)?,
        expected: optional_host_string(object, "expected", &unavailable, source, &row_path)?,
        reference: optional_host_string(object, "reference", &unavailable, source, &row_path)?,
        execution_receipt,
        baseline_ref: optional_host_string(
            object,
            "baseline_ref",
            &unavailable,
            source,
            &row_path,
        )?,
        observed_cost_usd: required_f64(object, "observed_cost_usd", source, &row_path)
            .map(HostObservedFieldV1::complete)?,
        latency_ms: required_u64(object, "latency_ms", source, &row_path)
            .map(HostObservedFieldV1::complete)?,
        tool_call_count: required_u64(object, "tool_call_count", source, &row_path)
            .map(HostObservedFieldV1::complete)?,
        observed_at_unix_ms: required_u64(object, "timestamp_unix_ms", source, &row_path)
            .map(HostObservedFieldV1::complete)?,
        evidence_source: required_string(object, "evidence_source", source, &row_path)?,
        provenance,
    })
}

fn parse_runtime_observations(
    value: &Value,
    result: &mut HostObservationIngressV1,
) -> Result<(), HostObservationIngressError> {
    check_payload_size(value, "runtime_observations")?;
    reject_semantic_fields(value, "runtime_observations", "observations")?;
    let object = value
        .as_object()
        .ok_or_else(|| HostObservationIngressError::ResponseShape {
            source: "runtime_observations".to_string(),
        })?;
    let Some(rows) = object_value(object, "observations") else {
        result.coverage.push(LineageCoverageGapV1 {
            field: "runtime_observations.observations".to_string(),
            reason: LineageUnavailableReason::ProviderOmitted,
        });
        return Ok(());
    };
    let rows = rows
        .as_array()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: "runtime_observations".to_string(),
            field: "observations".to_string(),
            reason: "must be an array".to_string(),
        })?;
    check_row_limit("runtime_observations", rows.len())?;
    for (index, row) in rows.iter().enumerate() {
        result.execution.push(parse_runtime_row(row, index)?);
    }
    Ok(())
}

fn parse_runtime_row(
    value: &Value,
    row: usize,
) -> Result<HostExecutionObservationV1, HostObservationIngressError> {
    let source = "runtime_observations";
    let row_path = format!("observations[{row}]");
    let object = value
        .as_object()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: row_path.clone(),
            reason: "must be an object".to_string(),
        })?;
    let observation_id = required_string(object, "id", source, &row_path)?;
    let payload = object_value(object, "payload").ok_or_else(|| {
        HostObservationIngressError::RequiredField {
            source: source.to_string(),
            field: format!("{row_path}.payload"),
        }
    })?;
    reject_semantic_fields(payload, source, &format!("{row_path}.payload"))?;
    let payload = payload
        .as_object()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("{row_path}.payload"),
            reason: "must be an object".to_string(),
        })?;
    let schema_version = required_string(payload, "schemaVersion", source, &row_path)?;
    if schema_version != CODERIGHT_EXECUTION_OBSERVATION_SCHEMA_V1 {
        return Err(HostObservationIngressError::UnsupportedSchema {
            source: source.to_string(),
            found: Some(schema_version),
            expected: CODERIGHT_EXECUTION_OBSERVATION_SCHEMA_V1,
        });
    }
    let event_id = required_string(payload, "eventId", source, &row_path)?;
    let session_id = required_string(payload, "sessionId", source, &row_path)?;
    let attributes = object_value(payload, "attributes")
        .map(|value| {
            value
                .as_object()
                .cloned()
                .ok_or_else(|| HostObservationIngressError::InvalidField {
                    source: source.to_string(),
                    field: format!("{row_path}.payload.attributes"),
                    reason: "must be an object".to_string(),
                })
        })
        .transpose()?;
    let created_at = string_field_or_unavailable(
        payload,
        "createdAt",
        ObservationUnavailableReasonV1::ProviderOmitted,
        source,
        &row_path,
    )?;
    let field = |key: &str| {
        attributes
            .as_ref()
            .and_then(|attrs| object_value(attrs, key))
            .map(|value| string_field(value, source, &format!("{row_path}.attributes.{key}")))
            .transpose()
            .map(|value| {
                value
                    .unwrap_or_else(|| unavailable(ObservationUnavailableReasonV1::ProviderOmitted))
            })
    };
    let array_field = |key: &str| {
        attributes
            .as_ref()
            .and_then(|attrs| object_value(attrs, key))
            .map(|value| string_array_field(value, source, &format!("{row_path}.attributes.{key}")))
            .transpose()
            .map(|value| {
                value
                    .unwrap_or_else(|| unavailable(ObservationUnavailableReasonV1::ProviderOmitted))
            })
    };
    let observation_kind = string_field_or_unavailable(
        payload,
        "phase",
        ObservationUnavailableReasonV1::ProviderOmitted,
        source,
        &row_path,
    )?;
    let outcome = string_field_or_unavailable(
        payload,
        "status",
        ObservationUnavailableReasonV1::ProviderOmitted,
        source,
        &row_path,
    )?;
    Ok(HostExecutionObservationV1 {
        schema_version,
        observation_id: observation_id.clone(),
        session_id: HostObservedFieldV1::complete(session_id),
        task_id: field("taskId")?,
        parent_task_id: string_field_or_unavailable(
            payload,
            "causalParentId",
            ObservationUnavailableReasonV1::ProviderOmitted,
            source,
            &row_path,
        )?,
        agent_id: field("agentId")?,
        agent_role: field("agentRole")?,
        observed_at: created_at,
        model: field("model")?,
        provider: field("provider")?,
        client: field("client")?,
        route_policy: field("routePolicy")?,
        observation_kind,
        subject_id: field("subjectId")?,
        tool: field("tool")?,
        call_id: field("callId")?,
        outcome,
        duration_ms: unavailable(ObservationUnavailableReasonV1::ProviderOmitted),
        artifact_refs: array_field("artifactRefs")?,
        evidence_refs: array_field("evidenceRefs")?,
        attributes: attributes
            .as_ref()
            .map(|value| HostObservedFieldV1::complete(value.clone()))
            .unwrap_or_else(|| unavailable(ObservationUnavailableReasonV1::ProviderOmitted)),
        provenance: HostObservationProvenanceV1::from_runtime(source, &observation_id, &event_id),
    })
}

fn check_response(
    value: &Value,
    source: &str,
    expected: &'static str,
) -> Result<(), HostObservationIngressError> {
    check_payload_size(value, source)?;
    reject_semantic_fields(value, source, "response")?;
    let object = value
        .as_object()
        .ok_or_else(|| HostObservationIngressError::ResponseShape {
            source: source.to_string(),
        })?;
    let found = object_value(object, "schemaVersion")
        .and_then(Value::as_str)
        .map(str::to_string);
    if found.as_deref() != Some(expected) {
        return Err(HostObservationIngressError::UnsupportedSchema {
            source: source.to_string(),
            found,
            expected,
        });
    }
    Ok(())
}

fn check_payload_size(value: &Value, source: &str) -> Result<(), HostObservationIngressError> {
    let size = serde_json::to_vec(value)
        .map_err(|error| HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: "response".to_string(),
            reason: error.to_string(),
        })?
        .len();
    if size > MAX_HOST_OBSERVATION_BYTES {
        return Err(HostObservationIngressError::ResponseTooLarge {
            source: source.to_string(),
            max: MAX_HOST_OBSERVATION_BYTES,
        });
    }
    Ok(())
}

fn check_row_limit(source: &str, rows: usize) -> Result<(), HostObservationIngressError> {
    if rows > MAX_HOST_OBSERVATION_ROWS {
        Err(HostObservationIngressError::RowLimitExceeded {
            source: source.to_string(),
            rows,
            max: MAX_HOST_OBSERVATION_ROWS,
        })
    } else {
        Ok(())
    }
}

fn reject_semantic_fields(
    value: &Value,
    source: &str,
    field: &str,
) -> Result<(), HostObservationIngressError> {
    const PROHIBITED: &[&str] = &[
        "admitted",
        "admission",
        "category",
        "semanticCategory",
        "semantic_category",
    ];
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if PROHIBITED.contains(&key.as_str()) {
                    return Err(HostObservationIngressError::ProhibitedSemanticField {
                        source: source.to_string(),
                        field: field.to_string(),
                        key: key.clone(),
                    });
                }
                reject_semantic_fields(child, source, &format!("{field}.{key}"))?;
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                reject_semantic_fields(child, source, &format!("{field}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn object_value<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    object.get(key).or_else(|| {
        let camel = snake_to_camel(key);
        object.get(camel.as_str())
    })
}

fn snake_to_camel(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut uppercase = false;
    for ch in value.chars() {
        if ch == '_' {
            uppercase = true;
        } else if uppercase {
            result.extend(ch.to_uppercase());
            uppercase = false;
        } else {
            result.push(ch);
        }
    }
    result
}

fn required_string(
    object: &Map<String, Value>,
    key: &str,
    source: &str,
    field: &str,
) -> Result<String, HostObservationIngressError> {
    let value =
        object_value(object, key).ok_or_else(|| HostObservationIngressError::RequiredField {
            source: source.to_string(),
            field: format!("{field}.{key}"),
        })?;
    let value = value
        .as_str()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("{field}.{key}"),
            reason: "must be a string".to_string(),
        })?;
    if value.trim().is_empty() {
        return Err(HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("{field}.{key}"),
            reason: "must not be empty".to_string(),
        });
    }
    Ok(value.to_string())
}

fn string_field(
    value: &Value,
    source: &str,
    field: &str,
) -> Result<HostObservedFieldV1<String>, HostObservationIngressError> {
    let value = value
        .as_str()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: field.to_string(),
            reason: "must be a string".to_string(),
        })?;
    if value.trim().is_empty() {
        return Err(HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: field.to_string(),
            reason: "must not be empty".to_string(),
        });
    }
    Ok(HostObservedFieldV1::complete(value.to_string()))
}

fn string_field_or_unavailable(
    object: &Map<String, Value>,
    key: &str,
    reason: ObservationUnavailableReasonV1,
    source: &str,
    field: &str,
) -> Result<HostObservedFieldV1<String>, HostObservationIngressError> {
    object_value(object, key)
        .filter(|value| !value.is_null())
        .map(|value| string_field(value, source, &format!("{field}.{key}")))
        .transpose()
        .map(|value| value.unwrap_or_else(|| unavailable(reason)))
}

fn complete_string_or_unavailable(
    object: &Map<String, Value>,
    key: &str,
    source: &str,
    field: &str,
) -> Result<HostObservedFieldV1<String>, HostObservationIngressError> {
    object_value(object, key)
        .map(|value| string_field(value, source, &format!("{field}.{key}")))
        .transpose()
        .map(|value| {
            value.unwrap_or_else(|| unavailable(ObservationUnavailableReasonV1::ProviderOmitted))
        })
}

fn required_u64(
    object: &Map<String, Value>,
    key: &str,
    source: &str,
    field: &str,
) -> Result<u64, HostObservationIngressError> {
    let value =
        object_value(object, key).ok_or_else(|| HostObservationIngressError::RequiredField {
            source: source.to_string(),
            field: format!("{field}.{key}"),
        })?;
    value
        .as_u64()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("{field}.{key}"),
            reason: "must be an unsigned integer".to_string(),
        })
}

fn required_f64(
    object: &Map<String, Value>,
    key: &str,
    source: &str,
    field: &str,
) -> Result<f64, HostObservationIngressError> {
    let value =
        object_value(object, key).ok_or_else(|| HostObservationIngressError::RequiredField {
            source: source.to_string(),
            field: format!("{field}.{key}"),
        })?;
    let value = value
        .as_f64()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("{field}.{key}"),
            reason: "must be a number".to_string(),
        })?;
    if !value.is_finite() {
        return Err(HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("{field}.{key}"),
            reason: "must be finite".to_string(),
        });
    }
    Ok(value)
}

fn complete_u64_or_unavailable(
    object: &Map<String, Value>,
    key: &str,
    source: &str,
    field: &str,
) -> Result<HostObservedFieldV1<u64>, HostObservationIngressError> {
    object_value(object, key)
        .map(|value| {
            value
                .as_u64()
                .map(HostObservedFieldV1::complete)
                .ok_or_else(|| HostObservationIngressError::InvalidField {
                    source: source.to_string(),
                    field: format!("{field}.{key}"),
                    reason: "must be an unsigned integer".to_string(),
                })
        })
        .transpose()
        .map(|value| {
            value.unwrap_or_else(|| unavailable(ObservationUnavailableReasonV1::ProviderOmitted))
        })
}

fn optional_f64(
    object: &Map<String, Value>,
    key: &str,
    unavailable: &BTreeMap<String, ObservationUnavailableReasonV1>,
    source: &str,
    field: &str,
) -> Result<HostObservedFieldV1<f64>, HostObservationIngressError> {
    match object_value(object, key) {
        Some(value) if !value.is_null() => {
            let value =
                value
                    .as_f64()
                    .ok_or_else(|| HostObservationIngressError::InvalidField {
                        source: source.to_string(),
                        field: format!("{field}.{key}"),
                        reason: "must be a number".to_string(),
                    })?;
            if !value.is_finite() {
                return Err(HostObservationIngressError::InvalidField {
                    source: source.to_string(),
                    field: format!("{field}.{key}"),
                    reason: "must be finite".to_string(),
                });
            }
            Ok(HostObservedFieldV1::complete(value))
        }
        _ => unavailable_field(unavailable, key, source, field),
    }
}

fn optional_host_string(
    object: &Map<String, Value>,
    key: &str,
    unavailable: &BTreeMap<String, ObservationUnavailableReasonV1>,
    source: &str,
    field: &str,
) -> Result<HostObservedFieldV1<String>, HostObservationIngressError> {
    match object_value(object, key) {
        Some(value) if !value.is_null() => string_field(value, source, &format!("{field}.{key}")),
        _ => unavailable_field(unavailable, key, source, field),
    }
}

fn unavailable_fields(
    object: &Map<String, Value>,
    source: &str,
    field: &str,
) -> Result<BTreeMap<String, ObservationUnavailableReasonV1>, HostObservationIngressError> {
    let Some(value) = object_value(object, "unavailable_fields") else {
        return Ok(BTreeMap::new());
    };
    let rows = value
        .as_array()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("{field}.unavailable_fields"),
            reason: "must be an array".to_string(),
        })?;
    let mut result = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        let row = row
            .as_object()
            .ok_or_else(|| HostObservationIngressError::InvalidField {
                source: source.to_string(),
                field: format!("{field}.unavailable_fields[{index}]"),
                reason: "must be an object".to_string(),
            })?;
        let name = required_string(
            row,
            "field",
            source,
            &format!("{field}.unavailable_fields[{index}]"),
        )?;
        let reason_value = object_value(row, "reason").ok_or_else(|| {
            HostObservationIngressError::RequiredField {
                source: source.to_string(),
                field: format!("{field}.unavailable_fields[{index}].reason"),
            }
        })?;
        let reason = parse_unavailable_reason(reason_value, source, &format!("{field}.{name}"))?;
        result.insert(name, reason);
    }
    Ok(result)
}

fn unavailable_field<T>(
    fields: &BTreeMap<String, ObservationUnavailableReasonV1>,
    key: &str,
    source: &str,
    field: &str,
) -> Result<HostObservedFieldV1<T>, HostObservationIngressError> {
    let reason =
        fields
            .get(key)
            .copied()
            .ok_or_else(|| HostObservationIngressError::MissingCoverage {
                source: source.to_string(),
                field: format!("{field}.{key}"),
            })?;
    Ok(unavailable(reason))
}

fn unavailable_field_or_default<T>(
    fields: &BTreeMap<String, ObservationUnavailableReasonV1>,
    key: &str,
    default_reason: ObservationUnavailableReasonV1,
) -> HostObservedFieldV1<T> {
    unavailable(*fields.get(key).unwrap_or(&default_reason))
}

fn parse_unavailable_reason(
    value: &Value,
    source: &str,
    field: &str,
) -> Result<ObservationUnavailableReasonV1, HostObservationIngressError> {
    let value = value
        .as_str()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: field.to_string(),
            reason: "unavailable reason must be a string".to_string(),
        })?;
    match value {
        "not_instrumented" => Ok(ObservationUnavailableReasonV1::NotInstrumented),
        "hub_inactive" => Ok(ObservationUnavailableReasonV1::HubInactive),
        "provider_omitted" | "input_not_provided" | "pending_result" => {
            Ok(ObservationUnavailableReasonV1::ProviderOmitted)
        }
        "host_unsupported" | "unsupported" | "incompatible_basis" | "overflow" => {
            Ok(ObservationUnavailableReasonV1::HostUnsupported)
        }
        other => Err(HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: field.to_string(),
            reason: format!("unsupported unavailable reason {other:?}"),
        }),
    }
}

fn count_value(
    object: &Map<String, Value>,
    key: &str,
    source: &str,
    field: &str,
) -> Result<HostObservedFieldV1<u64>, HostObservationIngressError> {
    let Some(value) = object_value(object, key) else {
        return Ok(unavailable(ObservationUnavailableReasonV1::ProviderOmitted));
    };
    let Some(value) = value.as_object() else {
        return Err(HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("{field}.{key}"),
            reason: "count must be an object".to_string(),
        });
    };
    let coverage = object_value(value, "coverage")
        .and_then(Value::as_str)
        .unwrap_or("unavailable");
    let reason = object_value(value, "unavailable_reason")
        .or_else(|| object_value(value, "unavailableReason"))
        .map(|reason| parse_unavailable_reason(reason, source, &format!("{field}.{key}")))
        .transpose()?;
    match coverage {
        "complete" => object_value(value, "count")
            .and_then(Value::as_u64)
            .map(HostObservedFieldV1::complete)
            .ok_or_else(|| HostObservationIngressError::MissingCoverage {
                source: source.to_string(),
                field: format!("{field}.{key}"),
            }),
        "partial" => Ok(HostObservedFieldV1::partial(
            object_value(value, "count").and_then(Value::as_u64),
            reason.ok_or_else(|| HostObservationIngressError::MissingCoverage {
                source: source.to_string(),
                field: format!("{field}.{key}"),
            })?,
        )),
        "unavailable" => Ok(unavailable(reason.ok_or_else(|| {
            HostObservationIngressError::MissingCoverage {
                source: source.to_string(),
                field: format!("{field}.{key}"),
            }
        })?)),
        other => Err(HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("{field}.{key}.coverage"),
            reason: format!("unsupported coverage {other:?}"),
        }),
    }
}

fn token_value(
    object: &Map<String, Value>,
    key: &str,
    source: &str,
    field: &str,
) -> Result<HostObservedFieldV1<u64>, HostObservationIngressError> {
    let Some(value) = object_value(object, key) else {
        return Ok(unavailable(ObservationUnavailableReasonV1::ProviderOmitted));
    };
    let Some(value) = value.as_object() else {
        return Err(HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("{field}.{key}"),
            reason: "token estimate must be an object".to_string(),
        });
    };
    match object_value(value, "tokens").and_then(Value::as_u64) {
        Some(tokens) => Ok(HostObservedFieldV1::complete(tokens)),
        None => {
            let reason = object_value(value, "unavailable_reason")
                .or_else(|| object_value(value, "unavailableReason"))
                .ok_or_else(|| HostObservationIngressError::MissingCoverage {
                    source: source.to_string(),
                    field: format!("{field}.{key}"),
                })?;
            Ok(unavailable(parse_unavailable_reason(
                reason,
                source,
                &format!("{field}.{key}"),
            )?))
        }
    }
}

fn token_basis_value(
    object: &Map<String, Value>,
    token_key: &str,
    basis_key: &str,
    source: &str,
    field: &str,
) -> Result<HostObservedFieldV1<String>, HostObservationIngressError> {
    let Some(token) = object_value(object, token_key) else {
        return Ok(unavailable(ObservationUnavailableReasonV1::ProviderOmitted));
    };
    let Some(token) = token.as_object() else {
        return Err(HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: format!("{field}.{token_key}"),
            reason: "token estimate must be an object".to_string(),
        });
    };
    let Some(value) = object_value(token, basis_key) else {
        return Ok(unavailable(ObservationUnavailableReasonV1::ProviderOmitted));
    };
    if value.as_str().is_some_and(|value| value.is_empty()) {
        let reason = object_value(token, "unavailable_reason")
            .or_else(|| object_value(token, "unavailableReason"))
            .map(|reason| parse_unavailable_reason(reason, source, &format!("{field}.{token_key}")))
            .transpose()?
            .unwrap_or(ObservationUnavailableReasonV1::ProviderOmitted);
        return Ok(unavailable(reason));
    }
    string_field(value, source, &format!("{field}.{token_key}.{basis_key}"))
}

fn refs_value(
    object: &Map<String, Value>,
    kind: &HostH4ObservationKindV1,
    source: &str,
    field: &str,
) -> Result<HostObservedFieldV1<Vec<String>>, HostObservationIngressError> {
    let key = match kind {
        HostH4ObservationKindV1::ToolSchema => "exact_overlap_refs",
        HostH4ObservationKindV1::ProceduralAsset => "visible_turn_ids",
    };
    let Some(value) = object_value(object, key) else {
        return Ok(unavailable(ObservationUnavailableReasonV1::ProviderOmitted));
    };
    let refs = string_array_field(value, source, &format!("{field}.{key}"))?;
    if refs.value.as_ref().is_some_and(|values| values.is_empty()) {
        let reason_key = format!("{key}_unavailable_reason");
        if let Some(reason) = object_value(object, reason_key.as_str()) {
            return Ok(unavailable(parse_unavailable_reason(
                reason,
                source,
                &format!("{field}.{reason_key}"),
            )?));
        }
    }
    Ok(refs)
}

fn string_array_field(
    value: &Value,
    source: &str,
    field: &str,
) -> Result<HostObservedFieldV1<Vec<String>>, HostObservationIngressError> {
    let values = value
        .as_array()
        .ok_or_else(|| HostObservationIngressError::InvalidField {
            source: source.to_string(),
            field: field.to_string(),
            reason: "must be an array".to_string(),
        })?;
    let mut result = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let value = value
            .as_str()
            .ok_or_else(|| HostObservationIngressError::InvalidField {
                source: source.to_string(),
                field: format!("{field}[{index}]"),
                reason: "must be a string".to_string(),
            })?;
        if value.trim().is_empty() {
            return Err(HostObservationIngressError::InvalidField {
                source: source.to_string(),
                field: format!("{field}[{index}]"),
                reason: "must not be empty".to_string(),
            });
        }
        result.push(value.to_string());
    }
    Ok(HostObservedFieldV1::complete(result))
}

fn estimator_basis(
    id: &HostObservedFieldV1<String>,
    version: &HostObservedFieldV1<String>,
) -> Option<String> {
    Some(format!(
        "{}@{}",
        id.value.as_ref()?,
        version.value.as_ref()?
    ))
}

fn unavailable<T>(reason: ObservationUnavailableReasonV1) -> HostObservedFieldV1<T> {
    HostObservedFieldV1::unavailable(reason)
}

fn to_lineage_reason(reason: ObservationUnavailableReasonV1) -> LineageUnavailableReason {
    match reason {
        ObservationUnavailableReasonV1::NotInstrumented => {
            LineageUnavailableReason::NotInstrumented
        }
        ObservationUnavailableReasonV1::HubInactive => LineageUnavailableReason::HubInactive,
        ObservationUnavailableReasonV1::ProviderOmitted => {
            LineageUnavailableReason::ProviderOmitted
        }
        ObservationUnavailableReasonV1::HostUnsupported => {
            LineageUnavailableReason::HostUnsupported
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn h6_row() -> Value {
        json!({
            "schema_version": CODERIGHT_EVALUATION_OUTCOME_SCHEMA_V1,
            "outcome_id": "out-1",
            "evaluator_id": null,
            "evaluator_version": null,
            "case_id": "case-1",
            "task_id": "task-1",
            "score_type": "verified_completion",
            "score": 1.0,
            "verdict": "pass",
            "observed_cost_usd": 0.0,
            "latency_ms": 10,
            "tool_call_count": 1,
            "evidence_source": "host",
            "execution_receipt": {
                "source": "coderight",
                "evidence_ids": ["task-1"],
                "receipt_id": null,
                "receipt_unavailable_reason": "input_not_provided"
            },
            "unavailable_fields": [
                {"field": "evaluator_id", "reason": "input_not_provided", "source": "host"},
                {"field": "evaluator_version", "reason": "input_not_provided", "source": "host"},
                {"field": "dataset_id", "reason": "input_not_provided", "source": "host"},
                {"field": "dataset_digest", "reason": "input_not_provided", "source": "host"},
                {"field": "experiment_id", "reason": "input_not_provided", "source": "host"},
                {"field": "trace_id", "reason": "input_not_provided", "source": "host"},
                {"field": "session_id", "reason": "input_not_provided", "source": "host"},
                {"field": "model", "reason": "input_not_provided", "source": "host"},
                {"field": "client", "reason": "input_not_provided", "source": "host"},
                {"field": "route_policy", "reason": "input_not_provided", "source": "host"},
                {"field": "expected", "reason": "input_not_provided", "source": "host"},
                {"field": "reference", "reason": "input_not_provided", "source": "host"},
                {"field": "baseline_ref", "reason": "input_not_provided", "source": "host"}
            ],
            "timestamp_unix_ms": 1_700_000_000_000u64
        })
    }

    fn metrics(rows: Vec<Value>) -> Value {
        json!({
            "schemaVersion": CODERIGHT_RESOURCE_METRICS_SCHEMA_V1,
            "membraneEvidence": {
                "h6": {"evaluationOutcomes": rows},
                "h4": {
                    "toolSchemaObservations": [],
                    "proceduralAssetObservations": []
                }
            }
        })
    }

    #[test]
    fn h6_ingress_preserves_typed_unavailable_fields() {
        let ingress = ingest_coderight_snapshot(Some(&metrics(vec![h6_row()])), None).unwrap();
        assert_eq!(ingress.h6.len(), 1);
        assert_eq!(
            ingress.h6[0].evaluator_id.unavailable_reason,
            Some(ObservationUnavailableReasonV1::ProviderOmitted)
        );
        assert!(ingress.h6[0].score.value == Some(1.0));
        assert!(ingress.h6[0].observed_cost_usd.value == Some(0.0));
    }

    #[test]
    fn joined_effectiveness_excludes_unmatched_h6_outcomes() {
        fn complete<T>(value: T) -> HostObservedFieldV1<T> {
            HostObservedFieldV1::complete(value)
        }
        let h4 = HostH4ObservationV1 {
            schema_version: CODERIGHT_PROCEDURAL_ASSET_SCHEMA_V1.into(),
            kind: HostH4ObservationKindV1::ProceduralAsset,
            observation_id: "h4-match".into(),
            asset_id: complete("asset-1".into()),
            label: complete("procedure".into()),
            digest: complete("digest".into()),
            bytes: complete(1),
            token_estimate: complete(2),
            invocation_count: complete(1),
            success_count: complete(1),
            failure_count: complete(0),
            exposed_count: complete(1),
            selected_count: complete(1),
            applied_count: complete(1),
            corrections_after_use: complete(0),
            refs: complete(vec!["h4-match".into()]),
            estimator_id: complete("estimator".into()),
            estimator_version: complete("1".into()),
            provenance: HostObservationProvenanceV1::from_runtime(
                "coderight",
                "receipt-1",
                "h4-match",
            ),
        };
        let h6 = |outcome_id: &str, score: f64| HostEvaluationOutcomeV1 {
            schema_version: CODERIGHT_EVALUATION_OUTCOME_SCHEMA_V1.into(),
            outcome_id: outcome_id.into(),
            evaluator_id: complete("eval".into()),
            evaluator_version: complete("1".into()),
            dataset_id: complete("set".into()),
            dataset_digest: complete("digest".into()),
            case_id: complete("case".into()),
            experiment_id: complete("exp".into()),
            trace_id: complete(outcome_id.into()),
            session_id: complete("session".into()),
            task_id: complete("task".into()),
            model: complete("model".into()),
            client: complete("client".into()),
            route_policy: complete("route".into()),
            score_type: "accuracy".into(),
            score: complete(score),
            verdict: complete("pass".into()),
            expected: complete("pass".into()),
            reference: complete("ref".into()),
            execution_receipt: complete("receipt".into()),
            baseline_ref: complete("base".into()),
            observed_cost_usd: complete(0.0),
            latency_ms: complete(1),
            tool_call_count: complete(1),
            observed_at_unix_ms: complete(1),
            evidence_source: "host".into(),
            provenance: HostObservationProvenanceV1::from_runtime(
                "coderight",
                "receipt-1",
                outcome_id,
            ),
        };
        let ingress = HostObservationIngressV1 {
            h4: vec![h4],
            h6: vec![h6("h6-match", 0.9), h6("h6-unmatched", 0.1)],
            ..Default::default()
        };
        let lineage = LearningLineageV1 {
            schema_version: "adapt.learning-lineage.v1".into(),
            item_id: "item".into(),
            nodes: vec![
                membrane_adapt::lineage::LineageRefV1 {
                    stage: membrane_adapt::lineage::LineageStage::Outcome,
                    id: "h4-match".into(),
                    receipt_ids: vec![],
                    digest: None,
                },
                membrane_adapt::lineage::LineageRefV1 {
                    stage: membrane_adapt::lineage::LineageStage::Outcome,
                    id: "h6-match".into(),
                    receipt_ids: vec![],
                    digest: None,
                },
            ],
            edges: vec![],
            coverage: vec![],
        };
        let rows = project_joined_effectiveness(&ingress, &[lineage]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].asset_id, "asset-1");
        assert_eq!(
            rows[0].effectiveness_verdict.value,
            Some(membrane_adapt::procedural_effectiveness::EffectivenessVerdict::Effective)
        );
        assert!(!rows[0]
            .evidence_refs
            .value
            .as_ref()
            .unwrap()
            .contains(&"h6-unmatched".into()));
    }

    #[test]
    fn h8_ingress_accepts_coderight_wire_shape() {
        let mut value = metrics(Vec::new());
        value["membraneEvidence"]["h8"] = json!({
            "remainingContextCeilings": [{
                "schemaVersion": 1,
                "ceilingId": "coderight-h8:session-1:1700000000000",
                "sessionId": "session-1",
                "taskId": {
                    "coverage": "unavailable",
                    "unavailableReason": "not_instrumented"
                },
                "requestedAtUnixMs": 1_700_000_000_000u64,
                "remainingTokens": {
                    "basis": {
                        "id": "provider_reported_context_tokens",
                        "version": "1"
                    },
                    "estimate": {"coverage": "complete", "value": 8_192}
                },
                "provenanceReceipt": {
                    "schemaVersion": 1,
                    "receiptId": "coderight-h8:session-1:1700000000000",
                    "source": "coderight",
                    "observedAtUnixMs": 1_700_000_000_000u64,
                    "receiptDigest": concat!(
                        "sha256:",
                        "0000000000000000000000000000000000000000000000000000000000000000"
                    )
                }
            }]
        });

        let ingress = ingest_coderight_snapshot(Some(&value), None).unwrap();
        assert_eq!(ingress.h8.len(), 1);
        assert_eq!(ingress.h8[0].remaining_tokens.estimate.value, Some(8_192));
        assert!(!ingress.coverage.iter().any(|gap| gap.field == "h8"));
    }

    #[test]
    fn semantic_prelabels_are_rejected_before_materialization() {
        let mut row = h6_row();
        row["category"] = json!("preferred");
        let error = ingest_coderight_snapshot(Some(&metrics(vec![row])), None).unwrap_err();
        assert!(matches!(
            error,
            HostObservationIngressError::ProhibitedSemanticField { key, .. } if key == "category"
        ));
        let mut row = h6_row();
        row["admitted"] = json!(true);
        let error = ingest_coderight_snapshot(Some(&metrics(vec![row])), None).unwrap_err();
        assert!(matches!(
            error,
            HostObservationIngressError::ProhibitedSemanticField { key, .. } if key == "admitted"
        ));
    }

    #[test]
    fn runtime_observation_missing_model_is_typed_not_zero() {
        let runtime = json!({
            "observations": [{
                "id": "evt-1",
                "payload": {
                    "schemaVersion": CODERIGHT_EXECUTION_OBSERVATION_SCHEMA_V1,
                    "eventId": "evt-1",
                    "sessionId": "session-1",
                    "phase": "tool",
                    "status": "ok",
                    "createdAt": "2026-08-28T00:00:00Z",
                    "attributes": {}
                }
            }]
        });
        let ingress = ingest_coderight_snapshot(None, Some(&runtime)).unwrap();
        assert_eq!(ingress.execution.len(), 1);
        assert_eq!(
            ingress.execution[0].model.unavailable_reason,
            Some(ObservationUnavailableReasonV1::ProviderOmitted)
        );
        assert!(ingress.execution[0].duration_ms.value.is_none());
    }

    #[test]
    fn runtime_missing_event_identity_is_rejected() {
        let runtime = json!({
            "observations": [{
                "id": "evt-1",
                "payload": {
                    "schemaVersion": CODERIGHT_EXECUTION_OBSERVATION_SCHEMA_V1,
                    "sessionId": "session-1"
                }
            }]
        });
        assert!(matches!(
            ingest_coderight_snapshot(None, Some(&runtime)),
            Err(HostObservationIngressError::RequiredField { .. })
        ));
    }

    #[test]
    fn join_uses_exact_lineage_ids_and_surfaces_unmatched_rows() {
        let ingress = ingest_coderight_snapshot(Some(&metrics(vec![h6_row()])), None).unwrap();
        let lineage = LearningLineageV1 {
            schema_version: "adapt.learning-lineage.v1".into(),
            item_id: "issue-1".into(),
            nodes: vec![membrane_adapt::lineage::LineageRefV1 {
                stage: membrane_adapt::lineage::LineageStage::Experience,
                id: "task-1".into(),
                receipt_ids: Vec::new(),
                digest: None,
            }],
            edges: Vec::new(),
            coverage: Vec::new(),
        };
        let joined = join_host_observations(&ingress, &[lineage]);
        assert_eq!(joined.rows[0].h6_outcome_ids, vec!["out-1"]);
        assert!(joined.unmatched_h4_observation_ids.is_empty());
        assert!(joined.unmatched_h6_outcome_ids.is_empty());
    }

    #[test]
    fn incompatible_estimator_basis_is_rejected() {
        let left = HostObservedFieldV1::complete("bytes".into());
        let left_version = HostObservedFieldV1::complete("1".into());
        let right = HostObservedFieldV1::complete("provider".into());
        let right_version = HostObservedFieldV1::complete("1".into());
        assert!(matches!(
            ensure_same_estimator_basis(&left, &left_version, &right, &right_version),
            Err(HostObservationIngressError::EstimatorBasisMismatch { .. })
        ));
    }
}
