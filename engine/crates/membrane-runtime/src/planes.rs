//! MBR-108: separation of concerns between the user-facing application plane, the
//! lifecycle/control plane (supervisor, install, lease, restart), and the persistent data
//! plane (SQLite, snapshots, receipts).
//!
//! Each plane lives in its own failure domain. The Data plane never owns a network port. The
//! Control plane never opens SQLite. The Application plane never writes to disk except via the
//! Control or Data plane. This module is the typed contract every other plane consumer reads;
//! adding a fourth plane is a breaking change.

/// The three process planes the membrane executable is partitioned into.
///
/// Order matters: callers iterate `PLANE_BOUNDARIES` in declaration order and expect
/// Application, Control, Data in that order. The mode → plane mapping in
/// `membrane::modes::plane_of` and the `schemas/registry/plane-boundaries.v1.golden.json` fixture
/// must stay aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Plane {
    /// User-facing surface: CLI subcommands, stdio MCP, loopback HTTP API. Reads from the
    /// Data plane through `MemoryStore`. Never opens SQLite or writes leases directly.
    Application,
    /// Lifecycle surface: supervisor-child launch, lease validation, lockfile management,
    /// heartbeat publication. Owns the lease authority. Never opens SQLite itself; writes
    /// heartbeat rows by going through the Data plane API.
    Control,
    /// Persistent storage surface: SQLite catalog, receipts, snapshot manifests. Never owns
    /// a network port. Exposes a typed read/write API consumed by the other two planes.
    Data,
}

impl Plane {
    pub fn as_str(&self) -> &'static str {
        match self {
            Plane::Application => "application",
            Plane::Control => "control",
            Plane::Data => "data",
        }
    }
}

/// Static description of one plane's process boundary, file ownership, and cross-plane
/// data flow. The `PLANE_BOUNDARIES` slice below is the canonical source of truth.
#[derive(Debug, Clone, Copy)]
pub struct PlaneBoundary {
    /// Lower-case plane name. Matches `Plane::as_str()` for the matching variant.
    pub name: &'static str,
    /// Whether this plane owns its own process (true) or runs as a tokio task under a single
    /// supervisor (false). The Data plane is `false` today — it shares the supervisor-child
    /// process — and the Application and Control planes are `true`.
    pub owns_process: bool,
    /// Source paths owned by this plane. The runtime enforces that only this plane may write
    /// to files under these paths.
    pub owns_files: &'static [&'static str],
    /// Planes this plane may read from. The Application plane reads from Data; the Control
    /// plane reads from Data to validate heartbeat presence; the Data plane reads from none.
    pub reads_from: &'static [Plane],
    /// Planes this plane may write to. Only the Control plane writes to Data (heartbeats,
    /// lease receipts). The Application plane writes nothing directly — it forwards through
    /// Control or Data.
    pub writes_to: &'static [Plane],
}

/// Canonical three-plane boundary table. Mirrors
/// `schemas/registry/plane-boundaries.v1.golden.json` byte-for-byte. Any change here MUST be
/// reflected in that fixture.
pub const PLANE_BOUNDARIES: &[PlaneBoundary] = &[
    PlaneBoundary {
        name: "application",
        owns_process: true,
        owns_files: &[
            "engine/crates/membrane-runtime/src/cli.rs",
            "engine/crates/membrane-runtime/src/serve.rs",
            "engine/crates/membrane-runtime/src/planes.rs",
            "engine/crates/membrane-mcp/src/",
        ],
        reads_from: &[Plane::Data],
        writes_to: &[],
    },
    PlaneBoundary {
        name: "control",
        owns_process: true,
        owns_files: &[
            "engine/crates/membrane-supervisor/src/supervisor.rs",
            "engine/crates/membrane-supervisor/src/resident.rs",
            "engine/crates/membrane-supervisor/src/lease.rs",
            "engine/crates/membrane-supervisor/src/lock.rs",
            "engine/crates/membrane-supervisor/src/watcher.rs",
        ],
        reads_from: &[Plane::Data],
        writes_to: &[Plane::Data],
    },
    PlaneBoundary {
        name: "data",
        owns_process: false,
        owns_files: &[
            "engine/crates/cortex-store/src/db.rs",
            "engine/crates/cortex-store/src/memdb.rs",
            "engine/crates/cortex-store/src/scope.rs",
            "engine/crates/cortex-store/src/context_telemetry.rs",
            "engine/crates/cortex-store/src/installation_identity.rs",
        ],
        reads_from: &[],
        writes_to: &[],
    },
];

/// Classify a file path into a plane by walking its components and matching the crate
/// segment that owns the file. Pure function — no filesystem I/O.
///
/// Returns `None` for paths that don't belong to any of the three planes (for example,
/// unrelated workspace files or generated artifacts outside the engine).
pub fn plane_for_path(path: &std::path::Path) -> Option<Plane> {
    for component in path.components() {
        let name = component.as_os_str().to_str();
        if let Some(segment) = name {
            match segment {
                // Application plane: anything inside the runtime crate or the stdio MCP crate.
                "membrane-runtime" | "membrane-mcp" => return Some(Plane::Application),
                // Control plane: anything inside the supervisor crate.
                "membrane-supervisor" => return Some(Plane::Control),
                // Data plane: anything inside the cortex-store crate.
                "cortex-store" => return Some(Plane::Data),
                _ => {}
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn plane_for_path_classifies_application_runtime_files() {
        let path = Path::new("/x/membrane-runtime/src/cli.rs");
        assert_eq!(plane_for_path(path), Some(Plane::Application));
    }

    #[test]
    fn plane_for_path_classifies_control_supervisor_files() {
        let path = Path::new("/x/membrane-supervisor/src/supervisor.rs");
        assert_eq!(plane_for_path(path), Some(Plane::Control));
    }

    #[test]
    fn plane_for_path_classifies_data_store_files() {
        let path = Path::new("/x/cortex-store/src/db.rs");
        assert_eq!(plane_for_path(path), Some(Plane::Data));
    }

    #[test]
    fn plane_for_path_returns_none_for_unrelated_paths() {
        let path = Path::new("/x/some-other-crate/src/lib.rs");
        assert_eq!(plane_for_path(path), None);
    }
}
