//! Forward-only schema for absorbed session records.

use rusqlite::Connection;

pub const ABSORBED_SCHEMA_VERSION: i64 = 1;

/// Install the absorbed-record tables on an existing Cortex connection.
///
/// The operation is idempotent and contains no data movement.  A separate
/// version table keeps this migration independent of the legacy memory schema.
pub fn ensure_absorbed_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS cortex_absorbed_schema (
             version INTEGER NOT NULL
         );
         INSERT INTO cortex_absorbed_schema(version)
             SELECT 0 WHERE NOT EXISTS (SELECT 1 FROM cortex_absorbed_schema);
         CREATE TABLE IF NOT EXISTS absorbed_sessions (
             session_id TEXT PRIMARY KEY,
             scope_id TEXT NOT NULL,
             workspace_root TEXT,
             permission_mode TEXT,
             model TEXT,
             provider TEXT,
             status TEXT NOT NULL,
             title TEXT,
             tags_json TEXT NOT NULL DEFAULT '[]',
             authority TEXT NOT NULL,
             influence_class TEXT NOT NULL,
             lifecycle TEXT NOT NULL,
             retention TEXT NOT NULL,
             provenance_json TEXT NOT NULL DEFAULT '[]',
             content_hash TEXT NOT NULL,
             created_at_ms INTEGER NOT NULL,
             updated_at_ms INTEGER NOT NULL,
             started_at_ms INTEGER,
             ended_at_ms INTEGER,
             payload_json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_absorbed_sessions_scope
             ON absorbed_sessions(scope_id, updated_at_ms, session_id);
         CREATE TABLE IF NOT EXISTS absorbed_events (
             session_id TEXT NOT NULL,
             seq INTEGER NOT NULL,
             event_id TEXT NOT NULL UNIQUE,
             event_type TEXT NOT NULL,
             payload_json TEXT NOT NULL,
             scope_id TEXT NOT NULL,
             authority TEXT NOT NULL,
             influence_class TEXT NOT NULL,
             lifecycle TEXT NOT NULL,
             retention TEXT NOT NULL,
             provenance_json TEXT NOT NULL DEFAULT '[]',
             content_hash TEXT NOT NULL,
             occurred_at_ms INTEGER NOT NULL,
             recorded_at_ms INTEGER NOT NULL,
             tombstoned INTEGER NOT NULL DEFAULT 0 CHECK (tombstoned IN (0,1)),
             PRIMARY KEY(session_id, seq)
         );
         CREATE INDEX IF NOT EXISTS idx_absorbed_events_session_time
             ON absorbed_events(session_id, occurred_at_ms, seq);
         CREATE TABLE IF NOT EXISTS absorbed_event_cursors (
             session_id TEXT PRIMARY KEY,
             last_seq INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS absorbed_event_tombstones (
             session_id TEXT NOT NULL,
             seq INTEGER NOT NULL,
             event_id TEXT NOT NULL,
             tombstoned_at_ms INTEGER NOT NULL,
             PRIMARY KEY(session_id, seq)
         );
         CREATE TABLE IF NOT EXISTS absorbed_tasks (
             task_id TEXT PRIMARY KEY,
             session_id TEXT NOT NULL,
             scope_id TEXT NOT NULL,
             status TEXT NOT NULL,
             title TEXT NOT NULL,
             goal TEXT,
             authority TEXT NOT NULL,
             influence_class TEXT NOT NULL,
             lifecycle TEXT NOT NULL,
             retention TEXT NOT NULL,
             provenance_json TEXT NOT NULL DEFAULT '[]',
             content_hash TEXT NOT NULL,
             created_at_ms INTEGER NOT NULL,
             updated_at_ms INTEGER NOT NULL,
             completed_at_ms INTEGER,
             payload_json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_absorbed_tasks_session
             ON absorbed_tasks(session_id, updated_at_ms, task_id);
         CREATE TABLE IF NOT EXISTS absorbed_artifacts (
             artifact_id TEXT PRIMARY KEY,
             session_id TEXT,
             task_id TEXT,
             handle TEXT NOT NULL,
             content_hash TEXT NOT NULL,
             media_type TEXT NOT NULL,
             byte_length INTEGER NOT NULL,
             scope_id TEXT NOT NULL,
             authority TEXT NOT NULL,
             influence_class TEXT NOT NULL,
             lifecycle TEXT NOT NULL,
             retention TEXT NOT NULL,
             provenance_json TEXT NOT NULL DEFAULT '[]',
             created_at_ms INTEGER NOT NULL,
             updated_at_ms INTEGER NOT NULL,
             payload_json TEXT NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_absorbed_artifacts_handle
             ON absorbed_artifacts(handle);
         CREATE INDEX IF NOT EXISTS idx_absorbed_artifacts_lineage
             ON absorbed_artifacts(session_id, task_id, updated_at_ms);
         UPDATE cortex_absorbed_schema SET version = 1 WHERE version < 1;
         COMMIT;",
    )
}
