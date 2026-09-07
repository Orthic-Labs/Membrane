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
//! Every refusal fails closed at the producer boundary (no row, no fabricated
//! read) but it does **not** collapse to an untyped `None`: producers
//! propagate [`ReadOnlyRefusal::hub_read_reason`] so a schema-generation
//! mismatch reaches the Hub receipt as `schema_generation_mismatch` rather
//! than being reported as a missing file. Recording degradation distinctly
//! from absence is a locked receipt invariant.
//!
//! No other caller may open a Cortex or catalog database ad hoc; the
//! `sanctioned_sqlite_open_sites_are_frozen` test below scans every crate
//! under `engine/crates`, excludes `#[cfg(test)]` items by brace balance,
//! resolves aliased `use rusqlite::Connection as X` imports, and freezes the
//! surviving open sites by file, enclosing function and count.

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

    /// Reason string a Hub producer publishes in `HubReadV1::Unavailable`.
    ///
    /// `Absent` is the one refusal that genuinely means "the input is not
    /// there", so it keeps the established `missing_input` reason. Every
    /// other refusal is a *degradation* of an input that does exist — most
    /// importantly a schema-generation mismatch — and must reach the receipt
    /// under its own code instead of masquerading as a missing file.
    pub fn hub_read_reason(&self) -> &'static str {
        match self {
            Self::Absent => "missing_input",
            other => other.code(),
        }
    }
}

/// Reason published when an accessor succeeded but the underlying tables held
/// nothing to project. Distinct from every [`ReadOnlyRefusal`] code.
pub const REASON_MISSING_INPUT: &str = "missing_input";

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

/// Producer-facing form: fails closed with the *typed* reason, so callers
/// publish `schema_generation_mismatch` (etc.) rather than `missing_input`.
pub fn open_readonly() -> Result<Connection, &'static str> {
    try_open_readonly().map_err(|refusal| refusal.hub_read_reason())
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

