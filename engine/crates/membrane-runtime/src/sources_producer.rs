//! MBR: producer for the Hub's sources/repositories section.
//!
//! `sources_explorer::project_sources_explorer` (src/sources_explorer.rs) is
//! a real, structurally complete projector, but nothing previously assembled
//! the `payload` JSON it expects. This module reads the local `doc_artifacts`
//! table (the repository/doc catalog maintained by `doc_spine.rs`) and builds
//! that payload. Any DB-access or query failure returns `None` so the caller
//! falls back to the honest "unavailable" facade — we never fabricate a
//! repository snapshot.

use serde_json::{json, Value};

use crate::hub_readonly_db::{now_unix_ms, open_readonly};

const MAX_PATHS: usize = 64;

/// Assemble the `sources_explorer` payload from the most-recently-updated
/// repository present in `doc_artifacts`. Returns `None` when the database is
/// unreachable or no documents are indexed.
pub fn build_sources_payload() -> Option<Value> {
    let conn = open_readonly()?;
    build_sources_payload_from(&conn)
}

pub(crate) fn build_sources_payload_from(conn: &rusqlite::Connection) -> Option<Value> {
    let (repository_root, repository_id, revision, generation, updated_at_ms): (
        String,
        String,
        String,
        i64,
        i64,
    ) = conn
        .query_row(
            "SELECT repository_root, repository_id, revision, MAX(index_generation), MAX(updated_at_ms)
             FROM doc_artifacts
             GROUP BY repository_root
             ORDER BY MAX(updated_at_ms) DESC
             LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .ok()?;

    let mut path_stmt = conn
        .prepare(
            "SELECT path, document_class, trust_label, lifecycle_state
             FROM doc_artifacts
             WHERE repository_root = ?1
             ORDER BY updated_at_ms DESC
             LIMIT ?2",
        )
        .ok()?;
    let paths: Vec<Value> = path_stmt
        .query_map(rusqlite::params![repository_root, MAX_PATHS as i64], |r| {
            let path: String = r.get(0)?;
            let kind: String = r.get(1)?;
            let trust: String = r.get(2)?;
            let lifecycle: String = r.get(3)?;
            Ok(json!({
                "path": path,
                "kind": kind,
                "evidence": format!("trust={trust} lifecycle={lifecycle}"),
            }))
        })
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;

    let recent: Option<(String, String, String, i64, String, String)> = conn
        .query_row(
            "SELECT doc_id, title, summary, updated_at_ms, path, parser_version
             FROM doc_artifacts
             WHERE repository_root = ?1
             ORDER BY updated_at_ms DESC
             LIMIT 1",
            [&repository_root],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .ok();

    let now_ms = now_unix_ms();
    let source_age_ms = now_ms.saturating_sub(updated_at_ms.max(0) as u64);

    let (contribution, parser_version) = match recent {
        Some((id, title, summary, observed_at, path, parser_version)) => (
            json!({
                "id": id,
                "summary": if summary.trim().is_empty() { title } else { summary },
                "paths": [path],
                "observedAtUnixMs": observed_at.max(0) as u64,
            }),
            parser_version,
        ),
        None => (json!({}), "unknown".to_string()),
    };

    Some(json!({
        "repository": {
            "id": repository_id,
            "origin": repository_root,
            "revision": revision,
        },
        "generation": {
            "id": generation.to_string(),
            "release": generation.to_string(),
            "index": generation.to_string(),
        },
        "clocks": {
            "observedAtUnixMs": now_ms,
            "contributionAtUnixMs": updated_at_ms.max(0) as u64,
            "sourceAgeMs": source_age_ms,
        },
        "readiness": {
            "state": "ready",
            "reason": "doc_artifacts row observed",
            "authoritative": true,
            "observedAtUnixMs": now_ms,
        },
        "parser": {
            "name": "doc_spine",
            "version": parser_version,
            "languages": [],
            "state": "observed",
        },
        "recentContribution": contribution,
        "paths": paths,
        // No doc-level adjacency/graph concept exists yet (links table is
        // memory-scoped, not doc-scoped); left honestly empty rather than
        // fabricated.
        "neighborhoods": [],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn seed(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE doc_artifacts (
                doc_id TEXT NOT NULL, repository_root TEXT NOT NULL, repository_id TEXT NOT NULL,
                revision TEXT NOT NULL, path TEXT NOT NULL, content_hash TEXT NOT NULL,
                parser_version TEXT NOT NULL, document_class TEXT NOT NULL,
                lifecycle_state TEXT NOT NULL DEFAULT 'active', trust_label TEXT NOT NULL,
                influence_class TEXT NOT NULL, sensitivity TEXT NOT NULL, generated INTEGER NOT NULL DEFAULT 0,
                index_generation INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
                title TEXT NOT NULL DEFAULT '', summary TEXT NOT NULL DEFAULT '',
                keywords_json TEXT NOT NULL DEFAULT '[]', superseded_by TEXT
            );",
        )
        .unwrap();
    }

    #[test]
    fn empty_database_fails_closed() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        assert!(build_sources_payload_from(&conn).is_none());
    }

    #[test]
    fn missing_table_fails_closed() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(build_sources_payload_from(&conn).is_none());
    }

    #[test]
    fn populated_repository_produces_payload() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        conn.execute(
            "INSERT INTO doc_artifacts (doc_id, repository_root, repository_id, revision, path,
                content_hash, parser_version, document_class, lifecycle_state, trust_label,
                influence_class, sensitivity, index_generation, updated_at_ms, title, summary)
             VALUES ('d1','/repo','repo-id','rev1','README.md','h1','v1','doc','active','trusted',
                'reference','public',3,1000,'Readme','A readme summary')",
            [],
        )
        .unwrap();
        let payload = build_sources_payload_from(&conn).expect("payload present");
        assert_eq!(payload["repository"]["id"], "repo-id");
        assert_eq!(payload["paths"].as_array().unwrap().len(), 1);
        assert_eq!(payload["recentContribution"]["summary"], "A readme summary");
        assert_eq!(payload["readiness"]["state"], "ready");
    }
}
