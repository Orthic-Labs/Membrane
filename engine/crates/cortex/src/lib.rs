//! Cortex durable-memory public API.
//!
//! Membrane owns orchestration & its Pull, Push, Ledger, Blueprint, & Adapt
//! namespaces. Cortex exposes only durable-memory storage primitives here.
//!
//! This crate is the stable executable-facing API: consumers depend on its
//! semver and schema-versioned records, while runtime module layout stays
//! private to the implementation.

pub use membrane_runtime::{MemDb, MemoryStore};

/// Explicit durable-memory support surface. Runtime orchestration modules are
/// intentionally not re-exported from Cortex, with the single exception of
/// `lifecycle`: the governed admission/promotion/review/Dream-intake entry
/// points below, which are themselves typed, storage-bound operations rather
/// than service orchestration.
pub mod durable {
    pub use membrane_runtime::{
        context_telemetry, feedback, memdb, memory_provider, scope, CheckpointError,
        CheckpointSourceRefV1, CheckpointSourceResolutionV1, CheckpointV1,
    };
}

pub use durable::{
    CheckpointError, CheckpointSourceRefV1, CheckpointSourceResolutionV1, CheckpointV1,
};

/// Governed Cortex lifecycle surface: proposal intake (including bounded
/// Dream-sourced background proposals), independently signed review,
/// checkpoint promotion, and bounded pending recovery. `LifecycleError` is
/// the typed failure for unavailable/invalid inputs (`code`, `message`,
/// `retryable`) — callers must not infer availability from success alone.
/// Added alongside the existing durable-memory surface; no V1 shape above is
/// changed or removed.
pub mod lifecycle {
    pub use membrane_runtime::cortex_lifecycle::{
        drain_background_proposals, proposal_status, propose, propose_multiwriter,
        propose_temporal, promote_checkpoint, recall_recipe, recover_pending, resolve_memory,
        review, trust_path, LifecycleError, ReviewedEffectV1, ReviewerKeyV1, ReviewerTrustV1,
    };
}

pub use lifecycle::LifecycleError;
