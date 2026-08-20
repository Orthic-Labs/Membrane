//! MBR: producer for the Hub's sentinel section.
//!
//! `memory_sentinel_view::project` (src/memory_sentinel_view.rs) is a real,
//! content-free projector, but nothing previously assembled the `report`
//! JSON it expects. This module reads only counts and IDs from the local
//! database — never memory content — and builds that report:
//!
//! - `lifecycle.active` / `superseded` come from `memories.lifecycle_state`.
//! - `lifecycle.demoted` counts `memory_quarantine` rows: quarantine is the
//!   real mechanism that removes a memory from the active recall pool
//!   short of expiry/supersession, so it is the truthful analogue of
//!   "demoted" in this schema (there is no literal `demoted` state).
//! - `lifecycle.expired` mirrors the same time-bound expiry expression
//!   `store.rs::metrics_json` already uses (`effective_until_ms` /
//!   `expires_at_ms` at or before "now").
//! - `proposals` has no durable source in Cortex; it remains empty until a
//!   governed proposal store is established outside durable memory.
//! - `contradictions` are distinct `context_feedback.candidate_id` values
//!   with a verified `outcome = 'contradicted'` — IDs only.
//! - `scratchpad`, `working`, and `taskCriteria` have no durable, queryable
//!   store today (the scratchpad is in-process, session-scoped, and not
//!   reachable from this DB-backed producer without a session id); they are
//!   left absent from the payload so the projector's honest "no evidence"
//!   defaults apply rather than a fabricated zero.
//!
//! Every field stays an ID or a count — raw memory `content` is never
//! selected, read, or forwarded. This is an absolute privacy boundary.

use serde_json::{json, Value};

use crate::hub_readonly_db::{now_unix_ms, open_readonly};

const MAX_ITEMS: usize = 64;

pub fn build_sentinel_report() -> Option<Value> {
    let conn = open_readonly()?;
    build_sentinel_report_from(&conn)
}

pub(crate) fn build_sentinel_report_from(conn: &rusqlite::Connection) -> Option<Value> {
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
        .ok()?;

    let state_count = |state: &str| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE lifecycle_state = ?1",
            [state],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };
    let active = state_count("active");
    let superseded = state_count("superseded");

    let now_ms = now_unix_ms() as i64;
    let expired: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memories WHERE
             (effective_until_ms IS NOT NULL AND effective_until_ms <= ?1)
             OR (expires_at_ms IS NOT NULL AND expires_at_ms <= ?1)",
            [now_ms],
            |r| r.get(0),
        )
        .ok()?;

    let demoted: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_quarantine", [], |r| r.get(0))
        .ok()?;

    let mut contradiction_stmt = conn
        .prepare(
            "SELECT DISTINCT candidate_id FROM context_feedback
             WHERE outcome = 'contradicted' AND verified = 1
             ORDER BY ts DESC LIMIT ?1",
        )
        .ok()?;
    let contradiction_ids: Vec<String> = contradiction_stmt
        .query_map([MAX_ITEMS as i64], |r| r.get(0))
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;

    Some(json!({
        "lifecycle": {
            "active": active,
            "demoted": demoted,
            "superseded": superseded,
            "expired": expired,
        },
        "proposals": [],
        "contradictions": contradiction_ids,
        // scratchpad/working/taskCriteria intentionally omitted: no durable
        // store backs them yet (see module doc comment).
        "evidence": {
            "state": "observed",
            "valid": true,
            "reason": format!("sqlite read of cortex-engine.db succeeded ({total} memory rows scanned)"),
        },
        // No authoritative gate evaluation is wired for the sentinel view
        // specifically; report that plainly instead of fabricating a pass.
        "gate": {
            "state": "not_evaluated",
            "reason": "no authoritative gate evaluation wired for the sentinel producer",
            "authoritative": false,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn seed(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE memories (
                id TEXT PRIMARY KEY, lifecycle_state TEXT NOT NULL DEFAULT 'active',
                effective_until_ms INTEGER, expires_at_ms INTEGER
            );
            CREATE TABLE memory_quarantine (id TEXT PRIMARY KEY);
            CREATE TABLE context_feedback (
                trace_id TEXT NOT NULL, candidate_id TEXT NOT NULL, outcome TEXT NOT NULL,
                verified INTEGER NOT NULL, ts TEXT NOT NULL,
                PRIMARY KEY (trace_id, candidate_id)
            );",
        )
        .unwrap();
    }

    #[test]
    fn missing_tables_fail_closed() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(build_sentinel_report_from(&conn).is_none());
    }

    #[test]
    fn empty_database_reports_zero_counts_not_unavailable() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        let report = build_sentinel_report_from(&conn).expect("report present");
        assert_eq!(report["lifecycle"]["active"], 0);
        assert_eq!(report["proposals"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn healthy_state_counts_and_ids_are_reported() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        conn.execute_batch(
            "INSERT INTO memories (id, lifecycle_state) VALUES ('m1','active'),('m2','active'),('m3','superseded');
             INSERT INTO memory_quarantine (id) VALUES ('m4');
             INSERT INTO context_feedback (trace_id, candidate_id, outcome, verified, ts) VALUES ('t1','m1','contradicted',1,'2026-08-01T00:00:00Z');",
        )
        .unwrap();
        let report = build_sentinel_report_from(&conn).expect("report present");
        assert_eq!(report["lifecycle"]["active"], 2);
        assert_eq!(report["lifecycle"]["superseded"], 1);
        assert_eq!(report["lifecycle"]["demoted"], 1);
        assert!(report["proposals"].as_array().unwrap().is_empty());
        assert_eq!(report["contradictions"][0], "m1");
        assert_eq!(report["evidence"]["valid"], true);
    }

    #[test]
    fn expired_memories_are_counted() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        conn.execute(
            "INSERT INTO memories (id, lifecycle_state, expires_at_ms) VALUES ('m1','active',1)",
            [],
        )
        .unwrap();
        let report = build_sentinel_report_from(&conn).expect("report present");
        assert_eq!(report["lifecycle"]["expired"], 1);
    }

    #[test]
    fn unverified_contradiction_is_not_counted() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        conn.execute_batch(
            "INSERT INTO memories (id) VALUES ('m1');
             INSERT INTO context_feedback (trace_id, candidate_id, outcome, verified, ts)
             VALUES ('t1','m1','contradicted',0,'2026-08-01T00:00:00Z');",
        )
        .unwrap();
        let report = build_sentinel_report_from(&conn).expect("report present");
        assert_eq!(report["contradictions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn no_raw_memory_content_column_is_ever_selected() {
        // Structural guard: the seeded fixture table has no `content` column
        // at all, so this producer cannot select memory text even by accident.
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        let err = conn
            .execute("SELECT content FROM memories", [])
            .unwrap_err();
        assert!(err.to_string().contains("no such column"));
    }
}
