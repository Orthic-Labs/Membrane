//! Native, content-free qualification fixtures for Adapt efficiency detectors.
//!
//! These fixtures exercise the H4 ingress boundary itself.  They are deliberately
//! separate from detector implementation tests: every ADP-043..064 row has its
//! own positive event shape, typed fields, semantic assertion, and near-miss
//! (missing-field) control.  Output contains only identities, counts, and a
//! digest; host payloads never leave this module.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const H4_EXECUTION_SCHEMA_V1: &str = "coderight.execution-observation.v1";
const H4_SOURCE: &str = "coderight";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct H4EventV1 {
    schema_version: String,
    event_id: String,
    observed_at_unix_ms: u64,
    kind: String,
    attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct H4FixtureV1 {
    schema_version: String,
    source: String,
    events: Vec<H4EventV1>,
}

fn event(index: usize, kind: &str, attributes: Value) -> Value {
    json!({
        "schemaVersion": H4_EXECUTION_SCHEMA_V1,
        "eventId": format!("h4-qualification-{index}"),
        "observedAtUnixMs": 1_700_000_000_000_u64 + index as u64,
        "kind": kind,
        "attributes": attributes,
    })
}

fn fixture(events: Vec<Value>) -> Result<H4FixtureV1, String> {
    serde_json::from_value(json!({
        "schemaVersion": "membrane.adapt-efficiency-qualification.v1",
        "source": H4_SOURCE,
        "events": events,
    }))
    .map_err(|error| format!("invalid native H4 fixture: {error}"))
}

fn required(case_id: &str) -> Option<&'static [&'static str]> {
    Some(match case_id {
        "ADP-043" => &["assignment_id", "worker_id"],
        "ADP-044" => &["agent_role", "lane_owner_identity", "lane_execution_boundary"],
        "ADP-045" => &["lane_id", "accepted_work_scope"],
        "ADP-046" => &["declared_lane_budget", "qualified_usage_or_cost"],
        "ADP-047" => &["required_efficiency_budget_contract"],
        "ADP-048" => &["subagent_identity", "incremental_accepted_value_identity"],
        "ADP-049" => &["subagent_context_digest"],
        "ADP-050" => &["context_replay_digest", "replay_size"],
        "ADP-051" => &["cache_key", "cache_rebuild_identity"],
        "ADP-052" => &["cache_key", "cache_invalidation_event"],
        "ADP-053" => &["model_call_failure", "progress_event"],
        "ADP-054" => &["tool", "subject_id"],
        "ADP-055" => &["semantic_work_digest"],
        "ADP-056" => &["tool_result_size_bytes", "replay_identity"],
        "ADP-057" => &["retry_event", "qualified_cost"],
        "ADP-058" => &["verification_identity"],
        "ADP-059" => &["plan_revision", "progress_event"],
        "ADP-060" => &["route_policy", "declared_route_cost_expectation"],
        "ADP-061" => &["lane_failure_causal_link", "integration_rework_identity"],
        "ADP-062" => &["subagent_identity", "terminal_task_event"],
        "ADP-063" => &["background_learning_identity", "background_learning_budget"],
        "ADP-064" => &["execution_observations"],
        _ => return None,
    })
}

fn wire_name(field: &str) -> String {
    let mut output = String::with_capacity(field.len());
    let mut uppercase = false;
    for character in field.chars() {
        if character == '_' {
            uppercase = true;
        } else if uppercase {
            output.extend(character.to_uppercase());
            uppercase = false;
        } else {
            output.push(character);
        }
    }
    output
}

fn field<'a>(event: &'a H4EventV1, name: &str) -> Result<&'a Value, String> {
    let wire = wire_name(name);
    let value = event
        .attributes
        .get(&wire)
        .ok_or_else(|| format!("{name} missing from {}", event.event_id))?;
    if value.is_null() {
        return Err(format!("{name} is null in {}", event.event_id));
    }
    Ok(value)
}

fn string<'a>(event: &'a H4EventV1, name: &str) -> Result<&'a str, String> {
    let value = field(event, name)?;
    value
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| format!("{name} is not an exact nonempty string"))
}

