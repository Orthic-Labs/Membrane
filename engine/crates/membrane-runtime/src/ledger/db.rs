//! Ledger's rebuildable, machine-local SQLite store.
//!
//! Ledger is deliberately not backed by Cortex's durable-memory database.  Its
//! rows are projections of the current worktree and may be discarded and
//! rebuilt at any time.

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

const LEDGER_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS ledger_doc_artifacts (
    doc_id TEXT NOT NULL,
    repository_root TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    revision TEXT NOT NULL,
    path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    parser_version TEXT NOT NULL,
    document_class TEXT NOT NULL,
    lifecycle_state TEXT NOT NULL DEFAULT 'active',
    title TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    keywords_json TEXT NOT NULL DEFAULT '[]',
    superseded_by TEXT,
    trust_label TEXT NOT NULL,
    influence_class TEXT NOT NULL,
    sensitivity TEXT NOT NULL,
    generated INTEGER NOT NULL DEFAULT 0,
    index_generation INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    UNIQUE(repository_root, path)
);
CREATE INDEX IF NOT EXISTS idx_ledger_doc_artifacts_root_state
  ON ledger_doc_artifacts(repository_root, lifecycle_state, index_generation);
CREATE TABLE IF NOT EXISTS ledger_doc_projections (
    parent_doc_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    content TEXT NOT NULL,
    token_count INTEGER NOT NULL,
    anchor_id TEXT NOT NULL,
    collapsed_to_parent TEXT,
    source_content_hash TEXT NOT NULL,
    source_revision TEXT NOT NULL,
    index_generation INTEGER NOT NULL,
    PRIMARY KEY(parent_doc_id, kind, anchor_id)
);
CREATE INDEX IF NOT EXISTS idx_ledger_doc_projections_parent_generation
  ON ledger_doc_projections(parent_doc_id, index_generation);
CREATE TABLE IF NOT EXISTS ledger_nodes (
    doc_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    anchor_id TEXT NOT NULL,
    parent_id TEXT,
    ordinal INTEGER NOT NULL,
    node_kind TEXT NOT NULL,
    heading_path TEXT NOT NULL,
    heading TEXT NOT NULL,
    source_start_byte INTEGER NOT NULL,
    source_end_byte INTEGER NOT NULL,
    span_hash TEXT NOT NULL,
    searchable_text TEXT NOT NULL,
    parser_version TEXT NOT NULL,
    projection_schema_version TEXT NOT NULL,
    fts_schema_version TEXT NOT NULL,
    tokenizer_id TEXT NOT NULL,
    query_normalizer_version TEXT NOT NULL,
    source_revision TEXT NOT NULL,
    ledger_generation INTEGER NOT NULL,
    PRIMARY KEY(doc_id, node_id)
);
CREATE INDEX IF NOT EXISTS idx_ledger_nodes_generation
  ON ledger_nodes(doc_id, ledger_generation);
CREATE VIRTUAL TABLE IF NOT EXISTS ledger_node_fts USING fts5(
    doc_id UNINDEXED,
    node_id UNINDEXED,
    path,
    title,
    heading,
    body,
    identifier_aliases,
    tokenize='unicode61 remove_diacritics 2'
);
CREATE TABLE IF NOT EXISTS ledger_activation (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    mode TEXT NOT NULL CHECK(mode IN ('legacy_scan','shadow','ledger_fts')),
    qualification_receipt_sha256 TEXT,
    corpus_version TEXT,
    activated_at_ms INTEGER NOT NULL
);
INSERT OR IGNORE INTO ledger_activation(singleton, mode, activated_at_ms)
VALUES (1, 'legacy_scan', 0);
"#;

/// File name of the pre-rename Guide index, kept only so an existing install
/// can be detected and retired explicitly during the Guide -> Ledger cutover.
const LEGACY_GUIDE_INDEX_FILE_NAME: &str = "guide-index.sqlite3";

/// File name of the canonical Ledger index.
const LEDGER_INDEX_FILE_NAME: &str = "ledger-index.sqlite3";

/// Connection wrapper for Ledger's disposable document index.
pub struct LedgerDb {
    connection: Mutex<Connection>,
    path: Option<PathBuf>,
}

