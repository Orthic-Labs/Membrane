//! Native MCP registry. Discovery & execution share this table.
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{Arc, OnceLock},
};

const CORE: &[&str] = &[
    "membrane_context",
    "membrane_source_read",
    "membrane_blueprint",
    "membrane_knowledge_propose",
    "membrane_checkpoint_save",
    "membrane_checkpoint_load",
    "membrane_working_context",
    "membrane_temporal_fact",
    "membrane_scratchpad",
    "membrane_feedback",
    "membrane_memory",
];
const OPERATOR: &[&str] = &["membrane_knowledge_review"];
const DIAGNOSTIC: &[&str] = &[
    "membrane_diagnostic_workspace",
    "membrane_diagnostic_mutation",
    "membrane_diagnostic_snapshot",
    "membrane_diagnostic_fence",
    "membrane_diagnostic_capabilities",
    "membrane_diagnostic_baseline",
    "membrane_diagnostic_provider",
];

fn caller() -> Value {
    json!({"type":"object","required":["root","repositoryId","scopeId"],"properties":{
      "root":{"type":"string","minLength":1},"repositoryId":{"type":"string","minLength":1},
      "scopeId":{"type":"string","minLength":1},"scopeDescriptor":{"type":"object"}
    },"additionalProperties":false})
}
/// RemainingContextCeilingV1, the host's observed remaining context. The
/// runtime requires it on every context request and never substitutes a
/// numeric fallback, so its shape belongs in the advertised schema.
fn remaining_context_ceiling() -> Value {
    json!({
        "type": "object",
        "description": "RemainingContextCeilingV1 (membrane-host-observation): the host's observed remaining context for this session and task. Required; never derived or defaulted by Membrane. Two cross-field bindings are enforced and refuse the request when broken: sessionId must equal caller.scopeId, and taskId.value must equal the request's task. taskId and remainingTokens.estimate must both carry coverage \"complete\", and requestedAtUnixMs and provenanceReceipt.observedAtUnixMs must both be non-zero.",
        "required": [
            "schemaVersion",
            "ceilingId",
            "sessionId",
            "taskId",
            "requestedAtUnixMs",
            "remainingTokens",
            "provenanceReceipt"
        ],
        "properties": {
            "schemaVersion": {"type": "integer", "minimum": 1},
            "ceilingId": {"type": "string", "minLength": 1},
            "sessionId": {"type": "string", "minLength": 1},
            "taskId": {"type": "object", "description": "ObservedFieldV1<String> carrying the task identity this ceiling was observed against"},
            "requestedAtUnixMs": {"type": "integer", "minimum": 0},
            "remainingTokens": {"type": "object", "description": "TokenEstimateV1"},
            "provenanceReceipt": {"type": "object", "description": "HostObservationProvenanceV1"}
        }
    })
}

