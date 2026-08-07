// GENERATED — DO NOT EDIT
// operation_registry_version: sha256:8b3f3a9e7633d68212487a03545cec53da02262a69639f343e92f23307645586
match name {
    "" => Some(vec![
        ("membrane_context".to_string(), String::new()),
        ("membrane_source_read".to_string(), String::new()),
        ("membrane_knowledge_propose".to_string(), String::new()),
        ("membrane_checkpoint_save".to_string(), String::new()),
        ("membrane_checkpoint_load".to_string(), String::new()),
        ("membrane_working_context".to_string(), String::new()),
        ("membrane_temporal_fact".to_string(), String::new()),
        ("membrane_scratchpad".to_string(), String::new()),
        ("membrane_feedback".to_string(), String::new()),
    ]),
    "membrane_context" => Some(vec![ // Federated context packet for one exact caller binding.
    ]),
    "membrane_source_read" => Some(vec![ // Hash-bound DocReadV1 section fetch for one exact caller binding.
    ]),
    "membrane_knowledge_propose" => Some(vec![ // Submit a bounded typed KnowledgeEmission proposal for quarantine review.
    ]),
    "membrane_checkpoint_save" => Some(vec![ // Save an A0 session checkpoint for one exact caller binding; never durable knowledge.
    ]),
    "membrane_checkpoint_load" => Some(vec![ // Load an unexpired A0 session checkpoint for one exact caller binding.
    ]),
    "membrane_working_context" => Some(vec![ // Save, load, or close bounded session/task working context; durability must be explicit.
    ]),
    "membrane_temporal_fact" => Some(vec![ // Record or query provenance-bound temporal facts with explicit single-valued predicate policy.
    ]),
    "membrane_scratchpad" => Some(vec![ // Save, load, or clear ephemeral non-searchable session/task scratchpad state.
    ]),
    "membrane_feedback" => Some(vec![ // Record bounded receipt-bound outcome feedback for quarantine review.
    ]),
    _ => None,
}