impl LedgerDb {
    /// Open the canonical cache-backed Ledger index.
    ///
    /// Before opening, retires any pre-rename `guide-index.sqlite3` left behind by an
    /// install predating the Guide -> Ledger cutover (see
    /// `docs/subsystems/LEDGER-MARKDOWN-INDEXING-AND-DOCUMENT-NAVIGATION-CANON.md` section 2.4
    /// and the "Migration" runtime contract). Ledger state is a rebuildable projection of
    /// the registered document sources (Locked invariant 1), and the renamed
    /// `guide_doc_*` -> `ledger_doc_*` tables mean the legacy file cannot be reused
    /// in place, so the legacy file is moved aside (never deleted) and a fresh Ledger
    /// generation is built on first sync.
    pub fn open_default() -> Result<Self, String> {
        let cache_root = crate::cache_root();
        retire_legacy_guide_index(&cache_root)?;
        Self::open(cache_root.join(LEDGER_INDEX_FILE_NAME))
    }

    /// Open a Ledger index at an explicit path. Parent directories are created
    /// for a real file, while `:memory:` remains useful for focused tests.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if path != Path::new(":memory:") {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
        }
        let connection = Connection::open(path).map_err(|error| error.to_string())?;
        connection
            .execute_batch(LEDGER_SCHEMA)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            connection: Mutex::new(connection),
            path: Some(path.to_path_buf()),
        })
    }

    /// Open an isolated in-memory Ledger index for tests.
    pub fn open_in_memory() -> Self {
        let connection = Connection::open_in_memory().expect("Ledger in-memory SQLite opens");
        connection
            .execute_batch(LEDGER_SCHEMA)
            .expect("Ledger schema initializes");
        Self {
            connection: Mutex::new(connection),
            path: None,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

/// Detect a pre-rename Guide index at `cache_root` and move it aside so it can never be
/// silently ignored (it would otherwise sit next to the new Ledger file forever, unindexed
/// and unexplained) and never be silently reused (its `guide_doc_*` tables do not match the
/// `ledger_doc_*` schema this store creates). The retired copy is preserved on disk — this is
/// a rebuild migration, not deletion — under a name that will never collide with a live index.
///
/// A no-op when there is no legacy file, or when a Ledger index already exists at the target
/// path (an already-migrated or already-fresh install).
fn retire_legacy_guide_index(cache_root: &Path) -> Result<(), String> {
    let legacy_path = cache_root.join(LEGACY_GUIDE_INDEX_FILE_NAME);
    let ledger_path = cache_root.join(LEDGER_INDEX_FILE_NAME);
    if ledger_path.exists() || !legacy_path.exists() {
        return Ok(());
    }
    let retired_path = unique_retired_path(cache_root);
    std::fs::rename(&legacy_path, &retired_path).map_err(|error| {
        format!(
            "failed to retire legacy Guide index at {}: {error}",
            legacy_path.display()
        )
    })
}

/// Picks a retirement path that cannot collide with a prior retirement left on disk.
fn unique_retired_path(cache_root: &Path) -> PathBuf {
    let base = cache_root.join(format!("{LEGACY_GUIDE_INDEX_FILE_NAME}.pre-ledger-rename"));
    if !base.exists() {
        return base;
    }
    for suffix in 1u32.. {
        let candidate = cache_root.join(format!(
            "{LEGACY_GUIDE_INDEX_FILE_NAME}.pre-ledger-rename.{suffix}"
        ));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("cache_root cannot hold u32::MAX retired legacy indexes")
}

#[cfg(test)]
mod tests {
    use super::{retire_legacy_guide_index, LedgerDb, LEGACY_GUIDE_INDEX_FILE_NAME};

    #[test]
    fn ledger_schema_isolated_from_cortex_table_names() {
        let db = LedgerDb::open_in_memory();
        let conn = db.lock();
        let names = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(names.iter().any(|name| name == "ledger_doc_artifacts"));
        assert!(names.iter().any(|name| name == "ledger_doc_projections"));
        assert!(names.iter().any(|name| name == "ledger_nodes"));
        assert!(names.iter().any(|name| name == "ledger_node_fts"));
        assert!(names.iter().any(|name| name == "ledger_activation"));
        assert!(!names.iter().any(|name| name == "memories"));
        assert!(!names.iter().any(|name| name == "doc_artifacts"));
    }

    /// Proves a pre-rename install (an on-disk `guide-index.sqlite3` with the old
    /// `guide_doc_*` tables) is handled explicitly on the next open: it is neither
    /// left in place unexplained (silently ignored) nor opened as though it already
    /// matched the new `ledger_doc_*` schema (silently corrupted/misread). Instead it
    /// is retired to a clearly-named sibling file and a fresh Ledger generation is
    /// built from source.
    #[test]
    fn legacy_guide_index_is_retired_not_ignored_or_corrupted() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_path = dir.path().join(LEGACY_GUIDE_INDEX_FILE_NAME);

        // Simulate a pre-rename install: old file name, old table names, with a row in it.
        {
            let conn = rusqlite::Connection::open(&legacy_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE guide_doc_artifacts (doc_id TEXT NOT NULL, path TEXT NOT NULL);
                 INSERT INTO guide_doc_artifacts (doc_id, path) VALUES ('doc-1', 'README.md');",
            )
            .unwrap();
        }
        assert!(legacy_path.exists());

        retire_legacy_guide_index(dir.path()).expect("legacy index retires cleanly");

        // The legacy file is gone from its old name (not silently ignored in place)...
        assert!(!legacy_path.exists());
        // ...but preserved, not corrupted or destroyed: it is still openable and its
        // pre-rename row is intact under the retirement name.
        let retired_path = dir
            .path()
            .join(format!("{LEGACY_GUIDE_INDEX_FILE_NAME}.pre-ledger-rename"));
        assert!(retired_path.exists());
        let retired_conn = rusqlite::Connection::open(&retired_path).unwrap();
        let path: String = retired_conn
            .query_row(
                "SELECT path FROM guide_doc_artifacts WHERE doc_id='doc-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(path, "README.md");

        // Opening the real Ledger store at the same cache root afterward builds a
        // fresh, correctly-named generation rather than reusing/misreading the legacy file.
        let ledger_path = dir.path().join("ledger-index.sqlite3");
        let ledger = LedgerDb::open(&ledger_path).unwrap();
        let conn = ledger.lock();
        let names = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(names.iter().any(|name| name == "ledger_doc_artifacts"));
        assert!(names.iter().any(|name| name == "ledger_doc_projections"));
        assert!(names.iter().any(|name| name == "ledger_nodes"));
        assert!(names.iter().any(|name| name == "ledger_node_fts"));
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM ledger_doc_artifacts", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "fresh Ledger generation starts empty, not carrying stale rows");
    }

    /// A second retirement (e.g. a second machine boot against the same cache root after
    /// a partial migration) must not clobber an already-retired copy or panic.
    #[test]
    fn retiring_twice_does_not_clobber_the_first_retired_copy() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_path = dir.path().join(LEGACY_GUIDE_INDEX_FILE_NAME);
        std::fs::write(&legacy_path, b"first").unwrap();
        retire_legacy_guide_index(dir.path()).unwrap();
        assert!(!legacy_path.exists());

        // A later run drops another legacy-named file at the same path (e.g. restored from
        // a stale backup) — retiring again must not silently overwrite the first retirement.
        std::fs::write(&legacy_path, b"second").unwrap();
        retire_legacy_guide_index(dir.path()).unwrap();
        assert!(!legacy_path.exists());

        let first = dir
            .path()
            .join(format!("{LEGACY_GUIDE_INDEX_FILE_NAME}.pre-ledger-rename"));
        let second = dir
            .path()
            .join(format!("{LEGACY_GUIDE_INDEX_FILE_NAME}.pre-ledger-rename.1"));
        assert_eq!(std::fs::read(&first).unwrap(), b"first");
        assert_eq!(std::fs::read(&second).unwrap(), b"second");
    }

    /// When a Ledger index already exists at the target path, a leftover legacy file must
    /// not be touched — retirement runs only on the pre-rename -> post-rename transition.
    #[test]
    fn legacy_file_is_left_alone_once_ledger_index_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_path = dir.path().join(LEGACY_GUIDE_INDEX_FILE_NAME);
        let ledger_path = dir.path().join("ledger-index.sqlite3");
        std::fs::write(&legacy_path, b"legacy").unwrap();
        std::fs::write(&ledger_path, b"already migrated").unwrap();

        retire_legacy_guide_index(dir.path()).unwrap();

        assert!(legacy_path.exists(), "already-migrated installs do not need retirement");
        assert_eq!(std::fs::read(&legacy_path).unwrap(), b"legacy");
    }
}
