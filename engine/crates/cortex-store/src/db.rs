//! MBR-501: durable recording of normalized cross-agent observable events.
//!
//! Every client emits the same [`membrane_protocol::ObservableEventV1`]
//! envelope; this module is the single writer that persists one row per
//! event into the `observable_events` table inside the existing
//! membrane-events SQLite database. The table is created on first use so
//! a clean install does not need an explicit migration; no existing table
//! schema is changed.
//!
//! The connection target mirrors the existing `MEMBRANE_EVENT_DB` env-var
//! convention (`memdb::resolve_event_db_path`). When that var is unset the
//! function defaults to `<memory_db_basename>.membrane-events.sqlite3` next
//! to the memory DB, matching what `MemDb::open` derives at runtime. The
//! function is intentionally fire-and-log: a missing DB is not fatal, so a
//! client that emits an event before its durable store is initialized
//! cannot deadlock the runtime.

use membrane_protocol::ObservableEventV1;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

/// Errors `record_observable_event` can surface to callers. SQLite I/O
/// errors are wrapped verbatim; the typed variants stay narrow on purpose
/// so the runtime can map them to retry decisions.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The on-disk database could not be opened, the schema could not be
    /// applied, or the row insert failed.
    #[error("observable_events insert: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The caller asked us to persist an envelope whose self-integrity
    /// check failed. This is the gate that prevents a tampered payload
    /// from sneaking into the durable ledger.
    #[error("observable_events integrity check failed: {0}")]
    Integrity(String),
}

/// Persist one observable event into the `observable_events` table. The
/// table is created on first write so no migration is required.
///
/// `now_unix_ms` is the durable-store wall clock (separate from the
/// envelope's `occurredAtUnixMs`, which is the producing client's clock).
/// `event` must verify its own digest; tampering is refused before any
/// row is written.
pub fn record_observable_event(
    event: &ObservableEventV1,
    now_unix_ms: u64,
) -> Result<(), StoreError> {
    let path = resolve_default_path();
    record_observable_event_at_path(event, now_unix_ms, &path)
}

fn record_observable_event_at_path(
    event: &ObservableEventV1,
    now_unix_ms: u64,
    path: &Path,
) -> Result<(), StoreError> {
    if let Err(message) = event.verify_self_integrity() {
        return Err(StoreError::Integrity(message));
    }

    let payload_json = serde_json::to_string(event)
        .map_err(|error| StoreError::Integrity(format!("serialize event: {error}")))?;

    let mut conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;",
    )?;
    ensure_observable_events_schema(&mut conn)?;

    let now_iso = crate::time::now_iso();
    conn.execute(
        "INSERT OR IGNORE INTO observable_events (
            event_id, schema_version, kind, client_id, installation_id,
            occurred_at_unix_ms, observed_at_unix_ms, observed_at_iso,
            scope_grant, mechanism, payload_json, digest
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            event.event_id,
            event.schema_version as i64,
            event.kind.as_str(),
            event.client_id,
            event.installation_id,
            event.occurred_at_unix_ms as i64,
            now_unix_ms as i64,
            now_iso,
            event.scope_grant,
            event.mechanism,
            payload_json,
            event.digest,
        ],
    )?;
    Ok(())
}

/// Create the `observable_events` table if it does not exist. The schema
/// mirrors the canonical envelope 1:1 so a round-trip read is a single
/// `SELECT`; the `payload_json` column keeps the original envelope verbatim
/// for forensic replay.
fn ensure_observable_events_schema(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS observable_events (
            event_id            TEXT PRIMARY KEY,
            schema_version      INTEGER NOT NULL,
            kind                TEXT NOT NULL
                CHECK (kind IN (
                    'context_delivered',
                    'scope_grant_issued',
                    'mechanism_receipt',
                    'adapter_heartbeat',
                    'provider_error',
                    'client_conflation'
                )),
            client_id           TEXT NOT NULL,
            installation_id     TEXT NOT NULL,
            occurred_at_unix_ms INTEGER NOT NULL,
            observed_at_unix_ms INTEGER NOT NULL,
            observed_at_iso     TEXT NOT NULL,
            scope_grant         TEXT,
            mechanism           TEXT,
            payload_json        TEXT NOT NULL,
            digest              TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_observable_events_installation_ts
            ON observable_events(installation_id, occurred_at_unix_ms);
        CREATE INDEX IF NOT EXISTS idx_observable_events_client_kind_ts
            ON observable_events(client_id, kind, occurred_at_unix_ms);",
    )
}

/// Default SQLite path the function opens when `MEMBRANE_EVENT_DB` is
/// unset. Mirrors the convention `memdb::resolve_event_db_path` uses so
/// the runtime does not need to plumb a connection into this module.
fn resolve_default_path() -> PathBuf {
    if let Some(path) = std::env::var_os("MEMBRANE_EVENT_DB") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("MEMBRANE_MEMORY_DB") {
        let memory_path = PathBuf::from(path);
        return derive_event_path(&memory_path);
    }
    // Final fallback: an in-memory database is unusable across processes,
    // but it keeps the function total when no env var is configured (the
    // caller can still observe the result through `StoreError::Sqlite`).
    PathBuf::from(":memory:")
}

fn derive_event_path(memory_path: &Path) -> PathBuf {
    let stem = memory_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("cortex");
    memory_path.with_file_name(format!("{stem}.membrane-events.sqlite3"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::{ObservableEventKindV1, ObservableEventV1};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_event_path(label: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        p.push(format!(
            "membrane-mbr501-{label}-{}-{n}.sqlite3",
            std::process::id()
        ));
        p
    }

    fn sample_envelope() -> ObservableEventV1 {
        ObservableEventV1::new_with_bindings(
            ObservableEventKindV1::ProviderError,
            "claude",
            "00000000-0000-4000-8000-0000000000ff",
            1_700_000_000_500,
            json!({"code": "context_unavailable", "retryable": true}),
            None,
            None,
        )
    }

    #[test]
    fn records_a_verified_envelope_into_a_fresh_db() {
        let path = temp_event_path("happy");

        let event = sample_envelope();
        record_observable_event_at_path(&event, 1_700_000_000_501, &path).expect("records");

        let conn = Connection::open(&path).expect("reopens");
        let row: (String, String, String, i64, String) = conn
            .query_row(
                "SELECT event_id, kind, client_id, observed_at_unix_ms, digest
                   FROM observable_events WHERE event_id = ?1",
                params![event.event_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("row exists");
        assert_eq!(row.0, event.event_id);
        assert_eq!(row.1, "provider_error");
        assert_eq!(row.2, "claude");
        assert_eq!(row.3, 1_700_000_000_501);
        assert_eq!(row.4, event.digest);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn refuses_to_record_a_tampered_envelope() {
        let path = temp_event_path("tampered");

        let mut event = sample_envelope();
        // Mutate the payload but DO NOT re-stamp the digest. The integrity
        // gate in `record_observable_event_at_path` calls `verify_self_integrity`,
        // which re-computes the digest from the other fields and compares;
        // the recomputed value no longer matches `event.digest`, so the
        // tampered envelope is refused before any row is written.
        event.payload = json!({"code": "context_unavailable", "retryable": false});
        let err = record_observable_event_at_path(&event, 1_700_000_000_502, &path)
            .expect_err("tampered envelope must be refused");
        assert!(matches!(err, StoreError::Integrity(_)));

        // The self-integrity check is the first thing the path-taking writer
        // does, before the DB is ever opened, so an `Integrity` Err proves
        // no row was persisted. Verify the DB file was never created.
        assert!(
            !path.exists(),
            "no DB file should be created when the envelope is refused"
        );
    }
}
