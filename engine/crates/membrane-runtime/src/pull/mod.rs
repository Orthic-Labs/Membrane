//! Pull owns evidence acquisition, admission, fusion, and publication.
//!
//! The implementation files remain independently testable, but their public
//! runtime namespace is deliberately `membrane_runtime::pull`.  There is no
//! root-level compatibility export: callers must name the semantic axis they
//! depend on.

pub mod admission;
pub mod cli;
pub mod federation;
pub mod federation_worker;
pub mod metrics;
pub mod publication;

/// Pull is the only runtime namespace that exposes final evidence admission.
/// The pure policy remains implemented by `cortex-core`; this is its single
/// Membrane-owned public route.
pub mod planner {
    pub use cortex_core::planner::*;
}

/// Pull's stable operation identity, used by diagnostics and capability
/// reports so every surface exposes the same six-axis vocabulary.
pub const AXIS: &str = "pull";
pub const OPERATION_NAMESPACE: &str = "membrane.pull";
