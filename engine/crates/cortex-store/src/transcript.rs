//! Durable, rebuildable transcript chunk projection.
//!
//! Raw absorbed events remain canonical. This store keeps chunk rows and a local FTS5 index so
//! retrieval can degrade independently without deleting or rewriting events.

use crate::MemDb;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub const TRANSCRIPT_STORE_SCHEMA_VERSION: u32 = 1;
const CHUNK_TABLE: &str = "cortex_transcript_chunks";
const FTS_TABLE: &str = "cortex_transcript_chunks_fts";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranscriptChunkRecord {
    pub schema_version: u32,
    pub chunk_id: String,
    pub session_id: String,
    pub seq_start: u64,
    pub seq_end: u64,
    pub role: String,
    pub speaker: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub authority: String,
    pub scope_id: String,
    pub content_hash: String,
    pub model_provenance: Option<String>,
    pub source_event_ids: Vec<String>,
    pub content: String,
    #[serde(default)]
    pub omissions: Vec<String>,
}

pub type TranscriptChunk = TranscriptChunkRecord;

#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptSearchHit {
    pub chunk: TranscriptChunkRecord,
    pub score: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum TranscriptStoreError {
    #[error("transcript sqlite operation: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("transcript serialization: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("transcript chunk has invalid identity or governance")]
    InvalidChunk,
}

pub struct TranscriptStore {
    db: MemDb,
}

impl TranscriptStore {
    pub fn new(db: MemDb) -> Result<Self, TranscriptStoreError> {
        {
            let conn = db.lock();
            ensure_schema(&conn)?;
        }
        Ok(Self { db })
    }

    pub fn database(&self) -> MemDb {
        self.db.clone()
    }

