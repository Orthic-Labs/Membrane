//! MBR / CTX-001: the single sanctioned read-only SQLite accessor.
//!
//! `cortex_store::memdb::MemDb::open` is the one *write* authority for the
//! Cortex durable database: it owns the WAL/busy-timeout pragmas, the
//! migration ladder and the schema-generation marker. Hub producers
//! (sources, adapters, sentinel, admission) are read-only consumers that must
//! never migrate a database they do not own — a producer that opened the DB
//! read-write would silently run the ladder from a possibly-older binary.
//!
//! So there is exactly one write authority (`MemDb::open`) and exactly one
//! sanctioned read-only accessor: [`open_readonly_sanctioned`]. It
//!
//! * opens strictly `SQLITE_OPEN_READ_ONLY` (never creates, never migrates),
//! * applies `busy_timeout` so a concurrent writer yields a wait, not a
//!   fabricated empty read,
//! * asserts `PRAGMA query_only` for defence in depth, and
//! * **fails closed on a schema-generation mismatch**: if `PRAGMA
//!   user_version` is not exactly the generation this binary was compiled
//!   against, the open is refused. A reader silently consuming an unknown
//!   schema is the defect this guards.
//!
//! Every refusal collapses to `None` at the producer boundary, which is how
//! producers already fail closed (no row, no fabricated read).
//!
//! No other caller may open a Cortex or catalog database ad hoc; the
//! `sanctioned_sqlite_open_sites_are_frozen` test below scans the sources and
//! fails when a new `Connection::open` / `open_with_flags` appears.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

/// Wait this long for a concurrent writer before reporting `SQLITE_BUSY`.
/// Matches `MemDb::open` / `Catalog::open` so readers and the writer agree.
const READONLY_BUSY_TIMEOUT_MS: u32 = 5_000;

/// Why a sanctioned read-only open was refused. Every variant is a
/// fail-closed outcome: the caller gets no connection and therefore no rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOnlyRefusal {
    /// No regular file at the resolved path.
    Absent,
    /// SQLite refused the open (locked, corrupt header, permissions).
    OpenFailed(String),
    /// The file opened but `PRAGMA user_version` could not be read.
    GenerationUnreadable(String),
    /// The file's schema generation is not the one this binary understands.
    GenerationMismatch { found: i64, expected: i64 },
}

impl ReadOnlyRefusal {
    /// Stable code for receipts/diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Absent => "database_absent",
            Self::OpenFailed(_) => "readonly_open_failed",
            Self::GenerationUnreadable(_) => "schema_generation_unreadable",
            Self::GenerationMismatch { .. } => "schema_generation_mismatch",
        }
    }
}

/// The one sanctioned read-only open. `expected_generation` is the
/// `PRAGMA user_version` this binary was compiled against; a database at any
/// other generation — older *or* newer — is refused rather than read.
pub fn open_readonly_sanctioned(
    path: &Path,
    expected_generation: i64,
) -> Result<Connection, ReadOnlyRefusal> {
    if !path.is_file() {
        return Err(ReadOnlyRefusal::Absent);
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| ReadOnlyRefusal::OpenFailed(error.to_string()))?;
    conn.busy_timeout(std::time::Duration::from_millis(u64::from(
        READONLY_BUSY_TIMEOUT_MS,
    )))
    .map_err(|error| ReadOnlyRefusal::OpenFailed(error.to_string()))?;
    // Defence in depth: the READ_ONLY flag already forbids writes; query_only
    // makes an attempted write fail inside SQLite rather than at the vfs.
    conn.execute_batch("PRAGMA query_only=1;")
        .map_err(|error| ReadOnlyRefusal::OpenFailed(error.to_string()))?;
    let found: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| ReadOnlyRefusal::GenerationUnreadable(error.to_string()))?;
    if found != expected_generation {
        return Err(ReadOnlyRefusal::GenerationMismatch {
            found,
            expected: expected_generation,
        });
    }
    Ok(conn)
}

