//! Guide's rebuildable, machine-local SQLite store.
//!
//! Guide is deliberately not backed by Cortex's durable-memory database.  Its
//! rows are projections of the current worktree and may be discarded and
//! rebuilt at any time.

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

const GUIDE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS guide_doc_artifacts (
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
CREATE INDEX IF NOT EXISTS idx_guide_doc_artifacts_root_state
  ON guide_doc_artifacts(repository_root, lifecycle_state, index_generation);
CREATE TABLE IF NOT EXISTS guide_doc_projections (
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
CREATE INDEX IF NOT EXISTS idx_guide_doc_projections_parent_generation
  ON guide_doc_projections(parent_doc_id, index_generation);
"#;

/// Connection wrapper for Guide's disposable document index.
pub struct GuideDb {
    connection: Mutex<Connection>,
    path: Option<PathBuf>,
}

impl GuideDb {
    /// Open the canonical cache-backed Guide index.
    pub fn open_default() -> Result<Self, String> {
        Self::open(crate::cache_root().join("guide-index.sqlite3"))
    }

    /// Open a Guide index at an explicit path. Parent directories are created
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
            .execute_batch(GUIDE_SCHEMA)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            connection: Mutex::new(connection),
            path: Some(path.to_path_buf()),
        })
    }

    /// Open an isolated in-memory Guide index for tests.
    pub fn open_in_memory() -> Self {
        let connection = Connection::open_in_memory().expect("Guide in-memory SQLite opens");
        connection
            .execute_batch(GUIDE_SCHEMA)
            .expect("Guide schema initializes");
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

#[cfg(test)]
mod tests {
    use super::GuideDb;

    #[test]
    fn guide_schema_isolated_from_cortex_table_names() {
        let db = GuideDb::open_in_memory();
        let conn = db.lock();
        let names = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(names, ["guide_doc_artifacts", "guide_doc_projections"]);
        assert!(!names.iter().any(|name| name == "memories"));
        assert!(!names.iter().any(|name| name == "doc_artifacts"));
    }
}
