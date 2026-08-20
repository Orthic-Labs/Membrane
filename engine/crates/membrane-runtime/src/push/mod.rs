//! Push owns faithful, recoverable context reduction.
//!
//! Pull owns selection and headroom. Push owns the faithful transform lineage
//! (`runc` → `skel` → `compress` → `truncate`) and never opens Cortex storage.
//! Transform observations are bounded process-local evidence for the Hub lane.

pub mod compress;
pub mod compression_provider;
pub mod prep;
pub mod runc;
pub mod skel;
pub mod truncate;
pub mod telemetry;

/// Push's stable operation identity, used by diagnostics and capability
/// reports so every surface exposes the same six-axis vocabulary.
pub const AXIS: &str = "push";
pub const OPERATION_NAMESPACE: &str = "membrane.push";
