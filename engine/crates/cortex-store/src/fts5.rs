//! Rebuildable FTS5 projection for Cortex lexical retrieval.
//!
//! The projection deliberately stores only fields needed to qualify and rank
//! a hit.  Canonical records remain in Cortex's durable tables; callers can
//! rebuild this table from those records after loss or corruption.

use rusqlite::{params, Connection, OptionalExtension};

/// Name of the rebuildable virtual table.
pub const FTS5_TABLE: &str = "cortex_fts5";

/// A row returned by the lexical projection.
#[derive(Debug, Clone, PartialEq)]
pub struct Fts5Hit {
    pub record_id: String,
    pub record_type: String,
    pub session_id: Option<String>,
    pub scope_id: String,
    pub lifecycle: String,
    pub authority: String,
    /// Positive relevance after converting SQLite's negative BM25 rank.
    pub score: f64,
}

/// Projection health after ensuring the virtual table exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionState {
    Ready,
    Rebuilt,
}

/// Errors are typed so callers can degrade lexical retrieval without losing
/// semantic records or pretending an unavailable index is authoritative.
#[derive(Debug, thiserror::Error)]
pub enum Fts5Error {
    #[error("fts5 sqlite operation: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("fts5 projection is missing or corrupt: {0}")]
    Degraded(String),
}

/// A connection-local FTS5 projection.
pub struct Fts5Projection<'conn> {
    conn: &'conn Connection,
}

impl<'conn> Fts5Projection<'conn> {
    pub fn new(conn: &'conn Connection) -> Self {
        Self { conn }
    }

