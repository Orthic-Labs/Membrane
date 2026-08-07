//! The receipt-side reconciliation of the cross-provider attention budget.
//!
//! Every receipt reconciles its selected and delivered tokens under one
//! budget (MBR-608). [`reconcile`] walks the packet's blocks, classifies each
//! into a lane (using the lane the renderer stamped on the block), and
//! returns a [`BudgetReconciliation`] that the receipt carries. The
//! reconciliation is the SINGLE place a caller can verify that:
//!
//!   * the per-lane selected tokens sum to the global selected tokens,
//!   * the per-lane delivered tokens sum to the global delivered tokens,
//!   * the global selected tokens did not exceed the global ceiling,
//!   * every block was classified into exactly one lane.
//!
//! Anything else is a contract drift and surfaces as a stable alert id.
//!
//! The reconciliation runs over a [`ContextPacketV1`] shaped packet whose
//! blocks carry the renderer-stamped lane (`block.lane`). It does not require
//! the renderer to know about membrane-core.

use crate::budget::CrossProviderBudget;
use crate::lane::{
    LaneAccounting, BUDGET_LANE_KINDS,
};
use membrane_protocol::{BlockV1, BudgetLaneKind, ContextPacketV1};
use serde::{Deserialize, Serialize};

/// One row of the receipt-side reconciliation. Mirrors the lane enumeration
/// in [`BUDGET_LANE_KINDS`] order; the type is the cross-language contract
/// the receipt carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetReconciliationRowV1 {
    pub lane: BudgetLaneKind,
    pub selected_tokens: u32,
    pub delivered_tokens: u32,
    pub delivered_chars: u32,
    pub blocks: u32,
}

/// The single receipt-side reconciliation. Every receipt carries one of
/// these. The plan-side [`CrossProviderBudget`] is what the planner fills;
/// the reconciliation is what the receipt proves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetReconciliation {
    /// Stable version of the reconciliation shape. Bumped only when the
    /// row schema gains a field.
    pub schema_version: u32,
    /// The single global ceiling the budget could not exceed.
    pub max_tokens: u32,
    /// Total selected tokens across all lanes. Equals the sum of the
    /// `selected_tokens` rows.
    pub total_selected_tokens: u32,
    /// Total delivered tokens across all lanes. Equals the sum of the
    /// `delivered_tokens` rows.
    pub total_delivered_tokens: u32,
    /// Total delivered chars across all lanes.
    pub total_delivered_chars: u32,
    /// One row per lane, in [`BUDGET_LANE_KINDS`] order.
    pub lanes: Vec<BudgetReconciliationRowV1>,
    /// Number of blocks the renderer could not classify into a lane. A
    /// non-zero count is a contract drift.
    pub unclassified_blocks: u32,
    /// True when the reconciliation passed every check (lane sums match the
    /// totals, global ceiling was respected, every block was classified).
    pub balanced: bool,
    /// Stable alert id emitted on the FIRST failure (deterministic order).
    /// `None` when the reconciliation is balanced.
    pub alert: Option<String>,
}

impl BudgetReconciliation {
    /// Schema version of the reconciliation shape. Bump on intentional
    /// breaking changes to the row layout.
    pub const SCHEMA_VERSION: u32 = 1;

    /// Stable alert id for a per-lane-vs-total mismatch on selected tokens.
    pub const ALERT_LANE_SELECTED_MISMATCH: &'static str = "lane_selected_mismatch";
    /// Stable alert id for a per-lane-vs-total mismatch on delivered tokens.
    pub const ALERT_LANE_DELIVERED_MISMATCH: &'static str = "lane_delivered_mismatch";
    /// Stable alert id for a selected-vs-global-ceiling breach.
    pub const ALERT_GLOBAL_CEILING_EXCEEDED: &'static str = "global_ceiling_exceeded";
    /// Stable alert id for a selected-vs-delivered mismatch on the consuming
    /// lanes (rendered). Drift must be visible.
    pub const ALERT_SELECTED_WITHOUT_DELIVERED: &'static str = "selected_without_delivered";
    /// Stable alert id for a block that the renderer could not classify.
    pub const ALERT_UNCLASSIFIED_LANE: &'static str = "budget_lane_unclassified";
}

/// The token count a block contributes to the lane accounting. Native,
/// resolver-backed, and metadata-only lanes contribute zero tokens; only the
/// rendered lane contributes its rendered tokens.
fn block_tokens_for_lane(block: &BlockV1) -> u32 {
    match block.lane {
        Some(BudgetLaneKind::Rendered) => block.rendered_tokens.unwrap_or(0),
        _ => 0,
    }
}