fn schema(name: &str) -> Value {
    let (required, properties) = match name {
        "membrane_context" => (
            // The runtime refuses every request without remainingContextCeiling
            // (RequestTimeH8Error::Missing), so the tool must advertise it.
            // Leaving it undeclared made the one entry tool impossible to call
            // correctly from its own schema.
            vec!["task", "repository", "caller", "remainingContextCeiling"],
            json!({"task":{"type":"string","minLength":1,"pattern":"\\S"},"repository":{"type":"string"},"caller":caller(),"budget":{"type":"integer","minimum":1},"scope":{"type":"string","enum":["repo","workspace"]},"deadlineMs":{"type":"integer","minimum":1},"sufficiencyContract":{"type":"object","description":"Optional planner-authored SufficiencyContractV1 (membrane-sufficiency-v1); transported verbatim to federate, never derived from task prose"},"remainingContextCeiling":remaining_context_ceiling()}),
        ),
        "membrane_source_read" => (
            vec![
                "repository",
                "caller",
                "sourceRef",
                "anchorId",
                "expectedContentHash",
            ],
            json!({"repository":{"type":"string"},"caller":caller(),"sourceRef":{"type":"string"},"anchorId":{"type":"string"},"expectedContentHash":{"type":"string"}}),
        ),
        "membrane_blueprint" => (
            vec!["repository", "caller", "operation"],
            json!({"repository":{"type":"string"},"caller":caller(),"operation":{"type":"string","enum":["architecture","symbol","reference","references","impact","changes","snapshot_get","snapshot_list","changes_since"]},"node":{"type":"string"},"items":{"type":"array"},"budget":{"type":"integer","minimum":1},"depth":{"type":"integer","minimum":0},"generationId":{"type":"string","minLength":1}}),
        ),
        "membrane_knowledge_propose" => (
            vec!["repository", "caller", "emission"],
            json!({"repository":{"type":"string"},"caller":caller(),"emission":{"type":"object"}}),
        ),
        "membrane_memory" => (
            vec!["repository", "caller", "operation", "id"],
            json!({"repository":{"type":"string"},"caller":caller(),
                "operation":{"type":"string","enum":["get","proposal_status","checkpoint_promote"]},
                "id":{"type":"string","minLength":1},"expectedContentHash":{"type":"string"},
                "offset":{"type":"integer","minimum":0,"maximum":1000000},
                "maxChars":{"type":"integer","minimum":1,"maximum":12000}}),
        ),
        "membrane_knowledge_review" => (
            vec!["repository", "caller", "review"],
            json!({"repository":{"type":"string"},"caller":caller(),"review":{"type":"object"}}),
        ),
        "membrane_checkpoint_save" => (
            vec!["repository", "caller", "checkpoint"],
            json!({"repository":{"type":"string"},"caller":caller(),"checkpoint":{"type":"object"}}),
        ),
        "membrane_checkpoint_load" => (
            vec!["repository", "caller", "id"],
            json!({"repository":{"type":"string"},"caller":caller(),"id":{"type":"string"},"asOfMs":{"type":"integer","minimum":0}}),
        ),
        "membrane_working_context" => (
            vec!["repository", "caller", "operation"],
            json!({"repository":{"type":"string"},"caller":caller(),"operation":{"type":"string","enum":["save","load","close"]},"context":{"type":"object"},"sessionId":{"type":"string"},"taskId":{"type":"string"},"contextId":{"type":"string"}}),
        ),
        "membrane_temporal_fact" => (
            vec!["repository", "caller", "operation"],
            json!({"repository":{"type":"string"},"caller":caller(),"operation":{"type":"string","enum":["record","query"]},"fact":{"type":"object"},"scopeId":{"type":"string"},"subject":{"type":"string"},"predicate":{"type":"string"},"asOf":{"type":"string"}}),
        ),
        "membrane_scratchpad" => (
            vec!["repository", "caller", "operation"],
            json!({"repository":{"type":"string"},"caller":caller(),"operation":{"type":"string","enum":["save","load","clear"]},"scratchpad":{"type":"object"},"sessionId":{"type":"string"},"taskId":{"type":"string"}}),
        ),
        "membrane_feedback" => (
            vec!["repository", "caller", "receiptId", "outcome"],
            json!({"repository":{"type":"string"},"caller":caller(),"receiptId":{"type":"string"},"outcome":{"type":"string","enum":["used","ignored","contradicted"]},"verdictRef":{"type":"string","minLength":1}}),
        ),
        _ => (
            vec!["operation"],
            json!({
              "operation":{"type":"string"},"repoId":{"type":"string"},"worktreeId":{"type":"string"},
              "projectRoot":{"type":"string"},"manifestDigest":{"type":"string"},"hashes":{"type":"array"},
              "epoch":{"type":"object"},"policyProfileName":{"type":"string"},
              "requiredCapabilities":{"type":"array"},"maxCost":{"type":"string"},"deadlineMs":{"type":"integer","minimum":1},
              "snapshot":{"type":"object"},"expectedEpoch":{"type":"object"},"policy":{"type":"object"},
              "name":{"type":"string"},"keyDigest":{"type":"string"},"provider":{"type":"string"}
            }),
        ),
    };
    let mut properties = properties;
    // These fields already have native consumers; declaring them prevents a
    // strict public boundary from silently removing task-authority narrowing.
    properties["taskGrantLevel"] = json!({"type":"string","enum":["read-only","write-proposed","write-trusted","admin"]});
    properties["scopeGrantId"] = json!({"type":"string","minLength":1});
    if name.starts_with("membrane_diagnostic_") { properties["caller"] = caller(); }
    json!({"type":"object","required":required,"properties":properties,"additionalProperties":false})
}
fn annotations(name: &str) -> Value {
    match name {
        "membrane_context"
        | "membrane_source_read"
        | "membrane_blueprint"
        | "membrane_diagnostic_fence"
        | "membrane_diagnostic_capabilities" => {
            json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true})
        }
        "membrane_diagnostic_provider" => {
            json!({"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false})
        }
        _ => json!({"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false}),
    }
}
pub(crate) fn definitions() -> Value {
    Value::Array(
        CORE.iter()
            .chain(DIAGNOSTIC).chain(OPERATOR)
            .map(|name| {
                let mut tool=json!({
                  "name":name,"description":description(name),
                  "inputSchema":schema(name),"annotations":annotations(name)
                });
                let output = match *name {
                    "membrane_memory" => Some(include_str!("../../../../schemas/operations/membrane-memory.v1.schema.json")),
                    "membrane_knowledge_review" => Some(include_str!("../../../../schemas/operations/membrane-knowledge-review.v1.schema.json")),
                    "membrane_knowledge_propose" => Some(include_str!("../../../../schemas/operations/membrane-knowledge-propose.v2.schema.json")),
                    "membrane_temporal_fact" => Some(include_str!("../../../../schemas/operations/membrane-temporal-fact.v2.schema.json")),
                    _ => None,
                };
                if let Some(output) = output { tool["outputSchema"] = serde_json::from_str(output).expect("compiled operation schema"); }
                tool
            })
            .collect(),
    )
}
fn requested(params: Option<&Value>) -> Option<Vec<&str>> {
    let list = params?.pointer("/_meta/membrane.toolsets.v1")?.as_array()?;
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for value in list {
        let group = value.as_str()?;
        if !matches!(group, "default" | "memory" | "blueprint" | "diagnostic" | "operator")
            || !seen.insert(group)
        {
            return None;
        }
        result.push(group);
    }
    Some(result)
}
pub(crate) fn negotiated_definitions(params: Option<&Value>) -> Value {
    // A conventional client must discover the safe memory workflow without
    // knowing a private metadata extension. Operator actions remain opt-in.
    let mut names = vec!["membrane_context", "membrane_knowledge_propose", "membrane_memory",
        "membrane_checkpoint_save", "membrane_checkpoint_load"];
    for group in requested(params).unwrap_or_default() {
        let additions: &[&str] = match group {
            "memory" => &CORE[3..],
            "blueprint" => &CORE[1..3],
            "diagnostic" => DIAGNOSTIC,
            "operator" => OPERATOR,
            _ => &[],
        };
        for name in additions {
            if !names.contains(name) {
                names.push(name);
            }
        }
    }
    Value::Array(
        definitions()
            .as_array()
            .unwrap()
            .iter()
            .filter(|tool| names.contains(&tool["name"].as_str().unwrap_or("")))
            .cloned()
            .collect(),
    )
}
fn envelope(operation: &str, code: &str, message: &str) -> Value {
    let (schema_version,error_version)=membrane_protocol::operations::operation_versions(operation);
    json!({"schemaVersion":schema_version,"operation":operation,"errorVersion":error_version,"result":{"kind":"error","code":code,"message":message,"retryable":false}})
}
pub fn invalid_envelope_code(name: &str) -> &'static str {
    match name {
        "membrane_source_read" => "source_read_envelope_invalid",
        "membrane_blueprint" => "blueprint_envelope_invalid",
        "membrane_checkpoint_save" => "checkpoint_envelope_invalid",
        "membrane_checkpoint_load" => "checkpoint_envelope_invalid",
        "membrane_working_context" => "working_context_envelope_invalid",
        "membrane_temporal_fact" => "temporal_fact_envelope_invalid",
        "membrane_scratchpad" => "scratchpad_envelope_invalid",
        "membrane_feedback" => "feedback_invalid",
        "membrane_knowledge_propose" => "proposal_envelope_invalid",
        "membrane_memory" => "memory_envelope_invalid",
        "membrane_knowledge_review" => "cortex_review_invalid",
        _ => "context_envelope_invalid",
    }
}
/// Runtime-owned operation bridge. Implementations must return the canonical
/// operation envelope, never an MCP transport envelope or legacy fallback.
pub trait NativeMcpExecutor: Send + Sync + 'static {
    fn execute(&self, name: &str, arguments: &Value) -> Value;
}
static EXECUTOR: OnceLock<Arc<dyn NativeMcpExecutor>> = OnceLock::new();
/// Install the one process-wide native operation owner before serving MCP.
pub fn install_executor(
    executor: Arc<dyn NativeMcpExecutor>,
) -> Result<(), Arc<dyn NativeMcpExecutor>> {
    EXECUTOR.set(executor)
}
/// Typed fail-closed seam. No interpreter fallback is ever attempted.
pub(crate) fn call(name: &str, arguments: &Value) -> Value {
    if !CORE.contains(&name) && !DIAGNOSTIC.contains(&name) && !OPERATOR.contains(&name) {
        return json!({"content":[{"type":"text","text":"unknown_tool"}],"isError":true});
    }
    if let Err(message) = validate_arguments(name, arguments) {
        let result = envelope(name, invalid_envelope_code(name), &message);
        return json!({"content":[{"type":"text","text":result.to_string()}],"structuredContent":result,"isError":true});
    }
    if let Some(executor) = EXECUTOR.get() {
        let result = executor.execute(name, arguments);
        let is_error = result.pointer("/result/kind").and_then(Value::as_str) != Some("success");
        return json!({"content":[{"type":"text","text":result.to_string()}],"structuredContent":result,"isError":is_error});
    }
    let (code, message) = if !arguments.is_object() {
        (invalid_envelope_code(name), "arguments must be an object")
    } else if DIAGNOSTIC.contains(&name) {
        (
            "diagnostic_unavailable",
            "native diagnostic runtime unavailable",
        )
    } else {
        ("context_unavailable", "native runtime handler unavailable")
    };
    let result = envelope(name, code, message);
    json!({"content":[{"type":"text","text":result.to_string()}],"structuredContent":result,"isError":true})
}

