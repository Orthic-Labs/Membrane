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
//! - `dimensions` carries the remaining MEM-029 health dimensions that ARE
//!   observable from this database (duplication, relation integrity,
//!   isolation, provenance gaps, lifecycle/supersession anomaly, embedding
//!   projection drift, observed recall, crowding, review backlog). Each
//!   dimension degrades independently to `not_evaluated` with a typed reason
//!   when its source table/column is unavailable — one missing projection
//!   never masks the dimensions that did read. Counts are real COUNTs over
//!   the named source; `ids` carry at most [`MAX_ITEMS`] drill-down IDs
//!   (CTX-030) and never content.
//!
//! Every field stays an ID or a count — raw memory `content` is never
//! selected, read, or forwarded. This is an absolute privacy boundary.

use serde_json::{json, Value};

use crate::hub_readonly_db::{now_unix_ms, open_readonly, REASON_MISSING_INPUT};

const MAX_ITEMS: usize = 64;

/// `Err` carries the reason to publish in `HubReadV1::Unavailable`: a typed
/// read-only refusal code (e.g. `schema_generation_mismatch`) when the
/// database exists but cannot be trusted, `missing_input` only when the input
/// is genuinely absent or empty.
pub fn build_sentinel_report() -> Result<Value, &'static str> {
    let conn = open_readonly()?;
    build_sentinel_report_from(&conn).ok_or(REASON_MISSING_INPUT)
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
        "dimensions": sentinel_dimensions(conn, now_ms),
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

/// Typed result of one dimension probe: `Ok((count, ids))` when the source
/// read succeeded, `Err(reason)` when it did not. `ids` are drill-down
/// identities only — relation ids or memory ids, never content.
type DimensionProbe = Result<(u64, Vec<String>), &'static str>;

/// Run `sql` expecting `(id TEXT, …)` rows; `count` is the number of rows
/// matched (bounded by `MAX_ITEMS` for drill-down) and `ids` their first
/// column. The total count is computed separately so the drill-down list can
/// stay bounded while the count stays exact.
fn probe_ids(conn: &rusqlite::Connection, count_sql: &str, id_sql: &str) -> DimensionProbe {
    let count: i64 = conn
        .query_row(count_sql, [], |r| r.get(0))
        .map_err(|_| "source_unavailable")?;
    let mut stmt = conn.prepare(id_sql).map_err(|_| "source_unavailable")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|_| "source_unavailable")?
        .take(MAX_ITEMS)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "source_unavailable")?;
    Ok((count.max(0) as u64, ids))
}

/// Same shape as [`probe_ids`] for probes whose rows carry no stable id
/// (aggregate-only dimensions such as observed recall or crowding).
fn probe_scalar(conn: &rusqlite::Connection, count_sql: &str) -> DimensionProbe {
    let count: i64 = conn
        .query_row(count_sql, [], |r| r.get(0))
        .map_err(|_| "source_unavailable")?;
    Ok((count.max(0) as u64, Vec::new()))
}

fn dimension(probe: DimensionProbe, observed_reason: &str) -> Value {
    match probe {
        Ok((count, ids)) => json!({
            "state": "observed",
            "count": count,
            "ids": ids,
            "reason": observed_reason,
        }),
        Err(reason) => json!({
            "state": "not_evaluated",
            "reason": reason,
        }),
    }
}

