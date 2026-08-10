//! MBR: producer for the Hub's adapters section.
//!
//! `agent_adapter_view::project` (src/agent_adapter_view.rs) is a real
//! projector, but nothing previously assembled the `report` JSON it expects.
//! This module reads `memory_identity` (which clients have ever written a
//! memory) and `memory_event_log` (what event kinds each client produced) to
//! build that report. Declared/client capability levels have no real
//! concept in the schema, so those fields stay `"unknown"` — the projector's
//! own `Status::unknown()` fallback then reports capability honestly as
//! unknown rather than fabricating a level comparison. Any DB-access or
//! query failure returns `None`.

use serde_json::{json, Value};

use crate::hub_readonly_db::open_readonly;

const MAX_ADAPTERS: usize = 64;

pub fn build_adapters_report() -> Option<Value> {
    let conn = open_readonly()?;
    build_adapters_report_from(&conn)
}

pub(crate) fn build_adapters_report_from(conn: &rusqlite::Connection) -> Option<Value> {
    let mut stmt = conn
        .prepare(
            "SELECT client, COUNT(*), MAX(created_at)
             FROM memory_identity
             GROUP BY client
             ORDER BY MAX(created_at) DESC
             LIMIT ?1",
        )
        .ok()?;
    let rows: Vec<(String, i64, String)> = stmt
        .query_map(rusqlite::params![MAX_ADAPTERS as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if rows.is_empty() {
        return None;
    }

    let mut delivering_stmt = conn
        .prepare(
            "SELECT COUNT(*) FROM memory_event_log WHERE client = ?1 AND event_kind = 'inject'",
        )
        .ok()?;

    let adapters: Vec<Value> = rows
        .into_iter()
        .map(|(client, identity_count, last_created)| {
            let inject_count: i64 = delivering_stmt
                .query_row([&client], |r| r.get(0))
                .unwrap_or(0);
            let delivering = if inject_count > 0 {
                json!({
                    "state": "available",
                    "mechanism": "memory_event_log inject rows observed",
                    "evidence": format!("{inject_count} inject event(s) recorded"),
                })
            } else {
                json!({
                    "state": "unavailable",
                    "mechanism": "memory_event_log",
                    "evidence": "no inject events recorded for this client",
                })
            };
            json!({
                "id": client,
                "client": client,
                "device": "unknown",
                "declaredCapability": "unknown",
                "clientCapability": "unknown",
                "installed": {
                    "state": "available",
                    "mechanism": "memory_identity rows observed",
                    "evidence": format!("{identity_count} identity row(s), last {last_created}"),
                },
                "active": {
                    "state": "available",
                    "mechanism": "memory_identity presence",
                    "evidence": format!("most recent identity row at {last_created}"),
                },
                "delivering": delivering,
                // No enforcement or cryptographic-proof concept exists for
                // adapters yet; left as the projector's honest "unknown"
                // fallback rather than fabricated.
            })
        })
        .collect();

    Some(json!({ "adapters": adapters }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn seed(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE memory_identity (
                memory_id TEXT PRIMARY KEY, artifact_id TEXT, origin_event_uid TEXT,
                installation_id TEXT, client TEXT NOT NULL, session_id TEXT, turn_id TEXT,
                trace_id TEXT, identity_status TEXT, created_at TEXT NOT NULL
            );
            CREATE TABLE memory_event_log (
                event_id INTEGER PRIMARY KEY, ts TEXT NOT NULL, event_kind TEXT NOT NULL,
                memory_id TEXT, surface TEXT NOT NULL DEFAULT 'unknown', session_id TEXT,
                trace_id TEXT, scope_id TEXT, quantity INTEGER NOT NULL DEFAULT 1,
                meta TEXT NOT NULL DEFAULT '{}', client TEXT
            );",
        )
        .unwrap();
    }

    #[test]
    fn empty_database_fails_closed() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        assert!(build_adapters_report_from(&conn).is_none());
    }

    #[test]
    fn missing_tables_fail_closed() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(build_adapters_report_from(&conn).is_none());
    }

    #[test]
    fn client_with_no_inject_events_is_not_delivering() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        conn.execute(
            "INSERT INTO memory_identity (memory_id, client, created_at) VALUES ('m1','codex','2026-08-01T00:00:00Z')",
            [],
        )
        .unwrap();
        let report = build_adapters_report_from(&conn).expect("report present");
        let adapters = report["adapters"].as_array().unwrap();
        assert_eq!(adapters.len(), 1);
        assert_eq!(adapters[0]["delivering"]["state"], "unavailable");
    }

    #[test]
    fn client_with_inject_events_is_delivering() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        conn.execute(
            "INSERT INTO memory_identity (memory_id, client, created_at) VALUES ('m1','claude_code','2026-08-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO memory_event_log (event_id, ts, event_kind, client) VALUES (1,'2026-08-01T00:00:01Z','inject','claude_code')",
            [],
        )
        .unwrap();
        let report = build_adapters_report_from(&conn).expect("report present");
        let adapters = report["adapters"].as_array().unwrap();
        assert_eq!(adapters[0]["delivering"]["state"], "available");
    }
}
