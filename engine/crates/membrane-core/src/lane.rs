//! Cross-provider budget lanes.
//!
//! Every Membrane block is admitted under exactly one of four explicit lanes
//! ([`membrane_protocol::BudgetLaneKind`]). The lane is the contract surface
//! that reconciles selected tokens, delivered tokens, and delivered chars
//! across providers. A provider may emit blocks in any subset of lanes; the
//! lane is about how the block is delivered, not where it came from.
//!
//! Lane → delivery surface mapping (the contract):
//!
//!   * [`BudgetLaneKind::Native`]         — host loaded the content with a
//!                                          matching host receipt (MBR-010).
//!                                          Zero bytes; zero tokens.
//!   * [`BudgetLaneKind::Rendered`]       — the renderer serialized inline
//!                                          text. `renderedTokens` is the
//!                                          token count consumed.
//!   * [`BudgetLaneKind::ResolverBacked`] — only a resolver reference, the
//!                                          agent retrieves on demand. Zero
//!                                          tokens consumed; `deliveredChars`
//!                                          tracks the reference header.
//!   * [`BudgetLaneKind::MetadataOnly`]   — metadata without content. Zero
//!                                          tokens; zero bytes.
//!
//! The lanes are mutually exclusive per block. The cross-provider budget
//! totals are the sum of the per-lane totals — never a separate counter.

use membrane_protocol::BudgetLaneKind;
use serde::{Deserialize, Serialize};

// Re-export the protocol lane enum so downstream call sites can keep
// importing `membrane_core::BudgetLaneKind` without chasing the protocol.
pub use membrane_protocol::BudgetLaneKind as LaneKind;

/// The fixed, ordered set of lanes. The order is the contract order; both
/// the Rust and TypeScript reconciliations iterate this exact sequence so the
/// receipts never reorder.
pub const BUDGET_LANE_KINDS: &[BudgetLaneKind] = &[
    BudgetLaneKind::Native,
    BudgetLaneKind::Rendered,
    BudgetLaneKind::ResolverBacked,
    BudgetLaneKind::MetadataOnly,
];

/// Per-lane allocation — the planner's intended cap for a lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaneAllocation {
    pub lane: BudgetLaneKind,
    /// Maximum tokens this lane may consume. Zero for lanes that never
    /// consume tokens (native, resolver_backed, metadata_only).
    pub max_tokens: u32,
    /// Tokens the planner actually selected into this lane.
    pub selected_tokens: u32,
}

impl LaneAllocation {
    pub fn new(lane: BudgetLaneKind, max_tokens: u32) -> Self {
        Self {
            lane,
            max_tokens: if lane.consumes_tokens() {
                max_tokens
            } else {
                0
            },
            selected_tokens: 0,
        }
    }

    /// Try to reserve `tokens` against this lane. Returns `true` and adds to
    /// `selected_tokens` when the lane has capacity, `false` otherwise.
    pub fn try_select(&mut self, tokens: u32) -> bool {
        if !self.lane.consumes_tokens() {
            // Non-consuming lanes always accept; selection is recorded on the
            // accounting side, not here.
            return true;
        }
        let proposed = self.selected_tokens.saturating_add(tokens);
        if proposed > self.max_tokens {
            return false;
        }
        self.selected_tokens = proposed;
        true
    }
}

/// Per-lane accounting — the receipt-side observation of the lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaneAccounting {
    pub lane: BudgetLaneKind,
    /// Tokens the planner selected for this lane.
    pub selected_tokens: u32,
    /// Tokens the renderer/delivery actually emitted for this lane.
    pub delivered_tokens: u32,
    /// Unicode code points (chars) carried in this lane across all blocks.
    pub delivered_chars: u32,
    /// Number of blocks that occupied this lane.
    pub blocks: u32,
}

impl LaneAccounting {
    pub fn new(lane: BudgetLaneKind) -> Self {
        Self {
            lane,
            selected_tokens: 0,
            delivered_tokens: 0,
            delivered_chars: 0,
            blocks: 0,
        }
    }

    /// Record one block against this lane.
    pub fn record(&mut self, selected_tokens: u32, delivered_tokens: u32, delivered_chars: u32) {
        self.selected_tokens = self.selected_tokens.saturating_add(selected_tokens);
        self.delivered_tokens = self.delivered_tokens.saturating_add(delivered_tokens);
        self.delivered_chars = self.delivered_chars.saturating_add(delivered_chars);
        self.blocks = self.blocks.saturating_add(1);
    }

    /// True when the lane's selected tokens were all delivered. Native,
    /// resolver-backed, and metadata-only lanes always satisfy this (their
    /// selected tokens are zero by construction).
    pub fn reconciled(&self) -> bool {
        if self.lane.consumes_tokens() {
            self.selected_tokens == self.delivered_tokens
        } else {
            // Zero-token lanes: any non-zero selected figure is a contract
            // violation; otherwise the lane is balanced by construction.
            self.selected_tokens == 0
        }
    }
}