/// Mirrors `hub_inputs::configured_workspace_root` (private to that module),
/// so we replicate the same env-var precedence here rather than reach across
/// a module boundary that wasn't designed to be shared.
pub fn configured_workspace_root() -> PathBuf {
    std::env::var_os("MEMBRANE_REPO_ROOT")
        .or_else(|| std::env::var_os("WORKSPACE_ROOT"))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Resolves the local cortex-engine database path. `MEMBRANE_DB_PATH` overrides
/// for tests/alternate installs; otherwise the workspace-relative default.
pub fn configured_db_path() -> PathBuf {
    std::env::var_os("MEMBRANE_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| configured_workspace_root().join("tools/.cache/memory/cortex-engine.db"))
}

/// Sanctioned read-only open of the configured Cortex database, reporting the
/// refusal. Fails closed on a schema-generation mismatch against
/// `cortex_store::memdb::LATEST_SCHEMA_VERSION`.
pub fn try_open_readonly() -> Result<Connection, ReadOnlyRefusal> {
    open_readonly_sanctioned(
        &configured_db_path(),
        cortex_store::memdb::LATEST_SCHEMA_VERSION,
    )
}

/// Producer-facing form: `None` on any refusal, so callers fail closed
/// instead of fabricating a healthy read.
pub fn open_readonly() -> Option<Connection> {
    try_open_readonly().ok()
}

/// Resolves the local catalog database path (the content-free admission
/// ledger — `receipts` / `retrieval_events`), distinct from the cortex
/// engine DB. `MEMBRANE_CATALOG` overrides; otherwise the same
/// workspace-relative directory as the cortex engine DB, matching
/// `membrane_runtime::catalog::resolve_catalog_path_from`'s `WORKSPACE_ROOT`
/// fallback arm.
pub fn configured_catalog_db_path() -> PathBuf {
    std::env::var_os("MEMBRANE_CATALOG")
        .map(PathBuf::from)
        .unwrap_or_else(|| configured_workspace_root().join("tools/.cache/memory/catalog.db"))
}

/// Sanctioned read-only open of the configured catalog database, reporting
/// the refusal. Fails closed on a schema-generation mismatch against
/// `catalog::CATALOG_SCHEMA_VERSION`.
pub fn try_open_readonly_catalog() -> Result<Connection, ReadOnlyRefusal> {
    open_readonly_sanctioned(
        &configured_catalog_db_path(),
        crate::catalog::CATALOG_SCHEMA_VERSION,
    )
}

/// Producer-facing form: `None` on any refusal.
pub fn open_readonly_catalog() -> Option<Connection> {
    try_open_readonly_catalog().ok()
}

pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_db_at(path: &Path, user_version: i64) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS probe(id TEXT PRIMARY KEY);
             INSERT OR IGNORE INTO probe(id) VALUES('a');
             PRAGMA user_version = {user_version};"
        ))
        .unwrap();
    }

    #[test]
    fn sanctioned_open_refuses_absent_database() {
        let dir = tempfile::tempdir().unwrap();
        let refusal =
            open_readonly_sanctioned(&dir.path().join("missing.db"), 26).expect_err("absent");
        assert_eq!(refusal, ReadOnlyRefusal::Absent);
        assert_eq!(refusal.code(), "database_absent");
    }

    #[test]
    fn sanctioned_open_fails_closed_on_generation_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("older.db");
        write_db_at(&path, 25);
        let refusal = open_readonly_sanctioned(&path, 26).expect_err("older generation refused");
        assert_eq!(
            refusal,
            ReadOnlyRefusal::GenerationMismatch {
                found: 25,
                expected: 26
            }
        );
        assert_eq!(refusal.code(), "schema_generation_mismatch");

        // A *newer* generation is equally unknown to this binary.
        let newer = dir.path().join("newer.db");
        write_db_at(&newer, 99);
        assert!(matches!(
            open_readonly_sanctioned(&newer, 26),
            Err(ReadOnlyRefusal::GenerationMismatch {
                found: 99,
                expected: 26
            })
        ));
    }

    #[test]
    fn sanctioned_open_admits_matching_generation_and_forbids_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("matching.db");
        write_db_at(&path, 26);
        let conn = open_readonly_sanctioned(&path, 26).expect("matching generation admitted");
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM probe", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        assert!(conn
            .execute("INSERT INTO probe(id) VALUES('b')", [])
            .is_err());
    }

    /// CTX-001 guard. Scans every non-test line of `membrane-runtime` and
    /// `cortex-store` sources for raw SQLite opens (`Connection::open` /
    /// `Connection::open_with_flags`, excluding `open_in_memory`) and compares
    /// the per-file count to this frozen inventory. A new ad-hoc open against
    /// a Cortex/catalog database therefore fails this test until it is either
    /// routed through `MemDb::open` (writes) or
    /// `open_readonly_sanctioned` (reads), or deliberately added here with a
    /// reason. Counts, not just presence, so a second open in an
    /// already-listed file is still caught.
    #[test]
    fn sanctioned_sqlite_open_sites_are_frozen() {
        // (path relative to the engine crates root, allowed non-test opens, reason)
        const SANCTIONED: &[(&str, usize, &str)] = &[
            (
                "cortex-store/src/memdb.rs",
                26,
                "MemDb::open is the sole write authority; the rest are the \
                 migration-ladder backouts, the event-ledger extraction it owns, \
                 and the frozen RC-2.3 read-only inspection",
            ),
            (
                "cortex-store/src/db.rs",
                1,
                "observable-event append path, its own event file, not the Cortex DB",
            ),
            (
                "membrane-runtime/src/hub_readonly_db.rs",
                1,
                "open_readonly_sanctioned: the one sanctioned read-only accessor",
            ),
            (
                "membrane-runtime/src/catalog.rs",
                2,
                "Catalog::open write authority for the catalog DB plus its \
                 alternate-candidate inventory probe",
            ),
            (
                "membrane-runtime/src/ledger/db.rs",
                1,
                "Ledger owns its own index database, not Cortex durable truth",
            ),
            (
                "membrane-runtime/src/push/recovery.rs",
                1,
                "Push owns push-artifacts.sqlite, not the Cortex DB",
            ),
            (
                "membrane-runtime/src/doctor.rs",
                1,
                "forensic read-only doctor probe: it must be able to open and \
                 report a database whose generation is wrong, so it reports the \
                 mismatch instead of refusing the open",
            ),
            (
                "membrane-runtime/src/cli.rs",
                1,
                "`membrane storage` forensic probe: same forensic exception as \
                 doctor.rs — it reports user_version rather than trusting it",
            ),
        ];

        let crates_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates root")
            .to_path_buf();

        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("read source dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(path);
                }
            }
        }

        let mut files = Vec::new();
        for krate in ["membrane-runtime", "cortex-store"] {
            walk(&crates_root.join(krate).join("src"), &mut files);
        }
        files.sort();

        let mut findings: Vec<String> = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).expect("read source");
            // Everything from the first `#[cfg(test)]` marker onward is test
            // scaffolding, which may open scratch databases freely.
            let production = match text.find("#[cfg(test)]") {
                Some(index) => &text[..index],
                None => &text[..],
            };
            let count = production
                .lines()
                .filter(|line| {
                    let trimmed = line.trim_start();
                    !trimmed.starts_with("//")
                        && trimmed.contains("Connection::open")
                        && !trimmed.contains("open_in_memory")
                })
                .count();
            if count == 0 {
                continue;
            }
            let relative = file
                .strip_prefix(&crates_root)
                .expect("under crates root")
                .to_string_lossy()
                .replace('\\', "/");
            match SANCTIONED.iter().find(|(name, _, _)| *name == relative) {
                Some((_, allowed, _)) if *allowed == count => {}
                Some((_, allowed, reason)) => findings.push(format!(
                    "{relative}: {count} raw SQLite opens, {allowed} sanctioned ({reason})"
                )),
                None => findings.push(format!(
                    "{relative}: {count} raw SQLite open(s) outside the sanctioned inventory; \
                     route writes through MemDb::open and reads through \
                     hub_readonly_db::open_readonly_sanctioned"
                )),
            }
        }
        assert!(
            findings.is_empty(),
            "unsanctioned SQLite open sites:\n{}",
            findings.join("\n")
        );
    }
}