/// Producer-facing form: fails closed with the typed reason.
pub fn open_readonly_catalog() -> Result<Connection, &'static str> {
    try_open_readonly_catalog().map_err(|refusal| refusal.hub_read_reason())
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

    /// Blank out every `#[cfg(test)]` item, keeping newlines so line
    /// numbers stay meaningful. Balances braces so a test module in the
    /// middle of a file does not swallow the production code after it.
    fn strip_cfg_test(text: &str) -> String {
        const MARK: &str = "#[cfg(test)]";
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut cursor = 0usize;
        while let Some(offset) = text[cursor..].find(MARK) {
            let start = cursor + offset;
            out.push_str(&text[cursor..start]);
            let mut index = start + MARK.len();
            let mut depth = 0usize;
            let mut opened = false;
            while index < bytes.len() {
                match bytes[index] {
                    b';' if !opened => {
                        index += 1;
                        break;
                    }
                    b'{' => {
                        depth += 1;
                        opened = true;
                    }
                    b'}' => {
                        depth -= 1;
                        if depth == 0 && opened {
                            index += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                index += 1;
            }
            for _ in text[start..index].lines().skip(1) {
                out.push('\n');
            }
            cursor = index;
        }
        out.push_str(&text[cursor..]);
        out
    }

    /// `Connection` plus every alias introduced by
    /// `use rusqlite::{Connection as Db}` / `use rusqlite::Connection as Db`.
    fn connection_names(text: &str) -> Vec<String> {
        let mut names = vec!["Connection".to_string()];
        let mut cursor = 0usize;
        while let Some(offset) = text[cursor..].find("Connection as ") {
            let start = cursor + offset + "Connection as ".len();
            let alias: String = text[start..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !alias.is_empty() && !names.contains(&alias) {
                names.push(alias);
            }
            cursor = start;
        }
        names
    }

    /// Name of the `fn` a line sits in, or `<module>` for item scope.
    fn enclosing_fn(line: &str, current: &mut String) {
        let mut rest = line.trim_start();
        loop {
            let stripped = ["pub(crate) ", "pub(super) ", "pub ", "async ", "unsafe ", "const ", "extern \"C\" "]
                .iter()
                .find_map(|prefix| rest.strip_prefix(*prefix));
            match stripped {
                Some(next) => rest = next.trim_start(),
                None => break,
            }
        }
        if let Some(tail) = rest.strip_prefix("fn ") {
            let name: String = tail
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                *current = name;
            }
        }
    }


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

    /// CTX-001 guard: freeze every raw SQLite open in the workspace.
    ///
    /// Scans **all** crates under `engine/crates` (not just two), because any
    /// crate can reach a Cortex/catalog database. For each `src` file it
    ///
    /// * removes `#[cfg(test)]` items by brace balance (an item ending in `;`,
    ///   such as a `#[cfg(test)] use ...;`, ends at that `;`) — a naive
    ///   truncation at the *first* `#[cfg(test)]` used to hide 99% of the
    ///   largest files, so a planted open below it was invisible;
    /// * resolves `use rusqlite::Connection as Alias` so `Alias::open(p)` is
    ///   caught as well as the literal `Connection::open`;
    /// * attributes each site to its enclosing `fn` and compares the whole
    ///   (file, function, count) inventory to the frozen list below.
    ///
    /// A new open anywhere — new file, new function, or a second open in an
    /// already-listed function — fails this test until it is routed through
    /// `MemDb::open` (writes) or `open_readonly_sanctioned` (reads), or
    /// deliberately added here with a recorded reason. `tests/` and `benches/`
    /// trees are not scanned: they are test scaffolding by construction.
    #[test]
    fn sanctioned_sqlite_open_sites_are_frozen() {
        // (path relative to the engine crates root, enclosing fn, opens, reason)
        const SANCTIONED: &[(&str, &str, usize, &str)] = &[
            (
                "cortex-store/src/db.rs",
                "record_observable_event_at_path",
                1,
                "observable-event append path, its own event file, not the Cortex DB",
            ),
            (
                "cortex-store/src/memdb.rs",
                "open",
                1,
                "MemDb::open is the sole write authority for the Cortex DB",
            ),
            (
                "cortex-store/src/memdb.rs",
                "inspect_smoke_recalls",
                1,
                "frozen RC-2.3 read-only inspection",
            ),
            (
                "cortex-store/src/memdb.rs",
                "extract_event_ledger",
                2,
                "event-ledger extraction the write authority owns",
            ),
            (
                "cortex-store/src/memdb.rs",
                "backout_identity_metadata_to_v14",
                1,
                "migration-ladder backout",
            ),
            ("cortex-store/src/memdb.rs", "backout_v10_to_v9", 4, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v11_to_v10", 3, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v12_to_v11", 2, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v13_to_v12", 2, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v14_to_v13", 1, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v20_if_present", 1, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v20_to_v19", 1, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v21_to_v20", 1, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v22_to_v21", 1, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v23_to_v22", 1, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v24_to_v23", 1, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v25_to_v24", 1, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v26_to_v25", 1, "migration-ladder backout"),
            ("cortex-store/src/memdb.rs", "backout_v27_to_v26", 1, "migration-ladder backout"),
            (
                "membrane-runtime/src/catalog.rs",
                "open",
                1,
                "Catalog::open write authority for the catalog DB",
            ),
            (
                "membrane-runtime/src/catalog.rs",
                "inventory_catalog_alternates",
                1,
                "alternate-candidate inventory probe",
            ),
            (
                "membrane-runtime/src/cli.rs",
                "storage_hygiene_report",
                1,
                "`membrane storage` forensic probe: same forensic exception as \
                 doctor.rs — it reports user_version rather than trusting it",
            ),
            (
                "membrane-runtime/src/doctor.rs",
                "run_with_policy",
                1,
                "forensic read-only doctor probe: it must be able to open and \
                 report a database whose generation is wrong, so it reports the \
                 mismatch instead of refusing the open",
            ),
            (
                "membrane-runtime/src/hub_readonly_db.rs",
                "open_readonly_sanctioned",
                1,
                "the one sanctioned read-only accessor",
            ),
            (
                "membrane-runtime/src/ledger/db.rs",
                "open",
                1,
                "Ledger owns its own index database, not Cortex durable truth",
            ),
            (
                "membrane-runtime/src/mcp_executor.rs",
                "working_context",
                1,
                "RECORDED DEBT (CTX-001): read-write open of the Hub event DB \
                 that CREATEs membrane_working_context. It is not the Cortex \
                 durable DB, but it is a cortex-store-owned file opened outside \
                 that crate's write authority. The old first-`#[cfg(test)]` \
                 truncation hid this site entirely; it is frozen here so it \
                 cannot grow, and belongs behind a cortex-store accessor",
            ),
            (
                "membrane-runtime/src/push/recovery.rs",
                "connection",
                1,
                "Push owns push-artifacts.sqlite, not the Cortex DB",
            ),
            (
                "membrane-transcript/src/source.rs",
                "load_opencode",
                1,
                "foreign host transcript DB (opencode), not a Cortex/catalog \
                 database: the sanctioned accessor asserts the Cortex schema \
                 generation, which a third-party file will never satisfy. \
                 Already strictly SQLITE_OPEN_READ_ONLY",
            ),
            (
                "membrane-transcript/src/source.rs",
                "load_cursor",
                1,
                "foreign host transcript DB (cursor); same reason as load_opencode",
            ),
        ];

        let crates_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates root")
            .to_path_buf();

        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(path);
                }
            }
        }

        let mut files = Vec::new();
        for entry in std::fs::read_dir(&crates_root).expect("read crates root") {
            let krate = entry.expect("crate entry").path();
            if krate.is_dir() {
                // `tests/` and `benches/` are scaffolding by construction.
                walk(&krate.join("src"), &mut files);
            }
        }
        files.sort();
        assert!(
            files.len() > 100,
            "guard scanned only {} files; the crate walk is broken",
            files.len()
        );

        // (file, enclosing fn) -> count
        let mut observed: std::collections::BTreeMap<(String, String), usize> =
            std::collections::BTreeMap::new();
        for file in &files {
            let text = std::fs::read_to_string(file).expect("read source");
            let production = strip_cfg_test(&text);
            let names = connection_names(&text);
            let relative = file
                .strip_prefix(&crates_root)
                .expect("under crates root")
                .to_string_lossy()
                .replace('\\', "/");
            let mut current = "<module>".to_string();
            for line in production.lines() {
                enclosing_fn(line, &mut current);
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") || trimmed.starts_with('*') {
                    continue;
                }
                let opens = names
                    .iter()
                    .filter(|name| {
                        trimmed.contains(&format!("{name}::open("))
                            || trimmed.contains(&format!("{name}::open_with_flags("))
                    })
                    .count();
                if opens > 0 {
                    *observed
                        .entry((relative.clone(), current.clone()))
                        .or_insert(0) += opens;
                }
            }
        }

        let expected: std::collections::BTreeMap<(String, String), usize> = SANCTIONED
            .iter()
            .map(|(file, function, count, _)| ((file.to_string(), function.to_string()), *count))
            .collect();
        assert_eq!(
            expected.len(),
            SANCTIONED.len(),
            "duplicate (file, fn) entry in the frozen inventory"
        );

        let mut findings: Vec<String> = Vec::new();
        for (site, count) in &observed {
            match expected.get(site) {
                Some(allowed) if allowed == count => {}
                Some(allowed) => findings.push(format!(
                    "{}::{}: {count} raw SQLite opens, {allowed} sanctioned",
                    site.0, site.1
                )),
                None => findings.push(format!(
                    "{}::{}: {count} raw SQLite open(s) outside the sanctioned inventory; \
                     route writes through MemDb::open and reads through \
                     hub_readonly_db::open_readonly_sanctioned",
                    site.0, site.1
                )),
            }
        }
        for site in expected.keys() {
            if !observed.contains_key(site) {
                findings.push(format!(
                    "{}::{}: frozen open site has disappeared; remove it from the inventory",
                    site.0, site.1
                ));
            }
        }
        assert!(
            findings.is_empty(),
            "SQLite open inventory drift:\n{}",
            findings.join("\n")
        );
    }

    /// Proves the guard's machinery on synthetic sources: the two blind spots
    /// of the previous first-`#[cfg(test)]`-truncation guard — a production
    /// open *below* a test module, and an aliased `Connection` import — are
    /// now both caught, while opens genuinely inside the test module are not.
    #[test]
    fn guard_sees_planted_open_below_test_module_and_behind_alias() {
        let source = "use rusqlite::Connection as Db;\n\
                      #[cfg(test)]\n\
                      mod tests {\n\
                      \x20   fn scratch() { let _ = Connection::open(\"scratch.db\"); }\n\
                      }\n\
                      pub fn planted() {\n\
                      \x20   let _ = Db::open(\"cortex-engine.db\");\n\
                      }\n";

        // The defect: truncating at the first `#[cfg(test)]` hides everything
        // after it, which in store.rs was 99.3% of the file.
        let naive = &source[..source.find("#[cfg(test)]").unwrap()];
        assert!(!naive.contains("Db::open("));

        let production = strip_cfg_test(source);
        assert!(
            !production.contains("Connection::open(\"scratch.db\")"),
            "the #[cfg(test)] module must be removed"
        );
        assert!(
            production.contains("Db::open(\"cortex-engine.db\")"),
            "production code after the test module must survive"
        );

        let names = connection_names(source);
        assert!(names.contains(&"Db".to_string()), "alias must be resolved");

        // Full scan of the synthetic file, as the guard performs it.
        let mut current = "<module>".to_string();
        let mut sites: Vec<(String, usize)> = Vec::new();
        for line in production.lines() {
            enclosing_fn(line, &mut current);
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with('*') {
                continue;
            }
            let opens = names
                .iter()
                .filter(|name| {
                    trimmed.contains(&format!("{name}::open("))
                        || trimmed.contains(&format!("{name}::open_with_flags("))
                })
                .count();
            if opens > 0 {
                sites.push((current.clone(), opens));
            }
        }
        assert_eq!(
            sites,
            vec![("planted".to_string(), 1)],
            "the planted open must be attributed to `planted`, and the \
             test-module open must not be counted"
        );
    }

    /// Line-number fidelity: stripping test modules must not shift the lines
    /// of the production code that follows.
    #[test]
    fn strip_cfg_test_preserves_line_numbering() {
        let source = "a\n#[cfg(test)]\nmod t {\n    fn f() {}\n}\nb\n";
        let stripped = strip_cfg_test(source);
        assert_eq!(stripped.lines().count(), source.lines().count());
        assert_eq!(stripped.lines().last(), Some("b"));
        // A `#[cfg(test)]` item that ends in `;` (e.g. a test-only `use`).
        let with_use = "#[cfg(test)]\nuse foo::Bar;\nfn keep() {}\n";
        assert!(strip_cfg_test(with_use).contains("fn keep()"));
        assert!(!strip_cfg_test(with_use).contains("foo::Bar"));
    }
}
