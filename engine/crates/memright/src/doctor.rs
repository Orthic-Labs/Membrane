//! Read-only database health checks.
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct Check {
    pub check: String,
    pub status: String,
    pub count: usize,
    pub ids: Vec<String>,
}

fn run_check(conn: &Connection, name: &str, sql: &str) -> Check {
    if name == "dangling-links"
        && conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='links'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
            == 0
    {
        return Check {
            check: name.into(),
            status: "ok".into(),
            count: 0,
            ids: Vec::new(),
        };
    }
    let result = conn.prepare(sql).and_then(|mut stmt| {
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        Ok(rows.filter_map(Result::ok).take(100).collect::<Vec<_>>())
    });
    match result {
        Ok(ids) => Check {
            check: name.into(),
            status: if ids.is_empty() { "ok" } else { "warn" }.into(),
            count: ids.len(),
            ids,
        },
        Err(error) => Check {
            check: name.into(),
            status: "critical".into(),
            count: 0,
            ids: vec![error.to_string()],
        },
    }
}

pub fn run(path: impl AsRef<Path>) -> Result<Vec<Check>, String> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let mut checks = vec![
        run_check(
            &conn,
            "embed-drift",
            "SELECT id FROM memories WHERE embed_model IS NULL OR length(embedding) < 8",
        ),
        run_check(
            &conn,
            "dangling-links",
            "SELECT dst_slug FROM links l WHERE NOT EXISTS (SELECT 1 FROM memories m WHERE m.id = l.dst_slug OR m.id LIKE '%' || '/' || l.dst_slug)",
        ),
        run_check(
            &conn,
            "scope-anomalies",
            "SELECT id FROM memories WHERE scope_id IS NULL OR trim(scope_id) = ''",
        ),
        run_check(
            &conn,
            "inert-rows",
            "SELECT id FROM memories WHERE inject_count > 3 AND access_count = 0",
        ),
    ];
    let lifecycle = conn
        .prepare("SELECT name FROM pragma_table_info('memories') WHERE name IN ('valid_until','superseded_by','confidence')")
        .and_then(|mut stmt| stmt.query_map([], |row| row.get::<_, String>(0)).map(|rows| rows.flatten().collect::<Vec<_>>()));
    if lifecycle.map(|columns| columns.len() == 3).unwrap_or(false) {
        checks.push(run_check(
            &conn,
            "lifecycle-expired-active",
            "SELECT id FROM memories WHERE valid_until IS NOT NULL AND valid_until <> '' AND valid_until <= strftime('%Y-%m-%dT%H:%M:%SZ','now') AND (superseded_by IS NULL OR superseded_by = '')",
        ));
        checks.push(run_check(
            &conn,
            "lifecycle-confidence",
            "SELECT id FROM memories WHERE confidence < 0 OR confidence > 1 OR confidence IS NULL",
        ));
    }
    Ok(checks)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doctor_is_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("doctor.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE memories(id TEXT, embed_model TEXT, embedding BLOB, scope_id TEXT, inject_count INTEGER, access_count INTEGER); CREATE TABLE links(dst_slug TEXT); INSERT INTO memories VALUES ('x', NULL, X'00', '', 4, 0); INSERT INTO links VALUES ('missing');").unwrap();
        drop(conn);
        let before = std::fs::read(&db).unwrap();
        let checks = run(&db).unwrap();
        assert!(checks.iter().all(|c| c.status == "warn"));
        assert_eq!(before, std::fs::read(&db).unwrap());
    }
}