fn description(name: &str) -> String {
    match name {
        "membrane_knowledge_propose" => "Submit untrusted memory for durable review. Returns its proposal ID and current state; this does not admit knowledge or authorize review.",
        "membrane_memory" => "Resolve an exact Cortex memory body using its ID and expected full-content hash, inspect a scoped proposal status, or submit a checkpoint for review. Large records use nextOffset with the same hash. get requires expectedContentHash; a read is not proof of usefulness.",
        "membrane_knowledge_review" => "Operator transport for an independently signed, scope/store/content-bound approve, reject, suppress or resume decision. Reviewer text alone grants no authority. This operation cannot enroll keys.",
        "membrane_temporal_fact" => "Query scoped temporal facts or submit a temporal fact proposal. record never directly establishes durable truth; governed predicate admission may report a typed unavailable state.",
        "membrane_checkpoint_save" => "Save bounded A0 task continuity, outside semantic recall. Use membrane_memory checkpoint_promote to submit a later knowledge proposal.",
        "membrane_checkpoint_load" => "Load an unexpired, same-scope A0 checkpoint by ID. A checkpoint is orientation, not authoritative durable knowledge.",
        _ => return format!("Native Membrane handler for {name}."),
    }.to_owned()
}

/// Validate the bounded structural vocabulary actually emitted by this
/// registry. Typed runtime records additionally validate nested semantic
/// payloads. This is not a general-purpose JSON Schema implementation.
pub fn validate_arguments(name: &str, arguments: &Value) -> Result<(), String> {
    if !CORE.contains(&name) && !DIAGNOSTIC.contains(&name) && !OPERATOR.contains(&name) {
        return Err("unknown native operation".into());
    }
    let bytes = serde_json::to_vec(arguments).map_err(|_| "invalid JSON arguments")?;
    if bytes.len() > 65536 { return Err("operation arguments exceed 65536 bytes".into()); }
    validate_shape(&schema(name), arguments, 0)?;
    if name == "membrane_memory" && arguments["operation"] == "get"
        && arguments.get("expectedContentHash").and_then(Value::as_str).is_none_or(str::is_empty) {
        return Err("memory get requires expectedContentHash".into());
    }
    Ok(())
}
fn validate_shape(schema: &Value, value: &Value, depth: usize) -> Result<(), String> {
    if depth > 64 { return Err("operation arguments exceed nesting limit".into()); }
    let valid_type = match schema.get("type").and_then(Value::as_str) {
        Some("object") => value.is_object(), Some("array") => value.is_array(),
        Some("string") => value.is_string(), Some("boolean") => value.is_boolean(),
        Some("integer") => value.as_i64().is_some() || value.as_u64().is_some(),
        Some("number") => value.is_number(), None => true,
        Some(_) => return Err("unsupported registry schema type".into()),
    };
    if !valid_type { return Err("operation argument type mismatch".into()); }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        if !values.contains(value) { return Err("operation argument is outside its enum".into()); }
    }
    if schema.get("const").is_some_and(|expected| value != expected) {
        return Err("operation argument const mismatch".into());
    }
    if let Some(text) = value.as_str() {
        if schema.get("minLength").and_then(Value::as_u64).is_some_and(|min| text.chars().count() < min as usize) {
            return Err("operation argument is too short".into());
        }
        if schema.get("pattern").and_then(Value::as_str) == Some("\\S") && text.trim().is_empty() {
            return Err("operation argument must not be blank".into());
        }
    }
    if let Some(number) = value.as_f64() {
        if schema.get("minimum").and_then(Value::as_f64).is_some_and(|min| number < min)
            || schema.get("maximum").and_then(Value::as_f64).is_some_and(|max| number > max) {
            return Err("operation argument is outside bounds".into());
        }
    }
    if let Some(object) = value.as_object() {
        for required in schema.get("required").and_then(Value::as_array).into_iter().flatten() {
            if !object.contains_key(required.as_str().ok_or("invalid registry required key")?) {
                return Err(format!("missing required operation field: {}", required.as_str().unwrap_or("")));
            }
        }
        for (key, child) in object {
            if let Some(child_schema) = schema.get("properties").and_then(|p| p.get(key)) {
                validate_shape(child_schema, child, depth + 1)?;
            } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                return Err(format!("unknown operation field: {key}"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod cortex_boundary_tests {
    use super::*;
    fn proposal() -> Value {
        json!({"repository":"repo","caller":{"root":"/repo","repositoryId":"repo","scopeId":"scope"},"emission":{"text":"Keep a useful fact."}})
    }
    #[test]
    fn cortex_proposer_cannot_smuggle_review_into_native_dispatch() {
        let mut request = proposal();
        assert!(validate_arguments("membrane_knowledge_propose", &request).is_ok());
        request["review"] = json!({"decision":"approve","reviewer":"admin"});
        assert!(validate_arguments("membrane_knowledge_propose", &request).unwrap_err().contains("review"));
        assert_eq!(call("membrane_knowledge_propose", &request)["isError"], true);
    }
    #[test]
    fn cortex_public_discovery_has_memory_but_not_operator_effects() {
        let tools = negotiated_definitions(None);
        let names: Vec<_> = tools.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"membrane_memory"));
        assert!(names.contains(&"membrane_knowledge_propose"));
        assert!(!names.contains(&"membrane_knowledge_review"));
        assert!(negotiated_definitions(Some(&json!({"_meta":{"membrane.toolsets.v1":["operator"]}})))
            .as_array().unwrap().iter().any(|t| t["name"] == "membrane_knowledge_review"));
    }
    #[test]
    fn cortex_structural_validation_rejects_unknown_identity_and_bad_bounds() {
        let mut request = proposal();
        request["caller"]["reviewer"] = json!("human");
        assert!(validate_arguments("membrane_knowledge_propose", &request).is_err());
        let mut request = proposal(); request.as_object_mut().unwrap().remove("emission");
        request["operation"] = json!("get"); request["id"] = json!("record");
        assert!(validate_arguments("membrane_memory", &request).is_err());
        request["expectedContentHash"] = json!("a".repeat(64));
        request["maxChars"] = json!(12001);
        assert!(validate_arguments("membrane_memory", &request).is_err());
    }
}
