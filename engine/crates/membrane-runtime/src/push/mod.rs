//! Push owns faithful, recoverable context reduction.
//!
//! Pull owns selection and headroom. Push owns the faithful transform lineage
//! (`runc` → `skel` → `compress` → `truncate`) and never opens Cortex storage.
//! Transform observations are bounded process-local evidence for the Hub lane.

pub mod compress;
pub mod compression_provider;
pub mod prep;
pub mod runc;
pub mod selection;
pub mod skel;
pub mod telemetry;
pub mod truncate;

pub use prep::{
    prep_files_with_budget_and_policy, prep_files_with_policy, PrepPolicy, PushPolicy,
    QueryAwarePolicy,
};

/// Push's stable operation identity, used by diagnostics and capability
/// reports so every surface exposes the same six-axis vocabulary.
pub const AXIS: &str = "push";
pub const OPERATION_NAMESPACE: &str = "membrane.push";
