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
    "membrane_memory",
    "membrane_checkpoint_save",
    "membrane_checkpoint_load",
    "membrane_working_context",
    "membrane_temporal_fact",
    "membrane_scratchpad",
    "membrane_feedback",
    "membrane_ledger",
    "membrane_push_prepare",
    "membrane_push_resolve",
];
const ADAPT: &[&str] = &["membrane_adapt_inspect"];
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
    if name.starts_with("membrane_push_") {
        let definitions: Value = serde_json::from_str(include_str!(
            "../../../../schemas/registry/push-tools.v1.json"
        ))
        .expect("Push schemas parse");
        return definitions
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == name)
            .expect("Push tool registered")["inputSchema"]
            .clone();
    }
    let (required, mut properties) = match name {
        "membrane_adapt_inspect" => (
            vec!["repository", "caller", "operation"],
            json!({"repository":{"type":"string","minLength":1},"caller":caller(),
        "operation":{"type":"string","enum":["preferences","explain","insights","proposals","status"]},
        "limit":{"type":"integer","minimum":0,"maximum":32},"hostContext":host_context()}),
        ),
        "membrane_context" => (
            // The runtime refuses every request without remainingContextCeiling
            // (RequestTimeH8Error::Missing), so the tool must advertise it.
            // Leaving it undeclared made the one entry tool impossible to call
            // correctly from its own schema.
            vec![
                "task",
                "taskId",
                "sessionId",
                "repository",
                "caller",
                "remainingContextCeiling",
            ],
            json!({"task":{"type":"string","minLength":1,"pattern":"\\S"},"taskId":{"type":"string","minLength":1,"description":"Stable task identity; distinct from task prose"},"sessionId":{"type":"string","minLength":1,"description":"Stable host session identity; distinct from caller.scopeId"},"requestId":{"type":"string","minLength":1},"generation":{"type":"string","minLength":1},"repository":{"type":"string"},"caller":caller(),"budget":{"type":"integer","minimum":1,"description":"Legacy native budget units (1024 tokens each); prefer budgetTokens"},"budgetTokens":{"type":"integer","minimum":1,"description":"Explicit final Pull attention ceiling in tokens"},"scope":{"type":"string","enum":["repo","workspace"]},"workspaceTargets":{"type":"array","description":"Optional repository-id subset for workspace scope. Omit to use the caller plus all explicitly granted child repositories.","items":{"type":"string","minLength":1},"maxItems":32,"uniqueItems":true},"deadlineMs":{"type":"integer","minimum":1},"scopeGrantId":{"type":"string","minLength":1},"anchors":{"type":"array","items":{"type":"string","minLength":1},"maxItems":64},"refresh":{"type":"boolean"},"consumerCapabilities":{"type":"object","description":"Negotiated capabilities of the consuming host. Resolver-only representations are eligible only when their resolver is listed here.","properties":{"resolvers":{"type":"array","items":{"type":"string","enum":["membrane_source_read","membrane_memory_read"]},"maxItems":32,"uniqueItems":true},"retainsDeliveredEvidence":{"type":"boolean"}},"additionalProperties":false},"sufficiencyContract":{"type":"object","description":"Optional planner-authored SufficiencyContractV1 (membrane-sufficiency-v1); transported verbatim to federate, never derived from task prose"},"remainingContextCeiling":remaining_context_ceiling(),"pushResolverToken":{"type":"string","minLength":64,"maxLength":64}}),
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
        "membrane_ledger" => (
            vec!["repository", "caller", "operation"],
            json!({"repository":{"type":"string"},"caller":caller(),
                "operation":{"enum":["recall","literal","outline","sync","status","activate","erase","backlinks","related","manifests","drift"]},
                "query":{"type":"string","minLength":1,"maxLength":4096},
                "k":{"type":"integer","minimum":1,"maximum":32},
                "path":{"type":"string"},"docId":{"type":"string"},"nodeId":{"type":"string"},
"scopeGrantId":{"type":"string"},"taskId":{"type":"string","minLength":1},
                "expectedContentHash":{"type":"string"},"continuationCursor":{"type":"string"},
                "maxSections":{"type":"integer","minimum":1,"maximum":256},
                "limit":{"type":"integer","minimum":1,"maximum":256},
                "mode":{"enum":["legacy_scan","shadow","ledger_fts"]},
                "fromManifest":{"type":"string"},"toManifest":{"type":"string"},
                "deadlineMs":{"type":"integer","minimum":1,"maximum":30000},
                "taskGrantLevel":{"type":"string"}}),
        ),
        "membrane_blueprint" => (
            vec!["repository", "caller", "operation"],
            json!({"repository":{"type":"string"},"caller":caller(),"operation":{"type":"string","enum":["architecture","symbol","reference","references","impact","changes","snapshot_get","snapshot_list","changes_since"]},"node":{"type":"string"},"items":{"type":"array"},"generationId":{"type":"string","minLength":1}}),
        ),
        "membrane_knowledge_propose" => (
            vec!["repository", "caller", "emission"],
            json!({"repository":{"type":"string"},"caller":caller(),"emission":{"type":"object",
                "required":["text","producer","epistemicClass"],
                "properties":{"text":{"type":"string","minLength":1},
                "producer":{"type":"string","enum":["manual","agent","harness","adapt_native","ingest_hook","dream","episodic","import","checkpoint","system"]},
                "epistemicClass":{"type":"string","enum":["observed","reported","inferred","directive"]}}},
            }),
        ),
        "membrane_knowledge_review" => (
            vec!["repository", "caller", "review"],
            json!({"repository":{"type":"string"},"caller":caller(),"review":{"type":"object"}}),
        ),
        "membrane_memory" => (
            vec!["repository", "caller", "operation"],
            json!({"repository":{"type":"string"},"caller":caller(),
                "operation":{"type":"string","enum":["get","proposal_status","checkpoint_promote","recall"]},
                "id":{"type":"string","minLength":1},
                "expectedContentHash":{"type":"string","pattern":"^sha256:[a-f0-9]{64}$"},
                "offset":{"type":"integer","minimum":0,"maximum":1000000},
                "maxChars":{"type":"integer","minimum":1,"maximum":12000},
                "query":{"type":"string","minLength":1,"maxLength":4096},
                "recipe":{"type":"object","required":["name","version"],"properties":{"name":{"type":"string","minLength":1},"version":{"type":"integer","minimum":1},"allowFallback":{"type":"boolean"}},"additionalProperties":false},
                "bounds":{"type":"object","properties":{"maxItems":{"type":"integer","minimum":1},"maxPreviewChars":{"type":"integer","minimum":0}},"additionalProperties":false},
                "projection":{"type":"string","enum":["metadata","preview"]}}),
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
            json!({"repository":{"type":"string"},"caller":caller(),"operation":{"type":"string","enum":["record","query"]},"fact":{"type":"object"},"singleValuedPredicates":{"type":"array","items":{"type":"string","minLength":1},"maxItems":64,"uniqueItems":true},"scopeId":{"type":"string"},"subject":{"type":"string"},"predicate":{"type":"string"},"asOf":{"type":"string"}}),
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
    if name == "membrane_source_read" {
        for field in [
            "docId",
            "nodeId",
            "expectedRevision",
            "expectedSpanHash",
            "continuationCursor",
            "ledgerTicket",
        ] {
            properties[field] = json!({"type":"string","maxLength":8192});
        }
        properties["ledgerGeneration"] = json!({"type":"integer","minimum":0});
        properties["maxBytes"] = json!({"type":"integer","minimum":1,"maximum":12000});
        properties["deadlineMs"] = json!({"type":"integer","minimum":1,"maximum":30000});
    }
    if name == "membrane_context" {
        properties["scopeGrantId"] = json!({"type":"string"});
    }
    properties["taskGrantLevel"] = json!({"type":"string","enum":["read-only","write-proposed","write-trusted","admin"]});
    properties["scopeGrantId"] = json!({"type":"string","minLength":1});
    if name.starts_with("membrane_diagnostic_") {
        properties["caller"] = caller();
    }
    json!({"type":"object","required":required,"properties":properties,"additionalProperties":false})
}
fn annotations(name: &str) -> Value {
    match name {
        "membrane_adapt_inspect"
        | "membrane_push_resolve"
        | "membrane_context"
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
        CORE.iter().chain(DIAGNOSTIC).chain(ADAPT).chain(OPERATOR)
            .map(|name| {
                if name.starts_with("membrane_push_") {
                    let entries: Value = serde_json::from_str(include_str!("../../../../schemas/registry/push-tools.v1.json")).expect("Push schemas parse");
                    return entries.as_array().unwrap().iter().find(|v| v["name"] == *name).unwrap().clone();
                }
                let mut tool = json!({
                  "name":name,"description":match *name {
                    "membrane_context" => "Federate bounded, grant-aware context through resident Membrane planner.",
                    "membrane_source_read" => "Resolve hash/revision/span-bound source reference.",
                    "membrane_ledger" => "Navigate resident Ledger document index. Final prompt admission remains Pull-owned.",
                    "membrane_adapt_inspect" => "Read scoped Taste decisions, Insights & live Adapt progress.",
                    "membrane_knowledge_propose" => "Submit untrusted knowledge proposal. No caller can self-review or admit truth.",
                    "membrane_knowledge_review" => "Apply installation-trusted signed Cortex review or reversible recall suppression.",
                    "membrane_memory" => "Resolve exact Cortex memory, inspect proposal state, promote checkpoints to proposals, or run bounded named recall recipes.",
                    "membrane_temporal_fact" => "Query temporal facts or submit proposal-only temporal record with explicit cardinality policy.",
                    _ => "Native Membrane operation.",
                  },
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
            }).collect(),
    )
}fn requested(params: Option<&Value>) -> Option<Vec<&str>> {
    let list = params?.pointer("/_meta/membrane.toolsets.v1")?.as_array()?;
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for value in list {
        let group = value.as_str()?;
        if !matches!(
            group,
            "default" | "memory" | "blueprint" | "diagnostic" | "ledger" | "adapt" | "push" | "operator"
        ) || !seen.insert(group)
        {
            return None;
        }
        result.push(group);
    }
    Some(result)
}
pub(crate) fn negotiated_definitions(params: Option<&Value>) -> Value {
    let mut names = vec![
        "membrane_context", "membrane_source_read", "membrane_ledger",
        "membrane_knowledge_propose", "membrane_memory",
        "membrane_checkpoint_save", "membrane_checkpoint_load",
    ];
    for group in requested(params).unwrap_or_default() {
        let additions: &[&str] = match group {
            "memory" => &CORE[3..11],
            "ledger" => &["membrane_source_read", "membrane_ledger"],
            "push" => &CORE[12..],
            "blueprint" => &CORE[1..3],
            "diagnostic" => DIAGNOSTIC,
            "adapt" => ADAPT,
            "operator" => OPERATOR,
            _ => &[],
        };
        for name in additions { if !names.contains(name) { names.push(name); } }
    }
    Value::Array(definitions().as_array().unwrap().iter()
        .filter(|tool| names.contains(&tool["name"].as_str().unwrap_or("")))
        .cloned().collect())
}fn envelope(operation: &str, code: &str, message: &str) -> Value {
    let (schema_version, error_version) = membrane_protocol::operations::operation_versions(operation);
    json!({"schemaVersion":schema_version,"operation":operation,"errorVersion":error_version,"result":{"kind":"error","code":code,"message":message,"retryable":false}})
}
pub fn invalid_envelope_code(name: &str) -> &'static str {
    match name {
        "membrane_source_read" => "source_read_envelope_invalid",
        "membrane_ledger" => "ledger_envelope_invalid",
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
/// Canonical result serializer shared with runtime final-wire measurement.
pub fn tool_result(result: Value) -> Value {
    let is_error = result.pointer("/result/kind").and_then(Value::as_str) != Some("success");
    json!({"content":[{"type":"text","text":result.to_string()}],"structuredContent":result,"isError":is_error})
}
/// Typed fail-closed seam. No interpreter fallback is ever attempted.
pub(crate) fn call(name: &str, arguments: &Value) -> Value {
    if !CORE.contains(&name) && !DIAGNOSTIC.contains(&name) && !ADAPT.contains(&name) && !OPERATOR.contains(&name) {
        return json!({"content":[{"type":"text","text":"unknown_tool"}],"isError":true});
    }
    if let Err(message) = validate_arguments(name, arguments) {
        let result = envelope(name, invalid_envelope_code(name), &message);
        return tool_result(result);
    }
    if let Some(executor) = EXECUTOR.get() {
        let result = executor.execute(name, arguments);
        return tool_result(result);
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

/// Validate bounded structural vocabulary emitted by registry.
pub fn validate_arguments(name: &str, arguments: &Value) -> Result<(), String> {
    if !CORE.contains(&name) && !DIAGNOSTIC.contains(&name) && !ADAPT.contains(&name) && !OPERATOR.contains(&name) {
        return Err("unknown native operation".into());
    }
    let bytes = serde_json::to_vec(arguments).map_err(|_| "invalid JSON arguments")?;
    if bytes.len() > 65_536 { return Err("operation arguments exceed 65536 bytes".into()); }
    validate_shape(&schema(name), arguments, 0)?;
    if name == "membrane_memory" {
        match arguments["operation"].as_str().unwrap_or("") {
            "get" if arguments.get("id").and_then(Value::as_str).is_none_or(str::is_empty)
                || arguments.get("expectedContentHash").and_then(Value::as_str).is_none_or(str::is_empty) =>
                return Err("memory get requires id and expectedContentHash".into()),
            "proposal_status" | "checkpoint_promote"
                if arguments.get("id").and_then(Value::as_str).is_none_or(str::is_empty) =>
                return Err("memory operation requires id".into()),
            "recall" if arguments.get("query").and_then(Value::as_str).is_none_or(str::is_empty)
                || !arguments.get("recipe").is_some_and(Value::is_object) =>
                return Err("memory recall requires query and recipe".into()),
            _ => {}
        }
    }
    if name == "membrane_temporal_fact" {
        match arguments["operation"].as_str().unwrap_or("") {
            "record" if !arguments.get("fact").is_some_and(Value::is_object)
                || !arguments.get("singleValuedPredicates").is_some_and(Value::is_array) =>
                return Err("temporal record requires fact and singleValuedPredicates".into()),
            "query" if ["subject", "predicate", "asOf"].iter().any(|field|
                arguments.get(*field).and_then(Value::as_str).is_none_or(str::is_empty)) =>
                return Err("temporal query requires subject, predicate and asOf".into()),
            _ => {}
        }
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
    if let Some(text) = value.as_str() {
        let len = text.chars().count() as u64;
        if schema.get("minLength").and_then(Value::as_u64).is_some_and(|min| len < min)
            || schema.get("maxLength").and_then(Value::as_u64).is_some_and(|max| len > max) {
            return Err("operation argument string is outside bounds".into());
        }
        if schema.get("pattern").and_then(Value::as_str) == Some("\\S") && text.trim().is_empty() {
            return Err("operation argument must not be blank".into());
        }
        if schema.get("pattern").and_then(Value::as_str) == Some("^sha256:[a-f0-9]{64}$")
            && (text.len() != 71 || !text.starts_with("sha256:")
                || !text[7..].bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())) {
            return Err("operation content hash is invalid".into());
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
            let key = required.as_str().ok_or("invalid registry required key")?;
            if !object.contains_key(key) { return Err(format!("missing required operation field: {key}")); }
        }
        for (key, child) in object {
            if let Some(child_schema) = schema.get("properties").and_then(|properties| properties.get(key)) {
                validate_shape(child_schema, child, depth + 1)?;
            } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                return Err(format!("unknown operation field: {key}"));
            }
        }
    }
    if let Some(array) = value.as_array() {
        if schema.get("maxItems").and_then(Value::as_u64).is_some_and(|max| array.len() as u64 > max) {
            return Err("operation array exceeds bound".into());
        }
        if schema.get("uniqueItems") == Some(&Value::Bool(true)) {
            let unique = array.iter().map(Value::to_string).collect::<HashSet<_>>();
            if unique.len() != array.len() { return Err("operation array items must be unique".into()); }
        }
        if let Some(item_schema) = schema.get("items") {
            for item in array { validate_shape(item_schema, item, depth + 1)?; }
        }
    }
    Ok(())
}
fn host_context() -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{
        "client":{"type":"string","minLength":1},"model":{"type":"string","minLength":1},
        "machine":{"type":"string","minLength":1},"dimensions":{"type":"object","additionalProperties":{"type":"string"}}
    },"description":"Actual host context. Omitted fields remain unavailable, never inferred from the transport."})
}
