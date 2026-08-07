//! MBR-303 contract and parity tests for the canonical Membrane MCP prompts.
//!
//! These tests assert every prompt:
//!   (a) only references operations that exist in the operations registry,
//!   (b) declares `authorityEscalation: false`,
//!   (c) lives inside an `authorityScope` no higher than the highest authority
//!       the operations it references already grant.
//! Book-mode discipline: the file is wired into the crate's test target but is
//! not executed during implementation — the deferred command list runs at the
//! end of Book 1.

use membrane_mcp::{get_prompt_payload, list_prompts_payload, PROMPT_NAMES};
use serde_json::Value;

const OPERATIONS_INDEX: &str =
    include_str!("../../../../operations/operations/operations-index.v1.golden.json");

fn registry_operations() -> Vec<String> {
    let value: Value = serde_json::from_str(OPERATIONS_INDEX).expect("registry parses");
    value["operations"]
        .as_array()
        .expect("operations array")
        .iter()
        .map(|entry| entry["name"].as_str().unwrap().to_string())
        .collect()
}

fn assert_bounded(name: &str, prompt: &Value) {
    let ops = prompt["operations"]
        .as_array()
        .unwrap_or_else(|| panic!("{name} declares an operations array"));
    assert!(
        !ops.is_empty(),
        "{name} must reference at least one operation"
    );
    let known = registry_operations();
    for op in ops {
        let op_name = op.as_str().unwrap();
        assert!(
            known.iter().any(|k| k == op_name),
            "{name} references unknown operation: {op_name}"
        );
    }
}

fn read_only_operations() -> &'static [&'static str] {
    &[
        "membrane_context",
        "membrane_source_read",
        "membrane_checkpoint_load",
        "membrane_working_context",
        "membrane_temporal_fact",
        "membrane_scratchpad",
    ]
}

fn write_proposed_operations() -> &'static [&'static str] {
    &[
        "membrane_knowledge_propose",
        "membrane_checkpoint_save",
        "membrane_feedback",
    ]
}

fn assert_no_escalation(name: &str, prompt: &Value) {
    assert_eq!(
        prompt["authorityEscalation"],
        Value::Bool(false),
        "{name} must declare authorityEscalation: false"
    );
    let ops: Vec<&str> = prompt["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let read_only = read_only_operations();
    let write_proposed = write_proposed_operations();
    let all_read_only = ops.iter().all(|op| read_only.contains(op));
    if all_read_only {
        assert_eq!(
            prompt["authorityScope"], "read-only",
            "{name} references only read-only operations and must declare authorityScope=read-only"
        );
    } else if ops.iter().any(|op| write_proposed.contains(op)) {
        assert_eq!(
            prompt["authorityScope"], "write-proposed",
            "{name} references a write-proposed operation and must declare authorityScope=write-proposed"
        );
    } else {
        panic!(
            "{name} references operations at an unsupported authority tier: {}",
            ops.join(", ")
        );
    }
}

#[test]
fn list_payload_returns_four_canonical_prompts() {
    let payload = list_prompts_payload();
    let prompts = payload["prompts"].as_array().unwrap();
    assert_eq!(prompts.len(), 4, "four canonical prompts are committed");
    let names: Vec<&str> = prompts
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, PROMPT_NAMES);
}

#[test]
fn list_payload_metadata_matches_canonical_contract() {
    let payload = list_prompts_payload();
    for prompt in payload["prompts"].as_array().unwrap() {
        let name = prompt["name"].as_str().unwrap();
        assert_bounded(name, prompt);
        assert_no_escalation(name, prompt);
        assert!(
            prompt["description"]
                .as_str()
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            "{name} declares a description"
        );
        let args = prompt["arguments"].as_array().unwrap();
        for arg in args {
            assert!(arg["name"].as_str().map(|s| !s.is_empty()).unwrap_or(false));
            assert!(arg["description"]
                .as_str()
                .map(|s| !s.is_empty())
                .unwrap_or(false));
            assert_eq!(arg["required"].as_bool().is_some(), true);
        }
    }
}

#[test]
fn get_payload_returns_every_named_prompt() {
    for name in PROMPT_NAMES {
        let payload = get_prompt_payload(name).expect("known prompt returns a payload");
        assert_eq!(payload["name"], Value::String((*name).to_string()));
        let messages = payload["messages"].as_array().unwrap();
        assert!(!messages.is_empty(), "{name} carries messages");
        for message in messages {
            assert_eq!(message["role"], "user", "{name} messages are user-authored");
            assert_eq!(
                message["content"]["type"], "text",
                "{name} messages are text"
            );
            let text = message["content"]["text"].as_str().unwrap();
            assert!(!text.is_empty(), "{name} message text is non-empty");
            assert!(
                !text.contains("grant-admin") && !text.contains("grant admin"),
                "{name} message must not ask for escalated grants"
            );
        }
    }
}

#[test]
fn prompt_messages_only_mention_declared_operations() {
    let registry: Vec<String> = registry_operations();
    for name in PROMPT_NAMES {
        let payload = get_prompt_payload(name).expect("known prompt returns a payload");
        let declared: Vec<&str> = payload["operations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for message in payload["messages"].as_array().unwrap() {
            let text = message["content"]["text"].as_str().unwrap();
            for op in &registry {
                if text.contains(op) && !declared.contains(&op.as_str()) {
                    panic!("{name} mentions {op} but does not declare it in operations[]");
                }
            }
            let lowered = text.to_ascii_lowercase();
            assert!(
                !lowered.contains("grant admin")
                    && !lowered.contains("grant-admin")
                    && !lowered.contains("root grant")
                    && !lowered.contains("elevated")
                    && !lowered.contains("system prompt")
                    && !lowered.contains("developer override"),
                "{name} message must not reference escalated grants or overrides: {text}"
            );
        }
    }
}

#[test]
fn prompt_authority_scope_is_never_above_referenced_operations() {
    let payload = list_prompts_payload();
    for prompt in payload["prompts"].as_array().unwrap() {
        let name = prompt["name"].as_str().unwrap();
        let ops: Vec<&str> = prompt["operations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let scope = prompt["authorityScope"].as_str().unwrap();
        match scope {
            "read-only" => assert!(
                ops.iter().all(|op| read_only_operations().contains(op)),
                "{name} declares read-only but references a write-proposed operation"
            ),
            "write-proposed" => assert!(
                ops.iter()
                    .any(|op| write_proposed_operations().contains(op)),
                "{name} declares write-proposed but references no write-proposed operation"
            ),
            other => panic!("{name} declares an unsupported authorityScope: {other}"),
        }
    }
}