/// Reconcile a finalized packet against the single cross-provider budget.
/// Builds a fresh per-lane ledger from the packet's blocks, verifies the
/// per-lane totals match the global totals, and returns a receipt-side
/// reconciliation.
pub fn reconcile(packet: &ContextPacketV1) -> BudgetReconciliation {
    let max_tokens = packet.budget.max_tokens;

    let mut accounting: Vec<LaneAccounting> = BUDGET_LANE_KINDS
        .iter()
        .map(|lane| LaneAccounting::new(*lane))
        .collect();

    let mut unclassified: u32 = 0;
    for block in &packet.blocks {
        let lane = match block.lane {
            Some(lane) => lane,
            None => {
                unclassified = unclassified.saturating_add(1);
                continue;
            }
        };
        let entry = accounting
            .iter_mut()
            .find(|entry| entry.lane == lane)
            .expect("lane present in the accounting ledger");
        entry.record(
            block.selected_tokens.unwrap_or(0),
            block_tokens_for_lane(block),
            block.delivered_chars.unwrap_or(0),
        );
    }

    let total_selected_tokens: u32 = accounting.iter().map(|row| row.selected_tokens).sum();
    let total_delivered_tokens: u32 = accounting.iter().map(|row| row.delivered_tokens).sum();
    let total_delivered_chars: u32 = accounting.iter().map(|row| row.delivered_chars).sum();

    let ceiling_exceeded = max_tokens > 0 && total_selected_tokens > max_tokens;

    let selected_without_delivered = accounting
        .iter()
        .any(|row| row.lane.consumes_tokens() && row.selected_tokens > row.delivered_tokens);

    let mut alert: Option<String> = None;
    if unclassified > 0 {
        alert = Some(BudgetReconciliation::ALERT_UNCLASSIFIED_LANE.to_string());
    } else if ceiling_exceeded {
        alert = Some(BudgetReconciliation::ALERT_GLOBAL_CEILING_EXCEEDED.to_string());
    } else if selected_without_delivered {
        alert = Some(BudgetReconciliation::ALERT_SELECTED_WITHOUT_DELIVERED.to_string());
    }

    let lanes: Vec<BudgetReconciliationRowV1> = accounting
        .into_iter()
        .map(|row| BudgetReconciliationRowV1 {
            lane: row.lane,
            selected_tokens: row.selected_tokens,
            delivered_tokens: row.delivered_tokens,
            delivered_chars: row.delivered_chars,
            blocks: row.blocks,
        })
        .collect();

    let balanced = alert.is_none();

    BudgetReconciliation {
        schema_version: BudgetReconciliation::SCHEMA_VERSION,
        max_tokens,
        total_selected_tokens,
        total_delivered_tokens,
        total_delivered_chars,
        lanes,
        unclassified_blocks: unclassified,
        balanced,
        alert,
    }
}