/// Derive the lane from a block's `deliveryClass` and `deliveryMode`. The
/// mapping is the MBR-608 contract; both the Rust and TypeScript
/// implementations must produce identical results.
pub fn lane_from_block(
    delivery_class: Option<&str>,
    delivery_mode: Option<&str>,
) -> Result<BudgetLaneKind, String> {
    // Native delivery is identified by the MBR-010 receipt verification, not
    // by deliveryClass. A block whose deliveryMode is "native" is always in
    // the native lane, regardless of its deliveryClass.
    if delivery_mode == Some("native") {
        return Ok(BudgetLaneKind::Native);
    }
    match delivery_class {
        Some("rendered") => Ok(BudgetLaneKind::Rendered),
        Some("metadata_only") => Ok(BudgetLaneKind::MetadataOnly),
        Some("resolver_backed") => Ok(BudgetLaneKind::ResolverBacked),
        Some(other) => Err(other.to_string()),
        None => Err("missing_delivery_class".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_string_contract_is_snake_case() {
        for lane in BUDGET_LANE_KINDS {
            let value = serde_json::to_value(lane).unwrap();
            assert!(
                value.is_string(),
                "lane must serialize as a string, got {value:?}"
            );
            let text = value.as_str().unwrap();
            assert!(
                text.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_'),
                "lane label must be snake_case: {text}"
            );
            assert_eq!(text, lane.as_str());
        }
    }

    #[test]
    fn consumes_tokens_only_for_rendered() {
        assert!(BudgetLaneKind::Rendered.consumes_tokens());
        assert!(!BudgetLaneKind::Native.consumes_tokens());
        assert!(!BudgetLaneKind::ResolverBacked.consumes_tokens());
        assert!(!BudgetLaneKind::MetadataOnly.consumes_tokens());
    }

    #[test]
    fn lane_from_block_records_native_lane_only_on_native_delivery_mode() {
        assert_eq!(
            lane_from_block(Some("rendered"), Some("native")),
            Ok(BudgetLaneKind::Native)
        );
        assert_eq!(
            lane_from_block(Some("metadata_only"), Some("native")),
            Ok(BudgetLaneKind::Native)
        );
        assert_eq!(
            lane_from_block(None, Some("native")),
            Ok(BudgetLaneKind::Native)
        );
    }

    #[test]
    fn lane_from_block_maps_delivery_class_for_non_native_modes() {
        assert_eq!(
            lane_from_block(Some("rendered"), Some("inline")),
            Ok(BudgetLaneKind::Rendered)
        );
        assert_eq!(
            lane_from_block(Some("metadata_only"), Some("reference")),
            Ok(BudgetLaneKind::MetadataOnly)
        );
        assert_eq!(
            lane_from_block(Some("resolver_backed"), Some("reference")),
            Ok(BudgetLaneKind::ResolverBacked)
        );
    }

    #[test]
    fn lane_from_block_rejects_unknown_delivery_class() {
        assert_eq!(
            lane_from_block(Some("inline"), Some("inline")),
            Err("inline".to_string())
        );
        assert_eq!(
            lane_from_block(None, Some("inline")),
            Err("missing_delivery_class".to_string())
        );
    }

    #[test]
    fn lane_allocation_respects_per_lane_cap() {
        let mut allocation = LaneAllocation::new(BudgetLaneKind::Rendered, 100);
        assert!(allocation.try_select(60));
        assert!(allocation.try_select(40));
        assert!(!allocation.try_select(1));
        assert_eq!(allocation.selected_tokens, 100);
    }

    #[test]
    fn lane_allocation_keeps_non_consuming_lanes_at_zero() {
        let mut allocation = LaneAllocation::new(BudgetLaneKind::Native, 999);
        assert_eq!(allocation.max_tokens, 0);
        assert!(allocation.try_select(500));
        assert_eq!(allocation.selected_tokens, 0);
    }

    #[test]
    fn lane_accounting_reconciles_only_when_delivered_matches_selected() {
        let mut accounting = LaneAccounting::new(BudgetLaneKind::Rendered);
        accounting.record(50, 50, 200);
        assert!(accounting.reconciled());

        accounting.record(25, 10, 100);
        assert!(!accounting.reconciled());
        assert_eq!(accounting.selected_tokens, 75);
        assert_eq!(accounting.delivered_tokens, 60);
    }

    #[test]
    fn lane_accounting_reconciles_zero_token_lanes_with_zero_selected() {
        let mut accounting = LaneAccounting::new(BudgetLaneKind::Native);
        accounting.record(0, 0, 0);
        assert!(accounting.reconciled());

        // Drift: somebody selected tokens for a zero-token lane.
        let mut drift = LaneAccounting::new(BudgetLaneKind::ResolverBacked);
        drift.record(10, 0, 0);
        assert!(!drift.reconciled());
    }
}