    /// Create the projection if absent.  This never deletes canonical data.
    pub fn ensure_schema(&self) -> Result<ProjectionState, Fts5Error> {
        let existed = self
            .conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
                params![FTS5_TABLE],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if let Some(sql) = self
            .conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                params![FTS5_TABLE],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            if !sql.to_ascii_lowercase().contains("using fts5") {
                return Err(Fts5Error::Degraded(
                    "cortex_fts5 exists but is not an FTS5 virtual table".into(),
                ));
            }
        }
        self.conn
            .execute_batch(
                "CREATE VIRTUAL TABLE IF NOT EXISTS cortex_fts5 USING fts5(
                record_id UNINDEXED,
                record_type UNINDEXED,
                session_id UNINDEXED,
                scope_id UNINDEXED,
                lifecycle UNINDEXED,
                authority UNINDEXED,
                content,
                keywords,
                tokenize='unicode61 remove_diacritics 2'
            );",
            )
            .map_err(|error| Fts5Error::Degraded(error.to_string()))?;
        Ok(if existed.is_some() {
            ProjectionState::Ready
        } else {
            ProjectionState::Rebuilt
        })
    }

    /// Rebuild only the projection rows supplied by the canonical store.
    pub fn rebuild<I>(&self, rows: I) -> Result<(), Fts5Error>
    where
        I: IntoIterator<Item = Fts5Document>,
    {
        self.ensure_schema()?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM cortex_fts5", [])?;
        for row in rows {
            insert_document(&tx, &row)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Insert or replace one projection row.  The record itself is never
    /// touched, so a projection failure cannot erase semantic memory.
    pub fn upsert(&self, row: &Fts5Document) -> Result<(), Fts5Error> {
        let tx = self.conn.unchecked_transaction()?;
        Self::upsert_on(&tx, row)?;
        tx.commit()?;
        Ok(())
    }

    /// Same replace-then-insert as `upsert`, but WITHOUT opening a transaction of
    /// its own. Callers that already hold one (the canonical `memories` write sink
    /// keeps the projection in step inside the same transaction, so a committed row
    /// and its lexical projection can never diverge) must use this: SQLite has no
    /// nested transactions, so `upsert` there fails with "cannot start a transaction
    /// within a transaction". Atomicity is the caller's open transaction.
    pub fn upsert_within(&self, row: &Fts5Document) -> Result<(), Fts5Error> {
        Self::upsert_on(self.conn, row)
    }

    fn upsert_on(conn: &Connection, row: &Fts5Document) -> Result<(), Fts5Error> {
        Fts5Projection::new(conn).ensure_schema()?;
        conn.execute(
            "DELETE FROM cortex_fts5 WHERE record_id=?1",
            params![row.record_id],
        )?;
        insert_document(conn, row)?;
        Ok(())
    }

    pub fn delete(&self, record_id: &str) -> Result<(), Fts5Error> {
        self.ensure_schema()?;
        self.conn.execute(
            "DELETE FROM cortex_fts5 WHERE record_id=?1",
            params![record_id],
        )?;
        Ok(())
    }

    /// Search with a sanitized AND query and deterministic keyset-like page.
    /// Qualification is applied in SQL before BM25 ranking.
    pub fn search(
        &self,
        query: &str,
        scope_id: Option<&str>,
        lifecycle: Option<&str>,
        authority: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Fts5Hit>, Fts5Error> {
        self.ensure_schema()?;
        let Some(match_query) = sanitize_match_query(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self
            .conn
            .prepare(
                // bm25() takes ONE weight per column, in declaration order.
                // `cortex_fts5` has eight columns, six of them UNINDEXED
                // (record_id, record_type, session_id, scope_id, lifecycle,
                // authority) and thus contributing no terms; they still consume
                // weight slots. Passing only two weights bound them to
                // record_id/record_type and left `content` and `keywords` both
                // at the default 1.0, so the intended keyword-over-body
                // weighting never applied. The full vector zeroes the UNINDEXED
                // columns and keeps the content:keywords ratio at 1.0:2.0.
                "SELECT record_id, record_type, session_id, scope_id, lifecycle, authority,
                    -bm25(cortex_fts5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0) AS relevance
             FROM cortex_fts5
             WHERE cortex_fts5 MATCH ?1
               AND (?2 IS NULL OR scope_id = ?2)
               AND (?3 IS NULL OR lifecycle = ?3)
               AND (?4 IS NULL OR authority = ?4)
             ORDER BY relevance DESC, record_id ASC
             LIMIT ?5 OFFSET ?6",
            )
            .map_err(|error| Fts5Error::Degraded(error.to_string()))?;
        let rows = stmt
            .query_map(
                params![
                    match_query,
                    scope_id,
                    lifecycle,
                    authority,
                    limit as i64,
                    offset as i64
                ],
                |row| {
                    Ok(Fts5Hit {
                        record_id: row.get(0)?,
                        record_type: row.get(1)?,
                        session_id: row.get(2)?,
                        scope_id: row.get(3)?,
                        lifecycle: row.get(4)?,
                        authority: row.get(5)?,
                        score: row.get(6)?,
                    })
                },
            )
            .map_err(|error| Fts5Error::Degraded(error.to_string()))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| Fts5Error::Degraded(error.to_string()))?;
        Ok(rows)
    }
}

/// Canonical fields projected into FTS5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fts5Document {
    pub record_id: String,
    pub record_type: String,
    pub session_id: Option<String>,
    pub scope_id: String,
    pub lifecycle: String,
    pub authority: String,
    pub content: String,
    pub keywords: String,
}

fn insert_document(conn: &Connection, row: &Fts5Document) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO cortex_fts5
         (record_id, record_type, session_id, scope_id, lifecycle, authority, content, keywords)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            row.record_id,
            row.record_type,
            row.session_id,
            row.scope_id,
            row.lifecycle,
            row.authority,
            row.content,
            row.keywords
        ],
    )?;
    Ok(())
}

/// Turn arbitrary user text into a safe FTS5 MATCH expression.  Operators,
/// column selectors, and unbalanced quotes are never passed through.
pub fn sanitize_match_query(query: &str) -> Option<String> {
    let terms = query
        .split_whitespace()
        .map(|raw| {
            raw.chars()
                .filter(|ch| ch.is_alphanumeric() || *ch == '_' || *ch == '-')
                .collect::<String>()
        })
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

#[cfg(test)]
mod tests {
    use super::sanitize_match_query;

    #[test]
    fn sanitizes_operators_and_empty_queries() {
        assert_eq!(
            sanitize_match_query("rust OR async"),
            Some("\"rust\" AND \"OR\" AND \"async\"".into())
        );
        assert_eq!(sanitize_match_query("***"), None);
    }
}
