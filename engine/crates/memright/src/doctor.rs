//! Versioned, read-only health checks for the currently installed schema.
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReportV0 {
    pub schema_version: &'static str,
    pub status: &'static str,
    pub checks: Vec<DoctorCheckV0>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheckV0 {
    pub code: &'static str,
    pub severity: &'static str,
    pub count: usize,
    pub sample_ids: Vec<String>,
    pub repair: &'static str,
}

pub fn run(path: impl AsRef<Path>) -> Result<DoctorReportV0, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| error.to_string())?;
    let mut checks = vec![
        check(
            &connection,
            "MRD-EMBED-MODEL-DRIFT",
            "warning",
            "SELECT id FROM memories WHERE embed_model IS NULL OR trim(embed_model) = ''",
            "reindex after selecting the intended embed model",
        ),
        check(
            &connection,
            "MRD-EMBED-SHORT",
            "critical",
            "SELECT id FROM memories WHERE embedding IS NULL OR length(embedding) < 8",
            "reindex affected rows; do not promote with missing embeddings",
        ),
        check(
            &connection,
            "MRD-SCOPE-ANOMALY",
            "warning",
            "SELECT id FROM memories WHERE scope_id IS NULL OR trim(scope_id) = ''",
            "normalize affected scopes with platform-aware scope tooling",
        ),
        check(
            &connection,
            "MRD-EFFECTIVENESS-UNVERIFIED",
            "info",
            "SELECT id FROM memories WHERE inject_count > 0 AND access_count = 0",
            "collect verified access feedback before curation decisions",
        ),
    ];
    checks.push(if table_exists(&connection, "links") {
        check(&connection, "MRD-DANGLING-WIKILINK", "warning", "SELECT dst_slug FROM links l WHERE NOT EXISTS (SELECT 1 FROM memories m WHERE m.id = l.dst_slug OR m.id LIKE '%' || '/' || l.dst_slug)", "repair link target or add an external-reference allowlist entry")
    } else {
        DoctorCheckV0 { code: "MRD-DANGLING-WIKILINK", severity: "info", count: 0, sample_ids: Vec::new(), repair: "links table absent in this schema" }
    });
    if column_exists(&connection, "memories", "lifecycle_state") {
        checks.extend([
            check(&connection, "MRD-LIFECYCLE-DANGLING", "critical", "SELECT id FROM memories WHERE superseded_by IS NOT NULL AND NOT EXISTS (SELECT 1 FROM memories successor WHERE successor.id=memories.superseded_by)", "restore the verified successor or clear invalid supersession"),
            check(&connection, "MRD-LIFECYCLE-CYCLE", "critical", "WITH RECURSIVE chain(origin, next, depth) AS (SELECT id, superseded_by, 0 FROM memories WHERE superseded_by IS NOT NULL UNION ALL SELECT chain.origin, memories.superseded_by, chain.depth + 1 FROM chain JOIN memories ON memories.id=chain.next WHERE chain.next IS NOT NULL AND chain.depth < 100) SELECT DISTINCT origin FROM chain WHERE next=origin", "break the cycle with a typed supersession adjudication"),
            check(&connection, "MRD-LIFECYCLE-WINDOW", "warning", "SELECT id FROM memories WHERE effective_from_ms IS NOT NULL AND effective_until_ms IS NOT NULL AND effective_until_ms <= effective_from_ms", "repair half-open effective interval"),
            check(&connection, "MRD-PROTECTED-EXPIRED", "warning", "SELECT id FROM memories WHERE priority_class='protected' AND ((effective_until_ms IS NOT NULL AND effective_until_ms <= (unixepoch('now') * 1000)) OR (expires_at_ms IS NOT NULL AND expires_at_ms <= (unixepoch('now') * 1000)))", "retain if required, but reverify or retire expired protected row"),
        ]);
    }
    let status = if checks
        .iter()
        .any(|check| check.severity == "critical" && check.count > 0)
    {
        "critical"
    } else if checks
        .iter()
        .any(|check| check.severity == "warning" && check.count > 0)
    {
        "warning"
    } else {
        "ok"
    };
    Ok(DoctorReportV0 {
        schema_version: "MemRightDoctorV0",
        status,
        checks,
    })
}

fn table_exists(connection: &Connection, name: &str) -> bool {
    connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> bool {
    connection
        .query_row(
            &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name=?1"),
            [column],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0
}

fn check(
    connection: &Connection,
    code: &'static str,
    severity: &'static str,
    sql: &str,
    repair: &'static str,
) -> DoctorCheckV0 {
    let samples = connection
        .prepare(sql)
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .map(|rows| rows.filter_map(Result::ok).take(20).collect::<Vec<_>>())
        })
        .unwrap_or_default();
    DoctorCheckV0 {
        code,
        severity,
        count: samples.len(),
        sample_ids: samples,
        repair,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_is_read_only_and_uses_locked_effectiveness_label() {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("doctor.db");
        let connection = Connection::open(&db).unwrap();
        connection.execute_batch("CREATE TABLE memories(id TEXT, embed_model TEXT, embedding BLOB, scope_id TEXT, inject_count INTEGER, access_count INTEGER); CREATE TABLE links(dst_slug TEXT); INSERT INTO memories VALUES ('x', NULL, X'00', '', 1, 0); INSERT INTO links VALUES ('missing');").unwrap();
        drop(connection);
        let before = std::fs::read(&db).unwrap();
        let report = run(&db).unwrap();
        assert_eq!(report.schema_version, "MemRightDoctorV0");
        assert!(report
            .checks
            .iter()
            .any(|check| check.code == "MRD-EFFECTIVENESS-UNVERIFIED"));
        assert_eq!(before, std::fs::read(&db).unwrap());
    }
}
