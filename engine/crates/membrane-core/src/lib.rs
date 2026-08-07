//! # membrane-core
//!
//! The **cross-provider attention budget** owner. Keeps one cross-provider
//! budget with explicit lanes (MBR-608) so every receipt reconciles its
//! selected and delivered tokens under one budget.
//!
//! ## Lanes
//!
//! Every block is admitted under exactly one of four explicit lanes
//! ([`membrane_protocol::BudgetLaneKind`]):
//!
//!   * [`BudgetLaneKind::Native`]         — host-loaded content with a
//!                                          matching host receipt (MBR-010).
//!                                          Zero bytes; zero tokens.
//!   * [`BudgetLaneKind::Rendered`]       — inline text the renderer
//!                                          serialized into the prompt.
//!   * [`BudgetLaneKind::ResolverBacked`] — resolver reference only; agent
//!                                          retrieves on demand. Zero tokens.
//!   * [`BudgetLaneKind::MetadataOnly`]   — metadata without content. Zero
//!                                          tokens; zero bytes.
//!
//! Lanes are mutually exclusive per block. The cross-provider total is the
//! sum of the per-lane totals — never a separate counter.
//!
//! ## Layout
//!
//!   * [`lane`]   — [`BudgetLaneKind`] (re-exported), [`LaneAllocation`],
//!                  [`LaneAccounting`], and the [`lane_from_block`] helper.
//!   * [`budget`] — [`CrossProviderBudget`], the single global ceiling.
//!   * [`reconcile`] — the receipt-side [`BudgetReconciliation`].
//!
//! The protocol-level types live in `membrane-protocol` so both Rust and
//! TypeScript can round-trip them. This crate builds the reconciliation
//! logic on top of those types.

pub mod budget;
pub mod lane;
pub mod reconcile;

pub use budget::CrossProviderBudget;
pub use lane::{
    lane_from_block, LaneAccounting, LaneAllocation, BUDGET_LANE_KINDS,
};
pub use membrane_protocol::BudgetLaneKind;
pub use reconcile::{
    budget_from_reconciliation, reconcile, BudgetReconciliation,
    BudgetReconciliationRowV1,
};
