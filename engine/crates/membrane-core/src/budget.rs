//! The single cross-provider attention budget.
//!
//! The budget is a global token ceiling + one allocation per lane. The
//! planner fills it, the renderer emits against it, and the reconciliation
//! proves the receipt's selected-vs-delivered totals agree across every
//! lane and provider.
//!
//! The cross-provider total is the sum of the per-lane totals. There is no
//! separate "provider total" — providers contribute to lanes, and the lane
//! totals are the only source of truth.

use crate::lane::{LaneAllocation, BUDGET_LANE_KINDS};
use membrane_protocol::BudgetLaneKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The one cross-provider attention budget. Holds the global ceiling and one
/// allocation per lane. Serialize so receipts can carry the plan-side state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CrossProviderBudget {
    /// The single global token ceiling the budget enforces.
    pub max_tokens: u32,
    /// The number of tokens the planner selected across every lane.
    pub selected_tokens: u32,
    /// One allocation per lane, keyed by lane kind. The map is populated by
    /// `new` and never reordered; reconciliations iterate in
    /// [`BUDGET_LANE_KINDS`] order, not by map insertion.
    pub lanes: BTreeMap<String, LaneAllocation>,
}

impl CrossProviderBudget {
    /// Build a budget with the global ceiling and one zero-default allocation
    /// per lane. `max_tokens` is the cross-provider total; the per-lane caps
    /// are derived from `default_lane_share` (defaulting to the full ceiling
    /// for the single consuming lane) but may be re-capped individually with
    /// [`set_lane_cap`].
    pub fn new(max_tokens: u32) -> Self {
        let consuming = BUDGET_LANE_KINDS
            .iter()
            .filter(|lane| lane.consumes_tokens())
            .count() as u32;
        let default_lane_share = if consuming > 0 {
            max_tokens / consuming
        } else {
            0
        };
        let mut lanes = BTreeMap::new();
        for lane in BUDGET_LANE_KINDS {
            let max_for_lane = if lane.consumes_tokens() {
                default_lane_share
            } else {
                0
            };
            lanes.insert(lane.as_str().to_string(), LaneAllocation::new(*lane, max_for_lane));
        }
        Self {
            max_tokens,
            selected_tokens: 0,
            lanes,
        }
    }

    /// Replace the per-lane cap. Useful for plans that want to reserve a
    /// lane's full capacity for a single provider block.
    pub fn set_lane_cap(&mut self, lane: BudgetLaneKind, max_tokens: u32) {
        let entry = self
            .lanes
            .entry(lane.as_str().to_string())
            .or_insert_with(|| LaneAllocation::new(lane, max_tokens));
        entry.max_tokens = if lane.consumes_tokens() {
            max_tokens
        } else {
            0
        };
    }

    /// Try to admit `tokens` against the named lane. Records the selection on
    /// the lane and the cross-provider total only when the lane has capacity
    /// and the global ceiling is not exceeded.
    pub fn try_select(&mut self, lane: BudgetLaneKind, tokens: u32) -> bool {
        // Non-consuming lanes do not count toward the global token ceiling.
        if !lane.consumes_tokens() {
            let entry = self
                .lanes
                .entry(lane.as_str().to_string())
                .or_insert_with(|| LaneAllocation::new(lane, 0));
            return entry.try_select(tokens);
        }
        if tokens > self.max_tokens.saturating_sub(self.selected_tokens) {
            return false;
        }
        let entry = self
            .lanes
            .entry(lane.as_str().to_string())
            .or_insert_with(|| LaneAllocation::new(lane, 0));
        if !entry.try_select(tokens) {
            return false;
        }
        self.selected_tokens = self.selected_tokens.saturating_add(tokens);
        true
    }

    /// The selected tokens held by the named lane.
    pub fn lane_selected(&self, lane: BudgetLaneKind) -> u32 {
        self.lanes
            .get(lane.as_str())
            .map(|entry| entry.selected_tokens)
            .unwrap_or(0)
    }

    /// Walk the lanes in the contract order. The map iterates alphabetically,
    /// but reconciliation must follow [`BUDGET_LANE_KINDS`] order.
    pub fn ordered_lanes(&self) -> impl Iterator<Item = &LaneAllocation> {
        BUDGET_LANE_KINDS.iter().filter_map(move |lane| {
            self.lanes.get(lane.as_str())
        })
    }

    /// Reserve headroom (`max_tokens - selected_tokens`), clamped to zero.
    pub fn remaining_tokens(&self) -> u32 {
        self.max_tokens.saturating_sub(self.selected_tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::BudgetLaneKind;

    #[test]
    fn new_budget_exposes_one_allocation_per_lane() {
        let budget = CrossProviderBudget::new(400);
        assert_eq!(budget.max_tokens, 400);
        assert_eq!(budget.selected_tokens, 0);
        for lane in BUDGET_LANE_KINDS {
            let stored = budget.lanes.get(lane.as_str()).expect("lane present");
            assert_eq!(stored.lane, *lane);
            if lane.consumes_tokens() {
                assert!(stored.max_tokens > 0, "rendered lane must have a cap");
            } else {
                assert_eq!(stored.max_tokens, 0, "non-consuming lane must be zero");
            }
        }
    }

    #[test]
    fn try_select_records_against_lane_and_global() {
        let mut budget = CrossProviderBudget::new(200);
        assert!(budget.try_select(BudgetLaneKind::Rendered, 60));
        assert!(budget.try_select(BudgetLaneKind::Rendered, 40));
        assert_eq!(budget.lane_selected(BudgetLaneKind::Rendered), 100);
        assert_eq!(budget.selected_tokens, 100);
    }

    #[test]
    fn try_select_rejects_when_global_ceiling_reached() {
        let mut budget = CrossProviderBudget::new(100);
        assert!(budget.try_select(BudgetLaneKind::Rendered, 100));
        assert!(!budget.try_select(BudgetLaneKind::Rendered, 1));
        assert_eq!(budget.selected_tokens, 100);
        assert_eq!(budget.remaining_tokens(), 0);
    }

    #[test]
    fn try_select_accepts_non_consuming_lanes_without_recording_tokens() {
        let mut budget = CrossProviderBudget::new(100);
        assert!(budget.try_select(BudgetLaneKind::Native, 50));
        assert!(budget.try_select(BudgetLaneKind::MetadataOnly, 50));
        assert_eq!(budget.selected_tokens, 0);
        assert_eq!(budget.remaining_tokens(), 100);
    }

    #[test]
    fn lanes_iterate_in_contract_order() {
        let budget = CrossProviderBudget::new(400);
        let order: Vec<_> = budget.ordered_lanes().map(|lane| lane.lane).collect();
        assert_eq!(order, BUDGET_LANE_KINDS.to_vec());
    }

    #[test]
    fn set_lane_cap_clears_capacity_for_non_consuming_lanes() {
        let mut budget = CrossProviderBudget::new(100);
        budget.set_lane_cap(BudgetLaneKind::Native, 999);
        assert_eq!(budget.lane_selected(BudgetLaneKind::Native), 0);
        assert_eq!(budget.lanes.get("native").unwrap().max_tokens, 0);
    }
}