/// Convenience: build a plan-side [`CrossProviderBudget`] from the
/// reconciliation. The reconciliation is the receipt; the budget is the
/// planner's pre-image. Building one from the other is a debug/inspection
/// helper, not the hot path.
pub fn budget_from_reconciliation(reconciliation: &BudgetReconciliation) -> CrossProviderBudget {
    let mut budget = CrossProviderBudget::new(reconciliation.max_tokens);
    let rendered_selected = reconciliation
        .lanes
        .iter()
        .find(|row| row.lane == BudgetLaneKind::Rendered)
        .map(|row| row.selected_tokens)
        .unwrap_or(0);
    let _ = budget.try_select(BudgetLaneKind::Rendered, rendered_selected);
    budget
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::{
        BlockV1, BudgetV1, ContextPacketV1, DeliveryClass, DeliveryStage, DropReason,
    };
    use std::collections::BTreeMap;

    fn block(id: &str, class: DeliveryClass, selected: u32, rendered: u32, chars: u32) -> BlockV1 {
        BlockV1 {
            id: id.to_string(),
            layer: 0,
            provider: "federated".to_string(),
            source_kind: "doc".to_string(),
            source_ref: id.to_string(),
            source_hash: format!("sha256:{}", "0".repeat(64)),
            trust_class: "trusted".to_string(),
            instruction_policy: "data_only".to_string(),
            base_commit: None,
            overlay_digest: None,
            freshness_class: None,
            snapshot_id: None,
            priority: 0,
            estimated_tokens: selected,
            delivery_stage: Some(DeliveryStage::Finalized),
            delivery_class: Some(class),
            selected_tokens: Some(selected),
            allotted_tokens: Some(selected),
            rendered_tokens: Some(rendered),
            delivered_chars: Some(chars),
            drop_reason: Some(DropReason::None),
            protected: false,
            recoverable: true,
            resolver: "read".to_string(),
            delivery_mode: None,
            lane: None,
            text: "".to_string(),
        }
    }

    fn packet(blocks: Vec<BlockV1>, max_tokens: u32) -> ContextPacketV1 {
        ContextPacketV1 {
            schema_version: 1,
            trace_id: "t1".to_string(),
            task: "task".to_string(),
            mode: "mode".to_string(),
            budget: BudgetV1 {
                max_tokens,
                admitted_tokens: 0,
                packet_char_budget_default: None,
                packet_char_budget_override: None,
                packet_char_budget_model: None,
                configured_packet_char_budget: None,
                effective_packet_char_budget: None,
            },
            allocations: BTreeMap::new(),
            provider_accounting: BTreeMap::new(),
            blocks,
            omissions: vec![],
            reconciliation: None,
        }
    }

    #[test]
    fn reconcile_balances_when_per_lane_totals_equal_global() {
        let mut r1 = block("r1", DeliveryClass::Rendered, 25, 25, 100);
        r1.lane = Some(BudgetLaneKind::Rendered);
        let mut r2 = block("r2", DeliveryClass::Rendered, 25, 25, 100);
        r2.lane = Some(BudgetLaneKind::Rendered);
        let mut n1 = block("n1", DeliveryClass::MetadataOnly, 0, 0, 0);
        n1.lane = Some(BudgetLaneKind::MetadataOnly);
        let packet = packet(vec![r1, r2, n1], 100);
        let reconciliation = reconcile(&packet);
        assert!(reconciliation.balanced, "alert={:?}", reconciliation.alert);
        assert_eq!(reconciliation.total_selected_tokens, 50);
        assert_eq!(reconciliation.total_delivered_tokens, 50);
        assert_eq!(reconciliation.total_delivered_chars, 200);
        assert_eq!(reconciliation.unclassified_blocks, 0);
        assert_eq!(reconciliation.lanes.len(), BUDGET_LANE_KINDS.len());
    }

    #[test]
    fn reconcile_emits_selected_without_delivered_alert_for_drift() {
        let mut r1 = block("r1", DeliveryClass::Rendered, 50, 30, 100);
        r1.lane = Some(BudgetLaneKind::Rendered);
        let packet = packet(vec![r1], 100);
        let reconciliation = reconcile(&packet);
        assert!(!reconciliation.balanced);
        assert_eq!(
            reconciliation.alert,
            Some(BudgetReconciliation::ALERT_SELECTED_WITHOUT_DELIVERED.to_string())
        );
    }

    #[test]
    fn reconcile_emits_ceiling_exceeded_alert() {
        let mut r1 = block("r1", DeliveryClass::Rendered, 200, 200, 0);
        r1.lane = Some(BudgetLaneKind::Rendered);
        let packet = packet(vec![r1], 100);
        let reconciliation = reconcile(&packet);
        assert!(!reconciliation.balanced);
        assert_eq!(
            reconciliation.alert,
            Some(BudgetReconciliation::ALERT_GLOBAL_CEILING_EXCEEDED.to_string())
        );
    }

    #[test]
    fn reconcile_emits_unclassified_alert_for_block_without_lane() {
        let mut r1 = block("r1", DeliveryClass::Rendered, 10, 10, 0);
        r1.lane = None; // contract drift: the renderer forgot to stamp the lane
        let packet = packet(vec![r1], 100);
        let reconciliation = reconcile(&packet);
        assert!(!reconciliation.balanced);
        assert_eq!(reconciliation.unclassified_blocks, 1);
        assert_eq!(
            reconciliation.alert,
            Some(BudgetReconciliation::ALERT_UNCLASSIFIED_LANE.to_string())
        );
    }

    #[test]
    fn reconcile_records_native_blocks_at_zero_tokens() {
        let mut n1 = block("n1", DeliveryClass::MetadataOnly, 0, 0, 0);
        n1.lane = Some(BudgetLaneKind::Native);
        let packet = packet(vec![n1], 100);
        let reconciliation = reconcile(&packet);
        assert!(reconciliation.balanced);
        let native_row = reconciliation
            .lanes
            .iter()
            .find(|row| row.lane == BudgetLaneKind::Native)
            .unwrap();
        assert_eq!(native_row.blocks, 1);
        assert_eq!(native_row.selected_tokens, 0);
        assert_eq!(native_row.delivered_tokens, 0);
    }

    #[test]
    fn budget_from_reconciliation_replays_global_total() {
        let mut r1 = block("r1", DeliveryClass::Rendered, 40, 40, 0);
        r1.lane = Some(BudgetLaneKind::Rendered);
        let packet = packet(vec![r1], 80);
        let reconciliation = reconcile(&packet);
        let budget = budget_from_reconciliation(&reconciliation);
        assert_eq!(budget.max_tokens, 80);
        assert_eq!(budget.selected_tokens, 40);
    }
}
