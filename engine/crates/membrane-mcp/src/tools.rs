use serde_json::{json, Value};

const NAMES: &[(&str, &str)] = &[
    ("membrane_context", "Federated context packet for one exact caller binding."),
    ("membrane_source_read", "Hash-bound DocReadV1 section fetch for one exact caller binding."),
    ("membrane_knowledge_propose", "Submit a bounded typed KnowledgeEmission proposal for quarantine review."),
    ("membrane_checkpoint_save", "Save an A0 session checkpoint for one exact caller binding; never durable knowledge."),
    ("membrane_checkpoint_load", "Load an unexpired A0 session checkpoint for one exact caller binding."),
    ("membrane_working_context", "Save, load, or close bounded session/task working context; durability must be explicit."),
    ("membrane_temporal_fact", "Record or query provenance-bound temporal facts with explicit single-valued predicate policy."),
    ("membrane_scratchpad", "Save, load, or clear ephemeral non-searchable session/task scratchpad state."),
    ("membrane_feedback", "Record bounded receipt-bound outcome feedback for quarantine review."),
];

pub(crate) fn definitions() -> Value {
    Value::Array(NAMES.iter().map(|(name, description)| json!({"name": name, "description": description, "inputSchema": {"type": "object"}, "outputSchema": {"type": "object", "required": ["data", "trace"]}})).collect())
}
