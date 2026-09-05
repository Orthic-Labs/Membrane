//! Versioned Adapt execution-efficiency detectors over the existing H4 host contract.
//!
//! The detector family is deliberately evidence-conservative. A detector only
//! emits a finding when H4 contains the exact mechanical facts required by the
//! detector. When the current host contract cannot express a prerequisite, the
//! detector returns `unavailable` with the missing fact rather than treating
//! missing telemetry as clean behavior.

use membrane_protocol::host_observation::{
    ExecutionCostV1, ExecutionObservationKindV1, ExecutionObservationV1, ObservationCoverageV1,
    ObservedFieldV1, HOST_OBSERVATION_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const DETECTOR_FAMILY_ID: &str = "adapt.execution-efficiency";
pub const DETECTOR_FAMILY_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectorCoverageStateV1 {
    Ran,
    Skipped,
    Unavailable,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectorContractV1 {
    pub atom_id: String,
    pub detector_id: String,
    pub detector_version: u32,
    pub required_host_facts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectorCoverageV1 {
    pub contract: String,
    pub family_id: String,
    pub family_version: u32,
    pub atom_id: String,
    pub detector_id: String,
    pub detector_version: u32,
    pub input_schema_version: u32,
    pub input_digest: String,
    pub state: DetectorCoverageStateV1,
    pub missing_fields: Vec<String>,
    pub findings: Vec<Value>,
    pub qualified_metrics: BTreeMap<String, Value>,
    pub honesty_limit: String,
}

fn contract(atom: &str, detector: &str, required: &[&str]) -> DetectorContractV1 {
    DetectorContractV1 {
        atom_id: atom.into(),
        detector_id: detector.into(),
        detector_version: 1,
        required_host_facts: required.iter().map(|value| (*value).into()).collect(),
    }
}

/// The committed harness-efficiency detector catalog (ADP-043..ADP-064).
///
/// A catalog entry is present even when H4 cannot currently supply its required
/// fact. That distinction lets runtime coverage say "detector unavailable"
/// rather than incorrectly saying "no finding".
pub fn detector_catalog() -> Vec<DetectorContractV1> {
    vec![
        contract(
            "ADP-043",
            "duplicate_assignment_execution",
            &["assignment_id", "worker_id"],
        ),
        contract(
            "ADP-044",
            "orchestrator_role_leakage",
            &["agent_role", "lane_owner_identity", "lane_execution_boundary"],
        ),
        contract(
            "ADP-045",
            "lane_scope_overlap",
            &["lane_id", "accepted_work_scope"],
        ),
        contract(
            "ADP-046",
            "bounded_lane_budget_exceeded",
            &["declared_lane_budget", "qualified_usage_or_cost"],
        ),
        contract(
            "ADP-047",
            "missing_efficiency_budget",
            &["required_efficiency_budget_contract"],
        ),
        contract(
            "ADP-048",
            "fanout_without_incremental_value",
            &["subagent_identity", "incremental_accepted_value_identity"],
        ),
        contract(
            "ADP-049",
            "subagent_context_duplication",
            &["subagent_context_digest"],
        ),
        contract(
            "ADP-050",
            "context_replay_amplification",
            &["context_replay_digest", "replay_size"],
        ),
        contract(
            "ADP-051",
            "cold_cache_rebuild_waste",
            &["cache_key", "cache_rebuild_identity"],
        ),
        contract(
            "ADP-052",
            "cache_invalidation_churn",
            &["cache_key", "cache_invalidation_event"],
        ),
        contract(
            "ADP-053",
            "no_progress_model_loop",
            &["model_call_failure", "progress_event"],
        ),
        contract(
            "ADP-054",
            "duplicate_tool_work",
            &["tool", "subject_id"],
        ),
        contract(
            "ADP-055",
            "semantic_tool_work_overlap",
            &["semantic_work_digest"],
        ),
        contract(
            "ADP-056",
            "oversized_tool_result_replay",
            &["tool_result_size_bytes", "replay_identity"],
        ),
        contract(
            "ADP-057",
            "retry_loop_cost",
            &["retry_event", "qualified_cost"],
        ),
        contract(
            "ADP-058",
            "verification_churn",
            &["verification_identity"],
        ),
        contract(
            "ADP-059",
            "replan_churn",
            &["plan_revision", "progress_event"],
        ),
        contract(
            "ADP-060",
            "routing_cost_mismatch",
            &["route_policy", "declared_route_cost_expectation"],
        ),
        contract(
            "ADP-061",
            "integration_rework_from_lane_failure",
            &["lane_failure_causal_link", "integration_rework_identity"],
        ),
        contract(
            "ADP-062",
            "stranded_worker_work",
            &["subagent_identity", "terminal_task_event"],
        ),
        contract(
            "ADP-063",
            "background_learning_over_budget",
            &["background_learning_identity", "background_learning_budget"],
        ),
        contract(
            "ADP-064",
            "qualified_execution_reporting",
            &["execution_observations"],
        ),
    ]
}

fn exact<T>(field: &ObservedFieldV1<T>) -> Option<&T> {
    (field.coverage == ObservationCoverageV1::Complete)
        .then_some(field.value.as_ref())
        .flatten()
}

fn coverage(
    atom: &str,
    detector: &str,
    input_digest: &str,
    state: DetectorCoverageStateV1,
    missing_fields: Vec<String>,
    findings: Vec<Value>,
    qualified_metrics: BTreeMap<String, Value>,
    honesty_limit: impl Into<String>,
) -> DetectorCoverageV1 {
    DetectorCoverageV1 {
        contract: "adapt.detector-coverage-item.v1".into(),
        family_id: DETECTOR_FAMILY_ID.into(),
        family_version: DETECTOR_FAMILY_VERSION,
        atom_id: atom.into(),
        detector_id: detector.into(),
        detector_version: 1,
        input_schema_version: HOST_OBSERVATION_SCHEMA_VERSION,
        input_digest: input_digest.into(),
        state,
        missing_fields,
        findings,
        qualified_metrics,
        honesty_limit: honesty_limit.into(),
    }
}

fn ran(
    atom: &str,
    detector: &str,
    input_digest: &str,
    findings: Vec<Value>,
    honesty_limit: impl Into<String>,
) -> DetectorCoverageV1 {
    coverage(
        atom,
        detector,
        input_digest,
        DetectorCoverageStateV1::Ran,
        Vec::new(),
        findings,
        BTreeMap::new(),
        honesty_limit,
    )
}

fn unavailable(
    atom: &str,
    detector: &str,
    input_digest: &str,
    missing: &[&str],
    honesty_limit: impl Into<String>,
) -> DetectorCoverageV1 {
    coverage(
        atom,
        detector,
        input_digest,
        DetectorCoverageStateV1::Unavailable,
        missing.iter().map(|value| (*value).into()).collect(),
        Vec::new(),
        BTreeMap::new(),
        honesty_limit,
    )
}

fn has_kind(observations: &[ExecutionObservationV1], kind: ExecutionObservationKindV1) -> bool {
    observations
        .iter()
        .any(|observation| observation.observation_kind == kind)
}

fn progress_event(kind: ExecutionObservationKindV1) -> bool {
    matches!(
        kind,
        ExecutionObservationKindV1::ToolResult
            | ExecutionObservationKindV1::WriteEdit
            | ExecutionObservationKindV1::VerificationResult
            | ExecutionObservationKindV1::SubagentFinished
            | ExecutionObservationKindV1::ArtifactProduced
            | ExecutionObservationKindV1::CompletionAccepted
    )
}

fn duplicate_assignment(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    let workers: BTreeSet<_> = observations
        .iter()
        .filter_map(|observation| exact(&observation.agent_id).cloned())
        .collect();
    if workers.len() <= 1
        && observations
            .iter()
            .all(|observation| exact(&observation.agent_id).is_some())
    {
        return ran(
            "ADP-043",
            "duplicate_assignment_execution",
            input_digest,
            Vec::new(),
            "One exact worker identity is present in this task window, so duplicate worker execution is not observed. H4 still has no independent assignment identity for multi-worker windows.",
        );
    }
    unavailable(
        "ADP-043",
        "duplicate_assignment_execution",
        input_digest,
        &["assignment_id"],
        "H4 exposes task and worker identities but not an independent assignment identity; multi-worker execution cannot be classified as duplicate assignment execution.",
    )
}

fn orchestrator_leakage(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    let mut role_missing = false;
    let mut orchestrator_seen = false;
    for observation in observations {
        match exact(&observation.agent_role) {
            Some(role) if role.eq_ignore_ascii_case("orchestrator") => orchestrator_seen = true,
            Some(_) => {}
            None => role_missing = true,
        }
    }
    if role_missing {
        return unavailable(
            "ADP-044",
            "orchestrator_role_leakage",
            input_digest,
            &["agent_role"],
            "At least one execution observation lacks an exact agent role.",
        );
    }
    if !orchestrator_seen {
        return ran(
            "ADP-044",
            "orchestrator_role_leakage",
            input_digest,
            Vec::new(),
            "No observation in this window identifies an orchestrator role; no orchestrator leakage finding is emitted.",
        );
    }
    unavailable(
        "ADP-044",
        "orchestrator_role_leakage",
        input_digest,
        &["lane_owner_identity", "lane_execution_boundary"],
        "An orchestrator is observable, but H4 does not declare which execution is lane-owned, so role leakage cannot be inferred from role labels alone.",
    )
}

fn lane_scope_overlap(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    let accepted = observations
        .iter()
        .filter(|observation| {
            observation.observation_kind == ExecutionObservationKindV1::CompletionAccepted
        })
        .count();
    if accepted <= 1 {
        return ran(
            "ADP-045",
            "lane_scope_overlap",
            input_digest,
            Vec::new(),
            "At most one accepted completion is present in this window; overlapping accepted lane work is not observed.",
        );
    }
    unavailable(
        "ADP-045",
        "lane_scope_overlap",
        input_digest,
        &["lane_id", "accepted_work_scope"],
        "H4 can report accepted completions and task scope, but it cannot bind accepted work to independent lane identities and lane-owned scopes.",
    )
}

fn fanout_without_value(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    let starts = observations
        .iter()
        .filter(|observation| {
            observation.observation_kind == ExecutionObservationKindV1::SubagentStarted
        })
        .count();
    if starts < 2 {
        return ran(
            "ADP-048",
            "fanout_without_incremental_value",
            input_digest,
            Vec::new(),
            "Fewer than two subagent starts are present, so a fanout-without-value condition is not observable in this window.",
        );
    }
    unavailable(
        "ADP-048",
        "fanout_without_incremental_value",
        input_digest,
        &["incremental_accepted_value_identity"],
        "Fanout is observable, but H4 has no exact identity for incremental accepted value contributed by each worker.",
    )
}

fn context_duplication(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    let starts = observations
        .iter()
        .filter(|observation| {
            observation.observation_kind == ExecutionObservationKindV1::SubagentStarted
        })
        .count();
    if starts < 2 {
        return ran(
            "ADP-049",
            "subagent_context_duplication",
            input_digest,
            Vec::new(),
            "Fewer than two subagent starts are present; cross-subagent context duplication is not observable.",
        );
    }
    unavailable(
        "ADP-049",
        "subagent_context_duplication",
        input_digest,
        &["subagent_context_digest"],
        "H4 does not carry the exact context identity or digest loaded by each subagent.",
    )
}

fn replay_amplification(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    let retrievals = observations
        .iter()
        .filter(|observation| {
            observation.observation_kind == ExecutionObservationKindV1::MembraneRetrieval
        })
        .count();
    if retrievals < 2 {
        return ran(
            "ADP-050",
            "context_replay_amplification",
            input_digest,
            Vec::new(),
            "Fewer than two Membrane retrievals are present; replay amplification is not observed.",
        );
    }
    unavailable(
        "ADP-050",
        "context_replay_amplification",
        input_digest,
        &["context_replay_digest", "replay_size"],
        "Repeated retrievals are observable, but H4 cannot prove that the same context bytes/tokens were replayed.",
    )
}

fn cold_cache_rebuild(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    let cache_write_seen = observations.iter().any(|observation| {
        exact(&observation.usage)
            .and_then(|usage| exact(&usage.cache_write_input_tokens))
            .is_some_and(|value| *value > 0)
    });
    if !cache_write_seen {
        return unavailable(
            "ADP-051",
            "cold_cache_rebuild_waste",
            input_digest,
            &["cache_coverage", "cache_key", "cache_rebuild_identity"],
            "No exact cache-write rebuild can be established for the full window; H4 also lacks cache-key identity needed to classify repeated rebuild waste.",
        );
    }
    unavailable(
        "ADP-051",
        "cold_cache_rebuild_waste",
        input_digest,
        &["cache_key", "cache_rebuild_identity"],
        "Cache-write usage is observable, but H4 does not identify the cache key or semantic rebuild target.",
    )
}

fn no_progress_model_loop(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    const FAILURE_THRESHOLD: usize = 3;
    let mut by_model: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut findings = Vec::new();

    for observation in observations {
        if progress_event(observation.observation_kind) {
            by_model.clear();
            continue;
        }
        if observation.observation_kind != ExecutionObservationKindV1::ModelCallFailed {
            continue;
        }
        let failures = by_model.entry(observation.model.clone()).or_default();
        failures.push(observation.observation_id.clone());
        if failures.len() == FAILURE_THRESHOLD {
            findings.push(json!({
                "finding":"no_progress_model_loop",
                "model":observation.model,
                "failure_threshold":FAILURE_THRESHOLD,
                "observation_ids":failures,
            }));
        }
    }

    ran(
        "ADP-053",
        "no_progress_model_loop",
        input_digest,
        findings,
        "v1 emits only after three exact ModelCallFailed observations for the same model without an intervening H4 progress-bearing event. It does not infer semantic stagnation from prose.",
    )
}

fn duplicate_tool_work(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    let mut first: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut findings = Vec::new();
    for observation in observations.iter().filter(|observation| {
        observation.observation_kind == ExecutionObservationKindV1::ToolCall
    }) {
        let Some(tool) = exact(&observation.tool) else {
            return unavailable(
                "ADP-054",
                "duplicate_tool_work",
                input_digest,
                &["tool"],
                "A ToolCall lacks an exact tool identity, so exact duplicate work cannot be classified for the complete window.",
            );
        };
        let Some(subject) = exact(&observation.subject_id) else {
            return unavailable(
                "ADP-054",
                "duplicate_tool_work",
                input_digest,
                &["subject_id"],
                "A ToolCall lacks an exact subject identity, so exact duplicate work cannot be classified for the complete window.",
            );
        };
        let key = (tool.clone(), subject.clone());
        if let Some(first_id) = first.get(&key) {
            findings.push(json!({
                "finding":"duplicate_tool_work",
                "tool":tool,
                "subject_id":subject,
                "first_observation_id":first_id,
                "repeated_observation_id":observation.observation_id,
                "call_id":exact(&observation.call_id),
            }));
        } else {
            first.insert(key, observation.observation_id.clone());
        }
    }
    ran(
        "ADP-054",
        "duplicate_tool_work",
        input_digest,
        findings,
        "A finding means the same exact H4 tool and subject identity was invoked more than once in the window. Necessity, semantic equivalence and waste are not inferred.",
    )
}

fn semantic_tool_overlap(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    let calls = observations
        .iter()
        .filter(|observation| observation.observation_kind == ExecutionObservationKindV1::ToolCall)
        .count();
    if calls <= 1 {
        return ran(
            "ADP-055",
            "semantic_tool_work_overlap",
            input_digest,
            Vec::new(),
            "At most one tool call is present, so pairwise semantic tool-work overlap is not observable.",
        );
    }
    unavailable(
        "ADP-055",
        "semantic_tool_work_overlap",
        input_digest,
        &["semantic_work_digest"],
        "H4 identifies tools and subjects but does not carry a canonical semantic work digest for overlap comparison.",
    )
}

fn oversized_tool_result_replay(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    if !has_kind(observations, ExecutionObservationKindV1::ToolResult) {
        return ran(
            "ADP-056",
            "oversized_tool_result_replay",
            input_digest,
            Vec::new(),
            "No ToolResult is present in this window, so oversized result replay is not observed.",
        );
    }
    unavailable(
        "ADP-056",
        "oversized_tool_result_replay",
        input_digest,
        &["tool_result_size_bytes", "replay_identity"],
        "H4 has ToolResult identity but not exact result size or a replay-content digest.",
    )
}

fn complete_cost(cost: &ObservedFieldV1<ExecutionCostV1>) -> Option<(u64, String, String)> {
    let cost = exact(cost)?;
    Some((
        *exact(&cost.amount)?,
        exact(&cost.unit)?.clone(),
        exact(&cost.basis)?.clone(),
    ))
}

fn retry_loop_cost(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    let retries: Vec<_> = observations
        .iter()
        .filter(|observation| observation.observation_kind == ExecutionObservationKindV1::Retry)
        .collect();
    if retries.len() < 2 {
        return ran(
            "ADP-057",
            "retry_loop_cost",
            input_digest,
            Vec::new(),
            "Fewer than two Retry observations are present; no retry loop is observed.",
        );
    }

    let mut costs: BTreeMap<(String, String), u64> = BTreeMap::new();
    for retry in &retries {
        let row_costs: Vec<_> = [complete_cost(&retry.tool_cost), complete_cost(&retry.asset_cost)]
            .into_iter()
            .flatten()
            .collect();
        if row_costs.is_empty() {
            return unavailable(
                "ADP-057",
                "retry_loop_cost",
                input_digest,
                &["retry_cost.amount", "retry_cost.unit", "retry_cost.basis"],
                "A retry loop is observable, but at least one Retry event lacks exact cost evidence. Missing cost is not treated as zero.",
            );
        }
        for (amount, unit, basis) in row_costs {
            let entry = costs.entry((unit, basis)).or_insert(0);
            let Some(total) = entry.checked_add(amount) else {
                return coverage(
                    "ADP-057",
                    "retry_loop_cost",
                    input_digest,
                    DetectorCoverageStateV1::Failed,
                    Vec::new(),
                    Vec::new(),
                    BTreeMap::new(),
                    "Qualified retry cost overflowed u64; no cost finding is emitted.",
                );
            };
            *entry = total;
        }
    }
    let cost_rows: Vec<_> = costs
        .into_iter()
        .map(|((unit, basis), amount)| json!({"amount":amount,"unit":unit,"basis":basis}))
        .collect();
    ran(
        "ADP-057",
        "retry_loop_cost",
        input_digest,
        vec![json!({
            "finding":"retry_loop_cost",
            "retry_count":retries.len(),
            "retry_observation_ids":retries.iter().map(|row| row.observation_id.clone()).collect::<Vec<_>>(),
            "qualified_costs":cost_rows,
        })],
        "Costs are summed only inside identical explicit unit+basis buckets. Missing costs are never zero-filled and different bases are never co-aggregated.",
    )
}

fn verification_churn(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    const START_THRESHOLD: usize = 3;
    let mut starts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for observation in observations.iter().filter(|observation| {
        observation.observation_kind == ExecutionObservationKindV1::VerificationStarted
    }) {
        let identity = exact(&observation.call_id)
            .or_else(|| exact(&observation.subject_id))
            .cloned();
        let Some(identity) = identity else {
            return unavailable(
                "ADP-058",
                "verification_churn",
                input_digest,
                &["verification_identity"],
                "A VerificationStarted event lacks both exact call and subject identity.",
            );
        };
        starts
            .entry(identity)
            .or_default()
            .push(observation.observation_id.clone());
    }
    let findings = starts
        .into_iter()
        .filter(|(_, ids)| ids.len() >= START_THRESHOLD)
        .map(|(identity, ids)| {
            json!({
                "finding":"verification_churn",
                "verification_identity":identity,
                "start_count":ids.len(),
                "observation_ids":ids,
                "start_threshold":START_THRESHOLD,
            })
        })
        .collect();
    ran(
        "ADP-058",
        "verification_churn",
        input_digest,
        findings,
        "v1 reports three or more starts for the same exact call/subject identity. It does not infer whether repeated verification was justified.",
    )
}

fn replan_churn(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    const REVISION_THRESHOLD: usize = 3;
    let mut segment = Vec::new();
    let mut findings = Vec::new();
    let flush = |segment: &mut Vec<String>, findings: &mut Vec<Value>| {
        if segment.len() >= REVISION_THRESHOLD {
            findings.push(json!({
                "finding":"replan_churn",
                "revision_count":segment.len(),
                "observation_ids":segment.clone(),
                "revision_threshold":REVISION_THRESHOLD,
            }));
        }
        segment.clear();
    };
    for observation in observations {
        if progress_event(observation.observation_kind) {
            flush(&mut segment, &mut findings);
        } else if observation.observation_kind == ExecutionObservationKindV1::PlanRevised {
            segment.push(observation.observation_id.clone());
        }
    }
    flush(&mut segment, &mut findings);
    ran(
        "ADP-059",
        "replan_churn",
        input_digest,
        findings,
        "v1 reports three or more PlanRevised observations without an intervening H4 progress-bearing event; semantic plan quality is not inferred.",
    )
}

fn routing_cost_mismatch(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    if !has_kind(observations, ExecutionObservationKindV1::RouteSelected) {
        return ran(
            "ADP-060",
            "routing_cost_mismatch",
            input_digest,
            Vec::new(),
            "No RouteSelected event is present, so no routing-cost mismatch is observed in this window.",
        );
    }
    unavailable(
        "ADP-060",
        "routing_cost_mismatch",
        input_digest,
        &["declared_route_cost_expectation"],
        "H4 records the selected route and observed costs but not the expected/allowed route-cost baseline needed to classify a mismatch.",
    )
}

fn stranded_worker_work(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    let mut open: BTreeMap<String, String> = BTreeMap::new();
    let mut findings = Vec::new();
    for observation in observations {
        match observation.observation_kind {
            ExecutionObservationKindV1::SubagentStarted => {
                let Some(agent) = exact(&observation.agent_id) else {
                    return unavailable(
                        "ADP-062",
                        "stranded_worker_work",
                        input_digest,
                        &["subagent_identity"],
                        "A SubagentStarted event lacks exact agent identity.",
                    );
                };
                open.insert(agent.clone(), observation.observation_id.clone());
            }
            ExecutionObservationKindV1::SubagentFinished => {
                let Some(agent) = exact(&observation.agent_id) else {
                    return unavailable(
                        "ADP-062",
                        "stranded_worker_work",
                        input_digest,
                        &["subagent_identity"],
                        "A SubagentFinished event lacks exact agent identity.",
                    );
                };
                open.remove(agent);
            }
            ExecutionObservationKindV1::CompletionAccepted
            | ExecutionObservationKindV1::CompletionRejected
            | ExecutionObservationKindV1::Cancellation
            | ExecutionObservationKindV1::Timeout => {
                if !open.is_empty() {
                    findings.push(json!({
                        "finding":"stranded_worker_work",
                        "terminal_observation_id":observation.observation_id,
                        "open_workers":open.iter().map(|(agent,start)| json!({"agent_id":agent,"start_observation_id":start})).collect::<Vec<_>>(),
                    }));
                }
            }
            _ => {}
        }
    }
    ran(
        "ADP-062",
        "stranded_worker_work",
        input_digest,
        findings,
        "A finding means a terminal task event was observed while an exact started subagent identity had no matching SubagentFinished event in the bounded window. Work value or cancellation cause is not inferred.",
    )
}

fn qualified_reporting(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> DetectorCoverageV1 {
    let mut metrics = BTreeMap::new();
    metrics.insert("event_count".into(), json!(observations.len()));
    if let (Some(first), Some(last)) = (observations.first(), observations.last()) {
        metrics.insert(
            "window_span_ms".into(),
            json!(last.observed_at_unix_ms.saturating_sub(first.observed_at_unix_ms)),
        );
    }

    let mut duration_total = 0_u64;
    let mut duration_complete = 0_u64;
    let mut duration_unavailable = 0_u64;
    let mut token_totals: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut cost_totals: BTreeMap<(String, String), u64> = BTreeMap::new();

    for observation in observations {
        if let Some(value) = exact(&observation.duration_ms) {
            duration_complete += 1;
            duration_total = duration_total.saturating_add(*value);
        } else {
            duration_unavailable += 1;
        }
        if let Some(usage) = exact(&observation.usage) {
            for (label, estimate) in [
                ("input", &usage.input_tokens),
                ("output", &usage.output_tokens),
            ] {
                if let Some(value) = exact(&estimate.estimate) {
                    let key = (
                        format!("{}:{}", estimate.basis.id, estimate.basis.version),
                        label.into(),
                    );
                    let entry = token_totals.entry(key).or_insert(0);
                    *entry = entry.saturating_add(*value);
                }
            }
        }
        for cost in [&observation.tool_cost, &observation.asset_cost] {
            if let Some((amount, unit, basis)) = complete_cost(cost) {
                let entry = cost_totals.entry((unit, basis)).or_insert(0);
                *entry = entry.saturating_add(amount);
            }
        }
    }

    metrics.insert(
        "duration".into(),
        json!({
            "qualified_total_ms":duration_total,
            "complete_event_count":duration_complete,
            "unavailable_event_count":duration_unavailable,
        }),
    );
    metrics.insert(
        "token_totals".into(),
        json!(token_totals
            .into_iter()
            .map(|((basis,direction),amount)| json!({"basis":basis,"direction":direction,"amount":amount}))
            .collect::<Vec<_>>()),
    );
    metrics.insert(
        "cost_totals".into(),
        json!(cost_totals
            .into_iter()
            .map(|((unit,basis),amount)| json!({"unit":unit,"basis":basis,"amount":amount}))
            .collect::<Vec<_>>()),
    );

    coverage(
        "ADP-064",
        "qualified_execution_reporting",
        input_digest,
        DetectorCoverageStateV1::Ran,
        vec!["assignment_id".into()],
        Vec::new(),
        metrics,
        "Only exact H4 facts are aggregated. Token estimates remain separated by estimator basis and direction; costs remain separated by unit+basis. H4 does not expose an independent assignment id, so these are task-window metrics until the host supplies one.",
    )
}

/// Execute every committed efficiency detector for one validated H4 window.
pub fn analyze_efficiency(
    observations: &[ExecutionObservationV1],
    input_digest: &str,
) -> Vec<DetectorCoverageV1> {
    vec![
        duplicate_assignment(observations, input_digest),
        orchestrator_leakage(observations, input_digest),
        lane_scope_overlap(observations, input_digest),
        unavailable(
            "ADP-046",
            "bounded_lane_budget_exceeded",
            input_digest,
            &["declared_lane_budget"],
            "H4 carries qualified usage/cost facts but no declared lane budget to compare them against.",
        ),
        unavailable(
            "ADP-047",
            "missing_efficiency_budget",
            input_digest,
            &["required_efficiency_budget_contract"],
            "The current host observation contract cannot distinguish a genuinely absent required budget from a budget that simply was not instrumented.",
        ),
        fanout_without_value(observations, input_digest),
        context_duplication(observations, input_digest),
        replay_amplification(observations, input_digest),
        cold_cache_rebuild(observations, input_digest),
        unavailable(
            "ADP-052",
            "cache_invalidation_churn",
            input_digest,
            &["cache_key", "cache_invalidation_event"],
            "H4 has cache token counters but no cache invalidation event or cache-key identity.",
        ),
        no_progress_model_loop(observations, input_digest),
        duplicate_tool_work(observations, input_digest),
        semantic_tool_overlap(observations, input_digest),
        oversized_tool_result_replay(observations, input_digest),
        retry_loop_cost(observations, input_digest),
        verification_churn(observations, input_digest),
        replan_churn(observations, input_digest),
        routing_cost_mismatch(observations, input_digest),
        unavailable(
            "ADP-061",
            "integration_rework_from_lane_failure",
            input_digest,
            &["lane_failure_causal_link", "integration_rework_identity"],
            "H4 cannot express a sealed causal link from a lane failure to later integration rework.",
        ),
        stranded_worker_work(observations, input_digest),
        unavailable(
            "ADP-063",
            "background_learning_over_budget",
            input_digest,
            &["background_learning_identity", "background_learning_budget"],
            "H4 does not provide a frozen background-learning identity/budget contract, so ordinary execution cost cannot be relabeled as background learner spend.",
        ),
        qualified_reporting(observations, input_digest),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::host_observation::{
        HostObservationProvenanceV1, ObservationUnavailableReasonV1,
    };

    const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn unavailable_field<T>() -> ObservedFieldV1<T> {
        ObservedFieldV1::unavailable(ObservationUnavailableReasonV1::HostUnsupported)
    }

    fn observation(id: &str, kind: ExecutionObservationKindV1) -> ExecutionObservationV1 {
        ExecutionObservationV1 {
            schema_version: HOST_OBSERVATION_SCHEMA_VERSION,
            observation_id: id.into(),
            session_id: "session".into(),
            task_id: ObservedFieldV1::complete("task".into()),
            parent_task_id: unavailable_field(),
            agent_id: ObservedFieldV1::complete("worker".into()),
            agent_role: ObservedFieldV1::complete("worker".into()),
            observed_at_unix_ms: 1,
            model: "model".into(),
            provider: "provider".into(),
            client: "client".into(),
            route_policy: unavailable_field(),
            observation_kind: kind,
            subject_id: unavailable_field(),
            tool: unavailable_field(),
            call_id: unavailable_field(),
            outcome: unavailable_field(),
            exit_code: unavailable_field(),
            duration_ms: unavailable_field(),
            usage: unavailable_field(),
            tool_cost: unavailable_field(),
            asset_cost: unavailable_field(),
            repository: unavailable_field(),
            scope: ObservedFieldV1::complete("scope".into()),
            artifact_refs: unavailable_field(),
            evidence_refs: unavailable_field(),
            completion: unavailable_field(),
            provenance_receipt: HostObservationProvenanceV1::new("receipt", "host", 1, HASH),
        }
    }

    #[test]
    fn committed_efficiency_catalog_is_atomic_and_versioned() {
        let catalog = detector_catalog();
        assert_eq!(catalog.len(), 22);
        let atoms: BTreeSet<_> = catalog.iter().map(|row| row.atom_id.as_str()).collect();
        assert_eq!(atoms.len(), 22);
        assert!(atoms.contains("ADP-043"));
        assert!(atoms.contains("ADP-064"));
        assert!(catalog.iter().all(|row| row.detector_version == 1));
    }

    #[test]
    fn duplicate_tool_detector_requires_exact_subject_and_then_reports_repetition() {
        let mut first = observation("tool-1", ExecutionObservationKindV1::ToolCall);
        first.tool = ObservedFieldV1::complete("read".into());
        let unavailable = duplicate_tool_work(&[first.clone()], "digest");
        assert_eq!(unavailable.state, DetectorCoverageStateV1::Unavailable);
        assert_eq!(unavailable.missing_fields, vec!["subject_id"]);

        first.subject_id = ObservedFieldV1::complete("file:a".into());
        let mut second = first.clone();
        second.observation_id = "tool-2".into();
        let ran = duplicate_tool_work(&[first, second], "digest");
        assert_eq!(ran.state, DetectorCoverageStateV1::Ran);
        assert_eq!(ran.findings.len(), 1);
    }

    #[test]
    fn detector_family_reports_contract_gaps_instead_of_clean_behavior() {
        let rows = analyze_efficiency(
            &[observation(
                "route",
                ExecutionObservationKindV1::RouteSelected,
            )],
            "digest",
        );
        let routing = rows
            .iter()
            .find(|row| row.atom_id == "ADP-060")
            .expect("routing detector");
        assert_eq!(routing.state, DetectorCoverageStateV1::Unavailable);
        assert!(routing
            .missing_fields
            .contains(&"declared_route_cost_expectation".to_string()));
    }
}
