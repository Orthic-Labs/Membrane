//! MBR: shared read-only sqlite access for Hub producers (sources, adapters,
//! sentinel). Each producer needs the same local cortex-engine database; this
//! module centralizes path resolution and connection opening so every
//! producer fails closed the same way — any missing file, locked database,
//! or open error yields `None` rather than a fabricated read.

use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags};

/// Mirrors `hub_inputs::configured_workspace_root` (private to that module),
/// so we replicate the same env-var precedence here rather than reach across
/// a module boundary that wasn't designed to be shared.
pub fn configured_workspace_root() -> PathBuf {
    std::env::var_os("MEMBRANE_REPO_ROOT")
        .or_else(|| std::env::var_os("WORKSPACE_ROOT"))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Resolves the local cortex-engine database path. `CORTEX_DB_PATH` overrides
/// for tests/alternate installs; otherwise the workspace-relative default.
pub fn configured_db_path() -> PathBuf {
    std::env::var_os("CORTEX_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| configured_workspace_root().join("tools/.cache/memory/cortex-engine.db"))
}

/// Best-effort read-only open of the configured database. Returns `None` on
/// any failure — missing file, locked database, corrupt header — so callers
/// fail closed instead of fabricating a healthy read.
pub fn open_readonly() -> Option<Connection> {
    let path = configured_db_path();
    if !path.is_file() {
        return None;
    }
    Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
}

pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
