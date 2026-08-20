//! Cortex durable-memory public API.
//!
//! Membrane owns orchestration & its Pull, Push, Guide, Blueprint, & Adapt
//! namespaces. Cortex exposes only durable-memory storage primitives here.

pub use membrane_runtime::{MemDb, MemoryStore};

/// Explicit durable-memory support surface. Runtime orchestration modules are
/// intentionally not re-exported from Cortex.
pub mod durable {
    pub use membrane_runtime::{
        context_telemetry, feedback, memdb, memory_provider, scope,
        CheckpointError, CheckpointSourceRefV1, CheckpointSourceResolutionV1, CheckpointV1,
    };
}

pub use durable::{
    CheckpointError, CheckpointSourceRefV1, CheckpointSourceResolutionV1, CheckpointV1,
};