/// MEM-029: the remaining bounded memory-health dimensions that this database
/// can observe. Every dimension is computed independently so one unavailable
/// source (older fixture, missing optional table) reports `not_evaluated`
/// rather than dragging the whole report down or fabricating a zero.
fn sentinel_dimensions(conn: &rusqlite::Connection, now_ms: i64) -> Value {
    // Exact-hash duplication among active memories. Near-duplicate detection
    // would require embedding-space distance — that inference is not
    // performed here, so the reason names the boundary.
    let duplication = dimension(
        probe_ids(
            conn,
            "SELECT COUNT(*) FROM memories m WHERE m.lifecycle_state='active'
               AND m.content_hash IS NOT NULL AND m.content_hash IN (
                 SELECT content_hash FROM memories
                   WHERE lifecycle_state='active' AND content_hash IS NOT NULL
                   GROUP BY content_hash HAVING COUNT(*) > 1)",
            "SELECT m.id FROM memories m WHERE m.lifecycle_state='active'
               AND m.content_hash IS NOT NULL AND m.content_hash IN (
                 SELECT content_hash FROM memories
                   WHERE lifecycle_state='active' AND content_hash IS NOT NULL
                   GROUP BY content_hash HAVING COUNT(*) > 1)
               ORDER BY m.content_hash, m.id",
        ),
        "active memories sharing an exact content_hash; embedding-space near-duplicates are not evaluated",
    );

    // Dangling relation endpoints: memory_relation rows whose source or
    // target has no memories row. Traversal already excludes them; here they
    // stay countable as an integrity signal.
    let relation_integrity = dimension(
        probe_ids(
            conn,
            "SELECT COUNT(*) FROM memory_relation r
               WHERE NOT EXISTS (SELECT 1 FROM memories m WHERE m.id = r.source_id)
                  OR NOT EXISTS (SELECT 1 FROM memories m WHERE m.id = r.target_id)",
            "SELECT r.relation_id FROM memory_relation r
               WHERE NOT EXISTS (SELECT 1 FROM memories m WHERE m.id = r.source_id)
                  OR NOT EXISTS (SELECT 1 FROM memories m WHERE m.id = r.target_id)
               ORDER BY r.relation_id",
        ),
        "memory_relation edges whose source or target memory is absent",
    );

    // Isolated active memories: no incident edge in either direction.
    let isolation = dimension(
        probe_ids(
            conn,
            "SELECT COUNT(*) FROM memories m WHERE m.lifecycle_state='active'
               AND NOT EXISTS (SELECT 1 FROM memory_relation r WHERE r.source_id = m.id OR r.target_id = m.id)",
            "SELECT m.id FROM memories m WHERE m.lifecycle_state='active'
               AND NOT EXISTS (SELECT 1 FROM memory_relation r WHERE r.source_id = m.id OR r.target_id = m.id)
               ORDER BY m.id",
        ),
        "active memories with no incident memory_relation edge",
    );

    // Provenance gaps: active rows carrying no source identity, or the
    // explicit unavailable marker for producer provenance.
    let provenance_gaps = dimension(
        probe_ids(
            conn,
            "SELECT COUNT(*) FROM memories WHERE lifecycle_state='active'
               AND (source_ids = '[]' OR producer IN ('unknown', 'unavailable_legacy'))",
            "SELECT id FROM memories WHERE lifecycle_state='active'
               AND (source_ids = '[]' OR producer IN ('unknown', 'unavailable_legacy'))
               ORDER BY id",
        ),
        "active memories with empty source_ids or an unavailable producer marker",
    );

    // Lifecycle/supersession anomalies, mirrored from the recall-eligibility
    // invariant in store.rs: active rows must have no superseded_by, a
    // superseded row must name its successor, and every superseded_by must
    // resolve to a real memory row.
    let lifecycle_anomaly = dimension(
        probe_ids(
            conn,
            "SELECT COUNT(*) FROM memories m
               WHERE (m.lifecycle_state='active' AND m.superseded_by IS NOT NULL)
                  OR (m.lifecycle_state='superseded' AND m.superseded_by IS NULL)
                  OR (m.superseded_by IS NOT NULL
                      AND NOT EXISTS (SELECT 1 FROM memories t WHERE t.id = m.superseded_by))",
            "SELECT m.id FROM memories m
               WHERE (m.lifecycle_state='active' AND m.superseded_by IS NOT NULL)
                  OR (m.lifecycle_state='superseded' AND m.superseded_by IS NULL)
                  OR (m.superseded_by IS NOT NULL
                      AND NOT EXISTS (SELECT 1 FROM memories t WHERE t.id = m.superseded_by))
               ORDER BY m.id",
        ),
        "lifecycle_state/superseded_by inconsistency or a dangling supersession target",
    );

    // Embedding projection drift: active memory rows the embedding
    // projection has not materialized. Mirrors the doctor MRD-EMBED-SHORT
    // critical finding class.
    let projection_drift = dimension(
        probe_ids(
            conn,
            "SELECT COUNT(*) FROM memories WHERE lifecycle_state='active' AND embedding IS NULL",
            "SELECT id FROM memories WHERE lifecycle_state='active' AND embedding IS NULL ORDER BY id",
        ),
        "active memories with no embedding projection materialized",
    );

    // Observed recall: production-traffic recall_log rows only (smoke rows
    // are isolated in recall_log_smoke). Aggregate-only — no per-row ids are
    // published.
    let observed_recall = dimension(
        probe_scalar(
            conn,
            "SELECT COUNT(*) FROM recall_log WHERE traffic_class='production'",
        ),
        "production recall_log observations; smoke traffic is excluded by traffic_class",
    );

    // Crowding: the largest active-memory population inside one scope.
    let crowding = dimension(
        probe_scalar(
            conn,
            "SELECT COALESCE(MAX(c), 0) FROM (
               SELECT COUNT(*) AS c FROM memories
                 WHERE lifecycle_state='active' GROUP BY scope_id)",
        ),
        "largest active-memory count held by a single scope_id",
    );

    // Review backlog: active memories whose review_after_ms is already due.
    let review_backlog = dimension(
        probe_ids(
            conn,
            &format!(
                "SELECT COUNT(*) FROM memories WHERE lifecycle_state='active'
                   AND review_after_ms IS NOT NULL AND review_after_ms <= {now_ms}"
            ),
            &format!(
                "SELECT id FROM memories WHERE lifecycle_state='active'
                   AND review_after_ms IS NOT NULL AND review_after_ms <= {now_ms}
                   ORDER BY review_after_ms, id"
            ),
        ),
        "active memories whose review_after_ms deadline has passed",
    );

    json!({
        "duplication": duplication,
        "relationIntegrity": relation_integrity,
        "isolation": isolation,
        "provenanceGaps": provenance_gaps,
        "lifecycleAnomaly": lifecycle_anomaly,
        "projectionDrift": projection_drift,
        "observedRecall": observed_recall,
        "crowding": crowding,
        "reviewBacklog": review_backlog,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn seed(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE memories (
                id TEXT PRIMARY KEY, lifecycle_state TEXT NOT NULL DEFAULT 'active',
                effective_until_ms INTEGER, expires_at_ms INTEGER,
                content_hash TEXT, source_ids TEXT NOT NULL DEFAULT '[]',
                producer TEXT NOT NULL DEFAULT 'manual', scope_id TEXT NOT NULL DEFAULT 'global',
                embedding BLOB, superseded_by TEXT, review_after_ms INTEGER
            );
            CREATE TABLE memory_quarantine (id TEXT PRIMARY KEY);
            CREATE TABLE memory_relation (
                relation_id TEXT PRIMARY KEY, source_id TEXT NOT NULL, target_id TEXT NOT NULL,
                relation TEXT NOT NULL, provenance_producer TEXT NOT NULL,
                provenance_ref TEXT NOT NULL, created_at TEXT NOT NULL
            );
            CREATE TABLE recall_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT, ts TEXT NOT NULL,
                traffic_class TEXT NOT NULL DEFAULT 'production'
            );
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
        // Empty-but-readable sources still observe real zero counts per
        // dimension — they are absent only when their source is unavailable.
        let dims = &report["dimensions"];
        assert_eq!(dims["duplication"]["count"], 0);
        assert_eq!(dims["isolation"]["count"], 0);
        assert_eq!(dims["observedRecall"]["count"], 0);
        assert_eq!(dims["reviewBacklog"]["count"], 0);
    }

    #[test]
    fn absent_dimension_sources_degrade_typed_not_evaluated() {
        // A fixture without memory_relation/recall_log/lifecycle columns must
        // not fail the report; each dimension reports typed not_evaluated.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE memories (id TEXT PRIMARY KEY, lifecycle_state TEXT NOT NULL DEFAULT 'active');
             CREATE TABLE memory_quarantine (id TEXT PRIMARY KEY);
             CREATE TABLE context_feedback (
                trace_id TEXT NOT NULL, candidate_id TEXT NOT NULL, outcome TEXT NOT NULL,
                verified INTEGER NOT NULL, ts TEXT NOT NULL,
                PRIMARY KEY (trace_id, candidate_id));",
        )
        .unwrap();
        let report = build_sentinel_report_from(&conn).expect("report present");
        let dims = &report["dimensions"];
        for key in [
            "duplication",
            "relationIntegrity",
            "isolation",
            "provenanceGaps",
            "lifecycleAnomaly",
            "projectionDrift",
            "observedRecall",
            "crowding",
            "reviewBacklog",
        ] {
            assert_eq!(dims[key]["state"], "not_evaluated", "{key}");
            assert_eq!(dims[key]["reason"], "source_unavailable", "{key}");
        }
        assert_eq!(report["lifecycle"]["active"], 0);
    }

    #[test]
    fn dimensions_count_real_rows_and_bound_drilldown_ids() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        conn.execute_batch(
            "INSERT INTO memories (id, lifecycle_state, content_hash, source_ids, scope_id)
               VALUES
               ('a1','active','h1','[\"s1\"]','scope-a'),
               ('a2','active','h1','[\"s1\"]','scope-a'),
               ('a3','active','h2','[]','scope-b'),
               ('s1','superseded',NULL,'[\"s\"]','scope-a');
             UPDATE memories SET superseded_by='missing-target' WHERE id='s1';
             INSERT INTO memory_relation
               (relation_id, source_id, target_id, relation, provenance_producer, provenance_ref, created_at)
               VALUES
               ('r-ok','a1','a2','supports','test','ref','t'),
               ('r-dangling','a1','ghost','supports','test','ref','t');
             INSERT INTO recall_log (ts, traffic_class) VALUES
               ('2026-09-01T00:00:00Z','production'),
               ('2026-09-02T00:00:00Z','smoke');",
        )
        .unwrap();
        let report = build_sentinel_report_from(&conn).expect("report present");
        let dims = &report["dimensions"];
        assert_eq!(dims["duplication"]["state"], "observed");
        assert_eq!(dims["duplication"]["count"], 2); // a1+a2 share h1
        assert_eq!(dims["relationIntegrity"]["count"], 1);
        assert_eq!(dims["relationIntegrity"]["ids"][0], "r-dangling");
        // a3 is active with no incident edge (r-ok/r-dangling only touch a1/a2/ghost).
        assert_eq!(dims["isolation"]["count"], 1);
        assert_eq!(dims["isolation"]["ids"][0], "a3");
        assert_eq!(dims["provenanceGaps"]["count"], 1); // a3 has source_ids=[]
        // s1: superseded with a dangling superseded_by target.
        assert_eq!(dims["lifecycleAnomaly"]["count"], 1);
        assert_eq!(dims["lifecycleAnomaly"]["ids"][0], "s1");
        assert_eq!(dims["projectionDrift"]["count"], 3); // a1,a2,a3 active, embedding NULL
        assert_eq!(dims["observedRecall"]["count"], 1); // production only
        assert_eq!(dims["crowding"]["count"], 2); // scope-a has a1+a2 active
        assert_eq!(dims["reviewBacklog"]["count"], 0);
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
