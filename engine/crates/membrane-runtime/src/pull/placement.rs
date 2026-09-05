//! Invariant-preserving semantic placement for finalized Pull blocks.
//!
//! Placement changes presentation order only. Membership, block fields,
//! authority/trust metadata, planner priority, protected state and source
//! identities are untouched.

use cortex_core::planner::ContextPacketV1;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementReceiptV1 {
    pub schema_version: u32,
    pub policy: &'static str,
    pub rows: Vec<PlacementRowV1>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementRowV1 {
    pub id: String,
    pub semantic_class: &'static str,
    pub original_index: usize,
    pub placed_index: usize,
}

pub fn place(packet: &mut ContextPacketV1) -> PlacementReceiptV1 {
    let original = packet
        .blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (block.id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    packet.blocks.sort_by(|left, right| {
        class_rank(left)
            .cmp(&class_rank(right))
            .then_with(|| original.get(&left.id).cmp(&original.get(&right.id)))
    });
    let rows = packet
        .blocks
        .iter()
        .enumerate()
        .map(|(placed_index, block)| PlacementRowV1 {
            id: block.id.clone(),
            semantic_class: semantic_class(block),
            original_index: original.get(&block.id).copied().unwrap_or(placed_index),
            placed_index,
        })
        .collect();
    PlacementReceiptV1 {
        schema_version: 1,
        policy: "pull-semantic-placement-v1",
        rows,
    }
}

fn semantic_class(block: &cortex_core::planner::BlockV1) -> &'static str {
    match (block.provider.as_str(), block.source_kind.as_str()) {
        ("rules", _) => "policy_evidence",
        ("cortex", _) | (_, "memory") => "durable_knowledge",
        ("blueprint", _) | ("live_files", _) | ("git", _) | (_, "graph") | (_, "file") => {
            "repository_evidence"
        }
        ("ledger", _) | (_, "doc") => "document_evidence",
        _ => "task_evidence",
    }
}

fn class_rank(block: &cortex_core::planner::BlockV1) -> u8 {
    match semantic_class(block) {
        "policy_evidence" => 0,
        "durable_knowledge" => 1,
        "repository_evidence" => 2,
        "document_evidence" => 3,
        _ => 4,
    }
}