    pub fn put(&self, chunk: &TranscriptChunkRecord) -> Result<(), TranscriptStoreError> {
        validate(chunk)?;
        let source_ids = serde_json::to_string(&chunk.source_event_ids)?;
        let omissions = serde_json::to_string(&chunk.omissions)?;
        let payload = serde_json::to_string(chunk)?;
        let conn = self.db.lock();
        ensure_schema(&conn)?;
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO cortex_transcript_chunks
             (chunk_id,session_id,seq_start,seq_end,role,speaker,started_at_ms,ended_at_ms,
              authority,scope_id,content_hash,model_provenance,source_event_ids,content,omissions,payload_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                chunk.chunk_id,
                chunk.session_id,
                chunk.seq_start as i64,
                chunk.seq_end as i64,
                chunk.role,
                chunk.speaker,
                chunk.started_at_ms as i64,
                chunk.ended_at_ms as i64,
                chunk.authority,
                chunk.scope_id,
                chunk.content_hash,
                chunk.model_provenance,
                source_ids,
                chunk.content,
                omissions,
                payload,
            ],
        )?;
        tx.execute("DELETE FROM cortex_transcript_chunks_fts WHERE chunk_id=?1", [&chunk.chunk_id])?;
        tx.execute(
            "INSERT INTO cortex_transcript_chunks_fts(chunk_id,session_id,scope_id,content,keywords)
             VALUES (?1,?2,?3,?4,?5)",
            params![chunk.chunk_id, chunk.session_id, chunk.scope_id, chunk.content, chunk.role],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn upsert(&self, chunk: &TranscriptChunkRecord) -> Result<(), TranscriptStoreError> {
        self.put(chunk)
    }

    pub fn get(&self, chunk_id: &str) -> Result<Option<TranscriptChunkRecord>, TranscriptStoreError> {
        let conn = self.db.lock();
        ensure_schema(&conn)?;
        let payload = conn
            .query_row(
                "SELECT payload_json FROM cortex_transcript_chunks WHERE chunk_id=?1",
                [chunk_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|value| serde_json::from_str(&value).map_err(TranscriptStoreError::from))
            .transpose()
    }

    pub fn list_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<TranscriptChunkRecord>, TranscriptStoreError> {
        let conn = self.db.lock();
        ensure_schema(&conn)?;
        let mut statement = conn.prepare(
            "SELECT payload_json FROM cortex_transcript_chunks
             WHERE session_id=?1 ORDER BY seq_start ASC, chunk_id ASC",
        )?;
        let rows = statement.query_map([session_id], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let payload = row?;
            serde_json::from_str(&payload).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(TranscriptStoreError::from)
    }

    pub fn search(
        &self,
        query: &str,
        session_id: Option<&str>,
        scope_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<TranscriptSearchHit>, TranscriptStoreError> {
        let Some(match_query) = sanitize_match_query(query) else {
            return Ok(Vec::new());
        };
        let conn = self.db.lock();
        ensure_schema(&conn)?;
        let mut statement = conn.prepare(
            "SELECT chunks.payload_json, -bm25(cortex_transcript_chunks_fts, 1.0, 2.0)
             FROM cortex_transcript_chunks_fts fts
             JOIN cortex_transcript_chunks chunks ON chunks.chunk_id=fts.chunk_id
             WHERE cortex_transcript_chunks_fts MATCH ?1
               AND (?2 IS NULL OR chunks.session_id=?2)
               AND (?3 IS NULL OR chunks.scope_id=?3)
             ORDER BY 2 DESC, chunks.chunk_id ASC LIMIT ?4 OFFSET ?5",
        )?;
        let rows = statement.query_map(
            params![match_query, session_id, scope_id, limit as i64, offset as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)),
        )?;
        let mut hits = Vec::new();
        for row in rows {
            let (payload, score) = row?;
            hits.push(TranscriptSearchHit {
                chunk: serde_json::from_str(&payload)?,
                score,
            });
        }
        Ok(hits)
    }

    pub fn delete(&self, chunk_id: &str) -> Result<bool, TranscriptStoreError> {
        let conn = self.db.lock();
        ensure_schema(&conn)?;
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM cortex_transcript_chunks_fts WHERE chunk_id=?1", [chunk_id])?;
        let changed = tx.execute("DELETE FROM cortex_transcript_chunks WHERE chunk_id=?1", [chunk_id])?;
        tx.commit()?;
        Ok(changed != 0)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<usize, TranscriptStoreError> {
        let conn = self.db.lock();
        ensure_schema(&conn)?;
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM cortex_transcript_chunks_fts WHERE session_id=?1", [session_id])?;
        let changed = tx.execute("DELETE FROM cortex_transcript_chunks WHERE session_id=?1", [session_id])?;
        tx.commit()?;
        Ok(changed)
    }
}

fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS cortex_transcript_chunks (
             chunk_id TEXT PRIMARY KEY,
             session_id TEXT NOT NULL,
             seq_start INTEGER NOT NULL,
             seq_end INTEGER NOT NULL,
             role TEXT NOT NULL,
             speaker TEXT NOT NULL,
             started_at_ms INTEGER NOT NULL,
             ended_at_ms INTEGER NOT NULL,
             authority TEXT NOT NULL,
             scope_id TEXT NOT NULL,
             content_hash TEXT NOT NULL,
             model_provenance TEXT,
             source_event_ids TEXT NOT NULL,
             content TEXT NOT NULL,
             omissions TEXT NOT NULL,
             payload_json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_cortex_transcript_session_seq
           ON cortex_transcript_chunks(session_id, seq_start, chunk_id);
         CREATE VIRTUAL TABLE IF NOT EXISTS cortex_transcript_chunks_fts USING fts5(
             chunk_id UNINDEXED, session_id UNINDEXED, scope_id UNINDEXED,
             content, keywords,
             tokenize='unicode61 remove_diacritics 2'
         );",
    )
}

fn validate(chunk: &TranscriptChunkRecord) -> Result<(), TranscriptStoreError> {
    if chunk.schema_version != TRANSCRIPT_STORE_SCHEMA_VERSION
        || chunk.chunk_id.trim().is_empty()
        || chunk.session_id.trim().is_empty()
        || chunk.seq_start == 0
        || chunk.seq_end <= chunk.seq_start
        || chunk.role.trim().is_empty()
        || chunk.speaker.trim().is_empty()
        || chunk.authority.trim().is_empty()
        || chunk.scope_id.trim().is_empty()
        || chunk.content_hash.trim().is_empty()
        || chunk.source_event_ids.is_empty()
    {
        return Err(TranscriptStoreError::InvalidChunk);
    }
    Ok(())
}

fn sanitize_match_query(query: &str) -> Option<String> {
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