fn object<'a>(event: &'a H4EventV1, name: &str) -> Result<&'a Map<String, Value>, String> {
    field(event, name)?
        .as_object()
        .ok_or_else(|| format!("{name} is not an exact object"))
}

fn array<'a>(event: &'a H4EventV1, name: &str) -> Result<&'a Vec<Value>, String> {
    field(event, name)?
        .as_array()
        .filter(|values| !values.is_empty())
        .ok_or_else(|| format!("{name} is not an exact nonempty array"))
}

fn object_string<'a>(value: &'a Map<String, Value>, name: &str) -> Result<&'a str, String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| format!("object field {name} is not exact"))
}

fn object_u64(value: &Map<String, Value>, name: &str) -> Result<u64, String> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("object field {name} is not exact"))
}

fn common(fixture: &H4FixtureV1, case_id: &str) -> Result<(), String> {
    if fixture.schema_version != "membrane.adapt-efficiency-qualification.v1" {
        return Err("fixture schema version mismatch".into());
    }
    if fixture.source != H4_SOURCE || fixture.events.is_empty() {
        return Err("fixture source or event set is invalid".into());
    }
    let required = required(case_id).ok_or_else(|| format!("unsupported case {case_id}"))?;
    for (index, event) in fixture.events.iter().enumerate() {
        if event.schema_version != H4_EXECUTION_SCHEMA_V1
            || event.event_id.trim().is_empty()
            || event.kind.trim().is_empty()
            || event.observed_at_unix_ms == 0
        {
            return Err(format!("H4 event {index} identity/schema is invalid"));
        }
    }
    let present: BTreeSet<String> = fixture
        .events
        .iter()
        .flat_map(|event| event.attributes.keys().cloned())
        .collect();
    for name in required {
        if !present.contains(&wire_name(name)) {
            return Err(format!("required exact field {name} is absent"));
        }
    }
    Ok(())
}

fn strings_intersect(left: &[Value], right: &[Value]) -> bool {
    let left: BTreeSet<&str> = left.iter().filter_map(Value::as_str).collect();
    right.iter().filter_map(Value::as_str).any(|value| left.contains(value))
}

