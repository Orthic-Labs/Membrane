//! MBR-108: separation of concerns between the user-facing application plane, the
//! lifecycle/control plane (resident, install, lifecycle, restart), and persistent data
//! plane (SQLite, snapshots, receipts).
//!
//! The planes are typed responsibility boundaries, not separate resident process identities.
//! Hub hosts resident Application, Control, and Data work in-process; bounded stateless clients
//! may also execute Application-plane adapters. The Data plane never owns a network port. The
//! Control plane never opens SQLite. The Application plane never writes to disk except via the
//! Control or Data plane. This module is the typed contract every other plane consumer reads;
//! adding a fourth plane is a breaking change.

/// The three responsibility planes composed by Membrane.
///
/// Order matters: callers iterate `PLANE_BOUNDARIES` in declaration order and expect
/// Application, Control, Data in that order. The mode → plane mapping in
/// `membrane::modes::plane_of` and the `schemas/registry/plane-boundaries.v1.golden.json` fixture
/// must stay aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Plane {
    /// User-facing surface: CLI subcommands, stdio MCP, loopback HTTP API. Reads from the
    /// Data plane through `MemoryStore`. Never opens SQLite or writes lifecycle bindings directly.
    Application,
    /// Lifecycle surface: resident launch, frame validation, lockfile management, heartbeat
    /// publication. Owns lifecycle authority. Never opens SQLite itself; writes
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
    /// Whether this plane owns a separate resident process. This is false for all three planes:
    /// the active Hub owns the sole resident process. Bounded stateless clients do not establish
    /// plane residency.
    pub owns_process: bool,
    /// Source paths owned by this plane. The runtime enforces that only this plane may write
    /// to files under these paths.
    pub owns_files: &'static [&'static str],
    /// Planes this plane may read from. The Application plane reads from Data; the Control
    /// plane reads from Data to validate heartbeat presence; the Data plane reads from none.
    pub reads_from: &'static [Plane],
    /// Planes this plane may write to. Only the Control plane writes to Data (heartbeats,
    /// lifecycle receipts). The Application plane writes nothing directly — it forwards through
    /// Control or Data.
    pub writes_to: &'static [Plane],
}

/// Canonical three-plane boundary table. Mirrors
/// `schemas/registry/plane-boundaries.v1.golden.json` byte-for-byte. Any change here MUST be
/// reflected in that fixture.
pub const PLANE_BOUNDARIES: &[PlaneBoundary] = &[
    PlaneBoundary {
        name: "application",
        owns_process: false,
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
        owns_process: false,
        owns_files: &[
            "apps/membrane-hub/src-tauri/src/supervisor.rs",
            "engine/crates/membrane-runtime/src/service.rs",
            "engine/crates/membrane-protocol/src/lease.rs",
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
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.ends_with("apps/membrane-hub/src-tauri/src/supervisor.rs")
        || normalized.ends_with("engine/crates/membrane-runtime/src/service.rs")
        || normalized.ends_with("engine/crates/membrane-protocol/src/lease.rs")
    {
        return Some(Plane::Control);
    }
    for component in path.components() {
        let name = component.as_os_str().to_str();
        if let Some(segment) = name {
            match segment {
                // Application plane: anything inside the runtime crate or the stdio MCP crate.
                "membrane-runtime" | "membrane-mcp" | "membrane" => {
                    return Some(Plane::Application)
                }
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
    fn plane_for_path_classifies_control_resident_files() {
        for path in [
            Path::new("/x/apps/membrane-hub/src-tauri/src/supervisor.rs"),
            Path::new("/x/engine/crates/membrane-runtime/src/service.rs"),
            Path::new("/x/engine/crates/membrane-protocol/src/lease.rs"),
        ] {
            assert_eq!(plane_for_path(path), Some(Plane::Control));
        }
    }

    #[test]
    fn all_planes_are_hosted_by_the_one_hub_process() {
        assert!(PLANE_BOUNDARIES.iter().all(|plane| !plane.owns_process));
    }

    #[test]
    fn golden_fixture_matches_hub_owned_process_topology() {
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../schemas/registry/plane-boundaries.v1.golden.json"
        ))
        .unwrap();
        let planes = golden["planes"].as_array().unwrap();
        assert!(planes
            .iter()
            .all(|plane| plane["ownsProcess"] == serde_json::Value::Bool(false)));
        assert_eq!(
            golden["modeMapping"],
            serde_json::json!({
                "Cli": "application",
                "StdioMcp": "application",
                "Install": "application",
                "Uninstall": "application",
                "MigrateLegacy": "application"
            })
        );
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
