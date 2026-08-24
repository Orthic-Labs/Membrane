use serde_json::{json, Value};
use std::collections::HashSet;

fn diagnostic_id() -> Value {
    json!({ "type": "string", "minLength": 1, "maxLength": 128 })
}

fn capabilities() -> Value {
    json!([
        "syntax",
        "repository_module_resolution",
        "import_export_binding",
        "name_resolution",
        "type_semantics",
        "configured_static_policy",
        "compiler_project_semantics",
        "generated_source_awareness"
    ])
}

fn cost_classes() -> Value {
    json!(["instant", "interactive", "verification", "build", "test"])
}

fn hash_entry_schema() -> Value {
    json!({
        "type": "object",
        "required": ["path", "hash"],
        "additionalProperties": false,
        "properties": {
            "path": { "type": "string", "minLength": 1 },
            "hash": { "type": "string", "minLength": 1 }
        }
    })
}

pub(crate) fn definitions() -> Value {
    json!([
        {
            "name": "membrane_diagnostic_workspace",
            "description": "Open, close, inspect, or reconcile one live-diagnostics workspace session on the resident Membrane service. status reads session state; reconcile proves exact current worktree bytes for reconciliation_only hosts and any mismatch against the latest cleared epoch classifies unknown_conflict or superseded, invalidating prior clearance. open binds one canonical absolute projectRoot (design §3 WorkspaceEngineKey); same repo/worktree + different root is a typed conflict, uncanonicalizable root is rejected.",
            "inputSchema": {
                "type": "object",
                "required": ["operation"],
                "additionalProperties": false,
                "properties": {
                    "operation": { "type": "string", "enum": ["open", "close", "status", "reconcile"] },
                    "repoId": diagnostic_id(),
                    "worktreeId": diagnostic_id(),
                    "projectRoot": { "type": "string", "minLength": 1, "maxLength": 1024, "description": "Canonical absolute worktree/project root to bind at open (design §3). Same repo/worktree + different canonical root is a typed conflict; uncanonicalizable root is rejected." },
                    "manifestDigest": { "type": "string", "minLength": 1, "maxLength": 256 },
                    "hashes": { "type": "array", "minItems": 0, "maxItems": 4096, "items": hash_entry_schema() }
                },
                "oneOf": [
                    {
                        "properties": { "operation": { "const": "open" } },
                        "required": ["repoId", "worktreeId"],
                        "not": { "anyOf": [{ "required": ["manifestDigest"] }, { "required": ["hashes"] }] }
                    },
                    {
                        "properties": { "operation": { "enum": ["close", "status"] } },
                        "required": ["repoId", "worktreeId"],
                        "not": { "anyOf": [{ "required": ["manifestDigest"] }, { "required": ["hashes"] }] }
                    },
                    {
                        "properties": { "operation": { "const": "reconcile" } },
                        "required": ["repoId", "worktreeId", "manifestDigest", "hashes"]
                    }
                ]
            }
        },
        {
            "name": "membrane_diagnostic_mutation",
            "description": "Transactionally begin or seal one coherent mutation batch, or register exact observed resulting bytes (registerObserved with observed_hook origin) for hosts without edit transactions. Seal/register invalidate stale clearance. Never blocks or rolls back writes: the fence gates semantic acceptance, not disk persistence.",
            "inputSchema": {
                "type": "object",
                "required": ["operation", "repoId", "worktreeId"],
                "additionalProperties": false,
                "properties": {
                    "operation": { "type": "string", "enum": ["begin", "seal", "registerObserved"] },
                    "repoId": diagnostic_id(),
                    "worktreeId": diagnostic_id(),
                    "epoch": {
                        "type": "object",
                        "description": "WorkspaceEpochV1 envelope (workspace-epoch.v1) bound to this repoId/worktreeId; origin transactional for seal, observed_hook for registerObserved."
                    }
                },
                "oneOf": [
                    {
                        "properties": { "operation": { "const": "begin" } },
                        "not": { "required": ["epoch"] }
                    },
                    {
                        "properties": { "operation": { "enum": ["seal", "registerObserved"] } },
                        "required": ["epoch"]
                    }
                ]
            }
        },
        {
            "name": "membrane_diagnostic_snapshot",
            "description": "Await a mutation-bound evidence snapshot plus planner gate decision (the operational fence path), or read get/explain/delta views of the last awaited snapshot cached per repoId:worktreeId in this server process. Events and presentation never clear the fence; only snapshot-await (and resident-side fence evaluation) produces operational decisions. get/explain/delta are cached views, never re-evaluation.",
            "inputSchema": {
                "type": "object",
                "required": ["operation"],
                "additionalProperties": false,
                "properties": {
                    "operation": { "type": "string", "enum": ["await", "get", "explain", "delta"] },
                    "repoId": diagnostic_id(),
                    "worktreeId": diagnostic_id(),
                    "policyProfileName": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "requiredCapabilities": {
                        "type": "array",
                        "minItems": 0,
                        "maxItems": 8,
                        "items": { "type": "string", "enum": capabilities() }
                    },
                    "maxCost": {
                        "type": "string",
                        "enum": cost_classes(),
                        "description": "Hard acquisition ceiling; defaults to interactive."
                    },
                    "deadlineMs": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 60000,
                        "description": "Absolute wait budget for await; defaults to 10000."
                    }
                },
                "oneOf": [
                    {
                        "properties": { "operation": { "const": "await" } },
                        "required": ["repoId", "worktreeId", "policyProfileName"]
                    },
                    {
                        "properties": { "operation": { "enum": ["get", "explain", "delta"] } },
                        "required": ["repoId", "worktreeId"],
                        "not": {
                            "anyOf": [
                                { "required": ["policyProfileName"] },
                                { "required": ["requiredCapabilities"] },
                                { "required": ["maxCost"] },
                                { "required": ["deadlineMs"] }
                            ]
                        }
                    }
                ]
            }
        },
        {
            "name": "membrane_diagnostic_fence",
            "description": "Pure Semantic Edit Fence evaluation: sends the exact DiagnosticEvidenceSnapshotV1, expected WorkspaceEpochV1 envelope, and planner-owned GatePolicyProfileV1 to the resident deterministic evaluator and returns DiagnosticGateDecisionV1 verbatim. It invents no policy, performs no provider acquisition, and never clears the resident fence by itself; the coding host enforces the returned decision.",
            "inputSchema": {
                "type": "object",
                "required": ["snapshot", "expectedEpoch", "policy"],
                "additionalProperties": false,
                "properties": {
                    "snapshot": {
                        "type": "object",
                        "description": "diagnostic-evidence-snapshot.v1 envelope."
                    },
                    "expectedEpoch": {
                        "type": "object",
                        "description": "workspace-epoch.v1 envelope the snapshot must match exactly."
                    },
                    "policy": {
                        "type": "object",
                        "description": "Planner-owned GatePolicyProfileV1: profileName, policyVersion, policyDigest, blockingCodes, requiredCapabilities."
                    }
                }
            }
        },
        {
            "name": "membrane_diagnostic_capabilities",
            "description": "Read the resident live-diagnostics capability advertisement: qualified providers, cost classes, and supported semantic capabilities. Read-only.",
            "inputSchema": {
                "type": "object",
                "required": [],
                "additionalProperties": false,
                "properties": {}
            }
        },
        {
            "name": "membrane_diagnostic_baseline",
            "description": "Capture or update a named diagnostics baseline for a workspace session; subsequent snapshot deltas classify issues as new, persistent, resolved, moved, changed, or unknown_baseline against it.",
            "inputSchema": {
                "type": "object",
                "required": ["operation", "repoId", "worktreeId", "name"],
                "additionalProperties": false,
                "properties": {
                    "operation": { "type": "string", "enum": ["capture", "update"] },
                    "repoId": diagnostic_id(),
                    "worktreeId": diagnostic_id(),
                    "name": { "type": "string", "minLength": 1, "maxLength": 128 }
                }
            }
        },
        {
            "name": "membrane_diagnostic_provider",
            "description": "List qualified providers (capabilities view), read resident supervisor health/status, or restart one supervised engine by workspace-engine key digest. list/status are read-only views; restart is a lifecycle action performed by the resident supervisor.",
            "inputSchema": {
                "type": "object",
                "required": ["operation"],
                "additionalProperties": false,
                "properties": {
                    "operation": { "type": "string", "enum": ["list", "status", "restart"] },
                    "keyDigest": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 256,
                        "description": "WorkspaceEngineKey digest identifying the engine to restart."
                    }
                },
                "oneOf": [
                    {
                        "properties": { "operation": { "const": "restart" } },
                        "required": ["keyDigest"]
                    },
                    {
                        "properties": { "operation": { "enum": ["list", "status"] } },
                        "not": { "required": ["keyDigest"] }
                    }
                ]
            }
        }
    ])
}

pub(crate) fn negotiated_definitions(params: Option<&Value>) -> Value {
    let requested = params
        .and_then(|value| value.pointer("/_meta/membrane.toolsets.v1"))
        .and_then(Value::as_array);
    let valid = requested
        .map(|groups| {
            let mut seen = HashSet::new();
            groups.iter().all(|group| {
                matches!(
                    group.as_str(),
                    Some("default" | "memory" | "blueprint" | "diagnostic")
                ) && seen.insert(group)
            })
        })
        .unwrap_or(true);
    if valid {
        definitions()
    } else {
        definitions()
    }
}