fn semantic(case_id: &str, fixture: &H4FixtureV1) -> Result<bool, String> {
    let events = &fixture.events;
    match case_id {
        "ADP-043" => {
            let mut assignments: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
            for event in events {
                assignments
                    .entry(string(event, "assignment_id")?)
                    .or_default()
                    .insert(string(event, "worker_id")?);
            }
            Ok(assignments.values().any(|workers| workers.len() >= 2))
        }
        "ADP-044" => {
            let mut leakage = false;
            for event in events {
                let role = string(event, "agent_role")?;
                let owner = string(event, "lane_owner_identity")?;
                let boundary = string(event, "lane_execution_boundary")?;
                leakage |= role.eq_ignore_ascii_case("orchestrator")
                    && boundary == "lane-owned"
                    && owner != role;
            }
            Ok(leakage)
        }
        "ADP-045" => {
            let accepted: Vec<_> = events
                .iter()
                .filter(|event| event.kind == "completion_accepted")
                .collect();
            for (index, left) in accepted.iter().enumerate() {
                let left_lane = string(left, "lane_id")?;
                let left_scope = array(left, "accepted_work_scope")?;
                for right in accepted.iter().skip(index + 1) {
                    if left_lane == string(right, "lane_id")?
                        && strings_intersect(left_scope, array(right, "accepted_work_scope")?)
                    {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        "ADP-046" => {
            let event = &events[0];
            let limit = object(event, "declared_lane_budget")?;
            let usage = object(event, "qualified_usage_or_cost")?;
            Ok(object_u64(limit, "amount")? < object_u64(usage, "amount")?
                && object_string(limit, "unit")? == object_string(usage, "unit")?
                && object_string(limit, "basis")? == object_string(usage, "basis")?)
        }
        "ADP-047" => {
            let contract = object(&events[0], "required_efficiency_budget_contract")?;
            Ok(contract.get("required").and_then(Value::as_bool) == Some(true)
                && contract.get("budgetPresent").and_then(Value::as_bool) == Some(false)
                && object_string(contract, "contractId").is_ok())
        }
        "ADP-048" => {
            let mut values = BTreeSet::new();
            for event in events {
                string(event, "subagent_identity")?;
                values.insert(string(event, "incremental_accepted_value_identity")?);
            }
            Ok(events.len() >= 2 && values.len() == 1)
        }
        "ADP-049" => {
            let digests: BTreeSet<_> = events
                .iter()
                .map(|event| string(event, "subagent_context_digest"))
                .collect::<Result<_, _>>()?;
            Ok(events.len() >= 2 && digests.len() == 1)
        }
        "ADP-050" => {
            let digests: BTreeSet<_> = events
                .iter()
                .map(|event| string(event, "context_replay_digest"))
                .collect::<Result<_, _>>()?;
            let sizes: BTreeSet<_> = events
                .iter()
                .map(|event| object_u64(object(event, "replay_size")?, "bytes"))
                .collect::<Result<_, _>>()?;
            Ok(events.len() >= 2 && digests.len() == 1 && sizes.len() == 1)
        }
        "ADP-051" => {
            let keys: BTreeSet<_> = events
                .iter()
                .map(|event| string(event, "cache_key"))
                .collect::<Result<_, _>>()?;
            let rebuilds: BTreeSet<_> = events
                .iter()
                .map(|event| string(event, "cache_rebuild_identity"))
                .collect::<Result<_, _>>()?;
            Ok(events.len() >= 2 && keys.len() == 1 && rebuilds.len() == 1)
        }
        "ADP-052" => {
            let key = string(&events[0], "cache_key")?;
            let invalidations: BTreeSet<_> = events
                .iter()
                .map(|event| {
                    if string(event, "cache_key")? != key {
                        return Err("cache invalidation keys differ".into());
                    }
                    string(event, "cache_invalidation_event")
                })
                .collect::<Result<_, String>>()?;
            Ok(invalidations.len() >= 2)
        }
        "ADP-053" => {
            let model = string(&events[0], "model")?;
            if events.len() < 3 {
                return Ok(false);
            }
            for event in events {
                if string(event, "model")? != model
                    || field(event, "model_call_failure")?.as_bool() != Some(true)
                    || field(event, "progress_event")?.as_bool() != Some(false)
                {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        "ADP-054" => {
            let identities: BTreeSet<_> = events
                .iter()
                .map(|event| Ok((string(event, "tool")?, string(event, "subject_id")?)))
                .collect::<Result<_, String>>()?;
            Ok(events.len() >= 2 && identities.len() == 1)
        }
        "ADP-055" => {
            let digests: BTreeSet<_> = events
                .iter()
                .map(|event| string(event, "semantic_work_digest"))
                .collect::<Result<_, _>>()?;
            Ok(events.len() >= 2 && digests.len() == 1)
        }
        "ADP-056" => {
            let identities: BTreeSet<_> = events
                .iter()
                .map(|event| string(event, "replay_identity"))
                .collect::<Result<_, _>>()?;
            let sizes: Vec<_> = events
                .iter()
                .map(|event| field(event, "tool_result_size_bytes")?.as_u64().ok_or_else(|| "tool result size is not exact".into()))
                .collect::<Result<_, String>>()?;
            Ok(events.len() >= 2 && identities.len() == 1 && sizes.iter().all(|size| *size >= 4096))
        }
        "ADP-057" => {
            let mut buckets = BTreeSet::new();
            for event in events {
                if field(event, "retry_event")?.as_bool() != Some(true) {
                    return Ok(false);
                }
                let cost = object(event, "qualified_cost")?;
                buckets.insert((
                    object_u64(cost, "amount")?,
                    object_string(cost, "unit")?.to_owned(),
                    object_string(cost, "basis")?.to_owned(),
                ));
            }
            Ok(events.len() >= 2 && buckets.len() == 1)
        }
        "ADP-058" => {
            let identities: BTreeSet<_> = events
                .iter()
                .map(|event| string(event, "verification_identity"))
                .collect::<Result<_, _>>()?;
            Ok(events.len() >= 3 && identities.len() == 1)
        }
        "ADP-059" => {
            let revisions: BTreeSet<_> = events
                .iter()
                .map(|event| string(event, "plan_revision"))
                .collect::<Result<_, _>>()?;
            Ok(events.len() >= 3
                && revisions.len() == events.len()
                && events.iter().all(|event| {
                    field(event, "progress_event").ok().and_then(Value::as_bool) == Some(false)
                }))
        }
        "ADP-060" => {
            let selected = object(&events[0], "route_policy")?;
            let expected = object(&events[0], "declared_route_cost_expectation")?;
            Ok(object_string(selected, "route")? != object_string(expected, "route")?
                && object_u64(selected, "cost")? > object_u64(expected, "maxCost")?)
        }
        "ADP-061" => {
            let failure = object(&events[0], "lane_failure_causal_link")?;
            let rework = object(&events[1], "integration_rework_identity")?;
            Ok(object_string(failure, "causalId")? == object_string(rework, "causalId")?
                && object_string(failure, "laneId")? == object_string(rework, "sourceLaneId")?)
        }
        "ADP-062" => {
            string(&events[0], "subagent_identity")?;
            let terminal = string(&events[1], "terminal_task_event")?;
            Ok(events[0].kind == "subagent_started"
                && events[1].kind == "task_terminal"
                && terminal == "completion_accepted")
        }
        "ADP-063" => {
            let identity = object(&events[0], "background_learning_identity")?;
            let budget = object(&events[0], "background_learning_budget")?;
            let budget_amount = object_u64(budget, "amount")?;
            let used = budget
                .get("used")
                .and_then(Value::as_u64)
                .ok_or_else(|| "background learning usage is not exact".to_string())?;
            Ok(object_string(identity, "jobId").is_ok() && used > budget_amount)
        }
        "ADP-064" => {
            let mut ids = BTreeSet::new();
            for event in events {
                let observation = object(event, "execution_observations")?;
                ids.insert(object_string(observation, "observationId")?.to_owned());
                object_u64(observation, "durationMs")?;
                object_u64(observation, "inputTokens")?;
                object_u64(observation, "outputTokens")?;
                object_u64(observation, "costUnits")?;
                object_string(observation, "outcome")?;
            }
            Ok(ids.len() == events.len())
        }
        _ => Err(format!("unsupported case {case_id}")),
    }
}

fn positive_fixture(case_id: &str) -> Result<H4FixtureV1, String> {
    let make_event = |index, kind, value| event(index, kind, value);
    let fixture = match case_id {
        "ADP-043" => fixture(vec![
            make_event(0, "subagent_started", json!({"assignmentId":"assignment-17","workerId":"worker-a"})),
            make_event(1, "subagent_started", json!({"assignmentId":"assignment-17","workerId":"worker-b"})),
        ]),
        "ADP-044" => fixture(vec![make_event(0, "lane_execution", json!({"agentRole":"orchestrator","laneOwnerIdentity":"worker-a","laneExecutionBoundary":"lane-owned"}))]),
        "ADP-045" => fixture(vec![
            make_event(0, "completion_accepted", json!({"laneId":"lane-a","acceptedWorkScope":["src/a.rs","src/shared.rs"]})),
            make_event(1, "completion_accepted", json!({"laneId":"lane-a","acceptedWorkScope":["src/shared.rs","src/b.rs"]})),
        ]),
        "ADP-046" => fixture(vec![make_event(0, "lane_usage", json!({"declaredLaneBudget":{"amount":100,"unit":"tokens","basis":"task"},"qualifiedUsageOrCost":{"amount":125,"unit":"tokens","basis":"task"}}))]),
        "ADP-047" => fixture(vec![make_event(0, "budget_check", json!({"requiredEfficiencyBudgetContract":{"contractId":"efficiency-budget-v1","required":true,"budgetPresent":false}}))]),
        "ADP-048" => fixture(vec![
            make_event(0, "subagent_finished", json!({"subagentIdentity":"worker-a","incrementalAcceptedValueIdentity":"value-1"})),
            make_event(1, "subagent_finished", json!({"subagentIdentity":"worker-b","incrementalAcceptedValueIdentity":"value-1"})),
        ]),
        "ADP-049" => fixture(vec![
            make_event(0, "subagent_started", json!({"subagentContextDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})),
            make_event(1, "subagent_started", json!({"subagentContextDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})),
        ]),
        "ADP-050" => fixture(vec![
            make_event(0, "context_retrieval", json!({"contextReplayDigest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","replaySize":{"bytes":8192}})),
            make_event(1, "context_retrieval", json!({"contextReplayDigest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","replaySize":{"bytes":8192}})),
        ]),
        "ADP-051" => fixture(vec![
            make_event(0, "cache_rebuild", json!({"cacheKey":"cache:blueprint","cacheRebuildIdentity":"rebuild:42"})),
            make_event(1, "cache_rebuild", json!({"cacheKey":"cache:blueprint","cacheRebuildIdentity":"rebuild:42"})),
        ]),
        "ADP-052" => fixture(vec![
            make_event(0, "cache_invalidated", json!({"cacheKey":"cache:blueprint","cacheInvalidationEvent":"invalidate-1"})),
            make_event(1, "cache_invalidated", json!({"cacheKey":"cache:blueprint","cacheInvalidationEvent":"invalidate-2"})),
        ]),
        "ADP-053" => fixture((0..3).map(|index| make_event(index, "model_call_failed", json!({"model":"gpt-5.6-luna","modelCallFailure":true,"progressEvent":false}))).collect()),
        "ADP-054" => fixture(vec![
            make_event(0, "tool_call", json!({"tool":"read_file","subjectId":"src/lib.rs"})),
            make_event(1, "tool_call", json!({"tool":"read_file","subjectId":"src/lib.rs"})),
        ]),
        "ADP-055" => fixture(vec![
            make_event(0, "tool_call", json!({"semanticWorkDigest":"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"})),
            make_event(1, "tool_call", json!({"semanticWorkDigest":"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"})),
        ]),
        "ADP-056" => fixture(vec![
            make_event(0, "tool_result", json!({"toolResultSizeBytes":8192,"replayIdentity":"replay-7"})),
            make_event(1, "tool_result", json!({"toolResultSizeBytes":8192,"replayIdentity":"replay-7"})),
        ]),
        "ADP-057" => fixture(vec![
            make_event(0, "retry", json!({"retryEvent":true,"qualifiedCost":{"amount":7,"unit":"tokens","basis":"model-output"}})),
            make_event(1, "retry", json!({"retryEvent":true,"qualifiedCost":{"amount":7,"unit":"tokens","basis":"model-output"}})),
        ]),
        "ADP-058" => fixture((0..3).map(|index| make_event(index, "verification_started", json!({"verificationIdentity":"verify:build"}))).collect()),
        "ADP-059" => fixture((0..3).map(|index| make_event(index, "plan_revised", json!({"planRevision":format!("revision-{index}"),"progressEvent":false}))).collect()),
        "ADP-060" => fixture(vec![make_event(0, "route_selected", json!({"routePolicy":{"route":"slow","cost":20},"declaredRouteCostExpectation":{"route":"fast","maxCost":10}}))]),
        "ADP-061" => fixture(vec![
            make_event(0, "lane_failed", json!({"laneFailureCausalLink":{"causalId":"cause-3","laneId":"lane-a"}})),
            make_event(1, "integration_rework", json!({"integrationReworkIdentity":{"causalId":"cause-3","sourceLaneId":"lane-a"}})),
        ]),
        "ADP-062" => fixture(vec![
            make_event(0, "subagent_started", json!({"subagentIdentity":"worker-a"})),
            make_event(1, "task_terminal", json!({"terminalTaskEvent":"completion_accepted"})),
        ]),
        "ADP-063" => fixture(vec![make_event(0, "background_learning", json!({"backgroundLearningIdentity":{"jobId":"learn-4"},"backgroundLearningBudget":{"amount":100,"used":125,"unit":"tokens"}}))]),
        "ADP-064" => fixture(vec![
            make_event(0, "execution_report", json!({"executionObservations":{"observationId":"obs-1","durationMs":20,"inputTokens":100,"outputTokens":30,"costUnits":2,"outcome":"completed"}})),
            make_event(1, "execution_report", json!({"executionObservations":{"observationId":"obs-2","durationMs":25,"inputTokens":120,"outputTokens":35,"costUnits":3,"outcome":"completed"}})),
        ]),
        _ => return Err(format!("unsupported case {case_id}")),
    }?;
    Ok(fixture)
}

fn remove_field(fixture: &mut H4FixtureV1, field_name: &str) {
    let wire = wire_name(field_name);
    for event in &mut fixture.events {
        event.attributes.remove(&wire);
    }
}

fn digest(fixture: &H4FixtureV1) -> Result<String, String> {
    let bytes = serde_json::to_vec(fixture).map_err(|error| format!("digest fixture: {error}"))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

/// Execute one native ADP efficiency qualification row.
pub(crate) fn run(case_id: &str) -> Result<Value, String> {
    let required = required(case_id).ok_or_else(|| format!("unsupported case {case_id}"))?;
    let fixture = positive_fixture(case_id)?;
    common(&fixture, case_id)?;
    let finding = semantic(case_id, &fixture)?;
    let mut missing = fixture.clone();
    remove_field(&mut missing, required[0]);
    if common(&missing, case_id).is_ok() || semantic(case_id, &missing).is_ok() {
        return Err(format!("{case_id} missing-field near miss was accepted"));
    }
    let mut wrong_schema = fixture.clone();
    wrong_schema.schema_version = "membrane.adapt-efficiency-qualification.v0".into();
    if common(&wrong_schema, case_id).is_ok() {
        return Err(format!("{case_id} schema near miss was accepted"));
    }
    Ok(json!({
        "schema": "membrane.adapt-efficiency-qualification-result.v1",
        "caseId": case_id,
        "status": "passed",
        "evidenceKind": "native_h4_content_free",
        "fixtureSchema": H4_EXECUTION_SCHEMA_V1,
        "fixtureDigest": digest(&fixture)?,
        "eventCount": fixture.events.len(),
        "requiredFieldCount": required.len(),
        "negativeControlCount": 2,
        "findingObserved": finding,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: [&str; 22] = [
        "ADP-043", "ADP-044", "ADP-045", "ADP-046", "ADP-047", "ADP-048", "ADP-049",
        "ADP-050", "ADP-051", "ADP-052", "ADP-053", "ADP-054", "ADP-055", "ADP-056",
        "ADP-057", "ADP-058", "ADP-059", "ADP-060", "ADP-061", "ADP-062", "ADP-063",
        "ADP-064",
    ];

    #[test]
    fn every_efficiency_row_has_native_h4_evidence() {
        for case_id in CASES {
            let result = run(case_id).unwrap_or_else(|error| panic!("{case_id}: {error}"));
            assert_eq!(result["status"], "passed");
            assert_eq!(result["evidenceKind"], "native_h4_content_free");
            assert_eq!(result["negativeControlCount"], 2);
            assert!(result["fixtureDigest"].as_str().is_some_and(|digest| digest.len() == 64));
            assert!(result.get("events").is_none());
            assert!(result.get("attributes").is_none());
        }
    }

    #[test]
    fn unknown_rows_fail_closed() {
        assert!(run("ADP-042").is_err());
        assert!(run("ADP-065").is_err());
    }

    #[test]
    fn malformed_and_missing_h4_fields_fail_closed() {
        let mut fixture = positive_fixture("ADP-054").unwrap();
        fixture.events[0].attributes.insert("subjectId".into(), Value::Null);
        assert!(common(&fixture, "ADP-054").is_ok());
        assert!(semantic("ADP-054", &fixture).is_err());

        fixture.events[0].schema_version = "coderight.execution-observation.v0".into();
        assert!(common(&fixture, "ADP-054").is_err());
    }
}
