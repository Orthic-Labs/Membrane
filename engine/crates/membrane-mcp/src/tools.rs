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
];
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
        "description": "RemainingContextCeilingV1 (membrane-host-observation): the host's observed remaining context for this session and task. Required; never derived or defaulted by Membrane.",
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
            json!({"repository":{"type":"string"},"caller":caller(),"operation":{"type":"string","enum":["architecture","symbol","reference","references","impact","changes","snapshot_get","snapshot_list","changes_since"]},"node":{"type":"string"},"items":{"type":"array"},"generationId":{"type":"string","minLength":1}}),
        ),
        "membrane_knowledge_propose" => (
            vec!["repository", "caller", "emission"],
            json!({"repository":{"type":"string"},"caller":caller(),"emission":{"type":"object"}}),
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
            .chain(DIAGNOSTIC)
            .map(|name| {
                json!({
                  "name":name,"description":format!("Native Membrane handler for {name}."),
                  "inputSchema":schema(name),"annotations":annotations(name)
                })
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
        if !matches!(group, "default" | "memory" | "blueprint" | "diagnostic")
            || !seen.insert(group)
        {
            return None;
        }
        result.push(group);
    }
    Some(result)
}
pub(crate) fn negotiated_definitions(params: Option<&Value>) -> Value {
    let mut names = vec!["membrane_context"];
    for group in requested(params).unwrap_or_default() {
        let additions: &[&str] = match group {
            "memory" => &CORE[3..],
            "blueprint" => &CORE[1..3],
            "diagnostic" => DIAGNOSTIC,
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
    json!({"schemaVersion":1,"operation":operation,"errorVersion":1,"result":{"kind":"error","code":code,"message":message,"retryable":false}})
}
fn invalid_envelope_code(name: &str) -> &'static str {
    match name {
        "membrane_source_read" => "source_read_envelope_invalid",
        "membrane_blueprint" => "blueprint_envelope_invalid",
        "membrane_checkpoint_save" => "checkpoint_envelope_invalid",
        "membrane_checkpoint_load" => "checkpoint_envelope_invalid",
        "membrane_working_context" => "working_context_envelope_invalid",
        "membrane_temporal_fact" => "temporal_fact_envelope_invalid",
        "membrane_scratchpad" => "scratchpad_envelope_invalid",
        "membrane_feedback" => "feedback_invalid",
        "membrane_knowledge_propose" => "proposal_scope_denied",
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
    if !CORE.contains(&name) && !DIAGNOSTIC.contains(&name) {
        return json!({"content":[{"type":"text","text":"unknown_tool"}],"isError":true});
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
