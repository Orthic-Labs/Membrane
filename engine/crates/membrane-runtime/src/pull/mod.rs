//! Pull owns evidence acquisition, admission, fusion, publication, and the
//! final faithful reduction/recovery path used for context delivery.
//!
//! The implementation files remain independently testable, but their public
//! runtime namespace is deliberately `membrane_runtime::pull`.  There is no
//! root-level compatibility export: callers must name the semantic axis they
//! depend on.

pub mod admission;
pub mod delivery_state;
pub mod delivery_acknowledgement;
pub mod cli;
pub mod federation;
pub(crate) mod federation_sources;
pub mod metrics;
pub mod placement;
pub(crate) mod native_federation;
pub mod publication;

// Reduction, selection, protected-span validation, spill, and recovery are
// Pull-owned implementation modules. Files retain their historical location
// until a source-compatible move can be made without changing the wire.
#[path = "../push/compress.rs"]
pub mod compress;
#[path = "../push/compression_provider.rs"]
pub mod compression_provider;
#[path = "../push/prep.rs"]
pub mod prep;
#[path = "../push/runc.rs"]
pub mod runc;
#[path = "../push/selection.rs"]
pub mod selection;
#[path = "../push/skel.rs"]
pub mod skel;
#[path = "../push/telemetry.rs"]
pub mod telemetry;
#[path = "../push/truncate.rs"]
pub mod truncate;
#[path = "../push/recovery.rs"]
pub mod recovery;
#[path = "../push/fidelity.rs"]
pub mod fidelity;
#[path = "../push/delivery.rs"]
pub mod delivery;
#[path = "../push/ast.rs"]
pub mod ast;
#[path = "../push/egress.rs"]
pub mod egress;
#[path = "../push/api.rs"]
pub mod api;
#[path = "../push/packet_selection.rs"]
pub mod packet_selection;

pub use prep::{
    prep_files_with_budget_and_policy, prep_files_with_policy, PrepPolicy, PushPolicy,
    QueryAwarePolicy,
};

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
