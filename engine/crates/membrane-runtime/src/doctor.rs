//! Versioned, read-only health checks for the currently installed schema.
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReportV0 {
    pub schema_version: &'static str,
    pub system: &'static str,
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
    run_with_policy(path, &[], &[])
}

/// Runs the fixed v0 checks while omitting only explicitly named, stable finding codes.
/// Suppressions are caller-owned policy; unknown names have no effect.
pub fn run_with_suppressions(
    path: impl AsRef<Path>,
    suppressed_codes: &[&str],
) -> Result<DoctorReportV0, String> {
    run_with_policy(path, suppressed_codes, &[])
}

/// Runs doctor checks while explicitly allowing known external wikilink targets.
/// External references are exact slugs supplied by the caller; unresolved targets
/// remain findings by default.
pub fn run_with_policy(
    path: impl AsRef<Path>,
    suppressed_codes: &[&str],
    external_refs: &[&str],
) -> Result<DoctorReportV0, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| error.to_string())?;
    let embedding_check = if column_exists(&connection, "memories", "embedding_q")? {
        check_embeddings(&connection)?
    } else {
        check(
            &connection,
            "MRD-EMBED-SHORT",
            "critical",
            "SELECT id FROM memories WHERE embedding IS NULL OR length(embedding) < 8",
            "reindex affected rows; do not promote with missing embeddings",
        )?
    };
    let mut checks = vec![
        check(
            &connection,
            "MRD-EMBED-MODEL-DRIFT",
            "warning",
            "SELECT id FROM memories WHERE embed_model IS NULL OR trim(embed_model) = ''",
            "reindex after selecting the intended embed model",
        )?,
        embedding_check,
        check_scope_anomalies(&connection)?,
        check(
            &connection,
            "MRD-EFFECTIVENESS-UNVERIFIED",
            "info",
            "SELECT id FROM memories WHERE inject_count > 0 AND access_count = 0",
            "collect verified access feedback before curation decisions",
        )?,
    ];
    checks.push(if table_exists(&connection, "links")? {
        check_dangling_wikilinks(&connection, external_refs)?
    } else {
        DoctorCheckV0 {
            code: "MRD-DANGLING-WIKILINK",
            severity: "info",
            count: 0,
            sample_ids: Vec::new(),
            repair: "links table absent in this schema",
        }
    });
    if column_exists(&connection, "memories", "lifecycle_state")? {
        checks.extend([
            check(&connection, "MRD-LIFECYCLE-DANGLING", "critical", "SELECT id FROM memories WHERE superseded_by IS NOT NULL AND NOT EXISTS (SELECT 1 FROM memories successor WHERE successor.id=memories.superseded_by)", "restore the verified successor or clear invalid supersession")?,
            check(&connection, "MRD-LIFECYCLE-CYCLE", "critical", "WITH RECURSIVE chain(origin, next, depth) AS (SELECT id, superseded_by, 0 FROM memories WHERE superseded_by IS NOT NULL UNION ALL SELECT chain.origin, memories.superseded_by, chain.depth + 1 FROM chain JOIN memories ON memories.id=chain.next WHERE chain.next IS NOT NULL AND chain.depth < 100) SELECT DISTINCT origin FROM chain WHERE next=origin", "break the cycle with a typed supersession adjudication")?,
            check(&connection, "MRD-LIFECYCLE-WINDOW", "warning", "SELECT id FROM memories WHERE effective_from_ms IS NOT NULL AND effective_until_ms IS NOT NULL AND effective_until_ms <= effective_from_ms", "repair half-open effective interval")?,
            check(&connection, "MRD-PROTECTED-EXPIRED", "warning", "SELECT id FROM memories WHERE priority_class='protected' AND ((effective_until_ms IS NOT NULL AND effective_until_ms <= (unixepoch('now') * 1000)) OR (expires_at_ms IS NOT NULL AND expires_at_ms <= (unixepoch('now') * 1000)))", "retain if required, but reverify or retire expired protected row")?,
            check(&connection, "MRD-LIFECYCLE-EXPIRED", "warning", "SELECT id FROM memories WHERE lifecycle_state='active' AND record_type!='checkpoint' AND ((effective_until_ms IS NOT NULL AND effective_until_ms <= (unixepoch('now') * 1000)) OR (expires_at_ms IS NOT NULL AND expires_at_ms <= (unixepoch('now') * 1000)))", "retire or reverify the expired active row")?,
            check(&connection, "MRD-SUPERSEDED-INJECTED", "critical", "SELECT DISTINCT memories.id FROM memories JOIN memory_event_log ON memory_event_log.memory_id=memories.id WHERE memories.lifecycle_state='superseded' AND memories.effective_until_ms IS NOT NULL AND memory_event_log.event_kind='injected' AND (unixepoch(memory_event_log.ts) * 1000) >= memories.effective_until_ms", "repair admission so superseded rows cannot be injected")?,
            check(&connection, "MRD-CHECKPOINT-EXPIRED", "critical", "SELECT id FROM memories WHERE record_type='checkpoint' AND lifecycle_state='active' AND expires_at_ms IS NOT NULL AND expires_at_ms <= (unixepoch('now') * 1000)", "retire expired checkpoints from active delivery")?,
        ]);
    }
    checks.retain(|check| !suppressed_codes.contains(&check.code));
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
        schema_version: "CryptDoctorV0",
        system: "Membrane",
        status,
        checks,
    })
}

fn check_dangling_wikilinks(
    connection: &Connection,
    external_refs: &[&str],
) -> Result<DoctorCheckV0, String> {
    let mut statement = connection
        .prepare("SELECT dst_slug FROM links l WHERE NOT EXISTS (SELECT 1 FROM memories m WHERE m.id = l.dst_slug OR m.id LIKE '%' || '/' || l.dst_slug) ORDER BY dst_slug")
        .map_err(|error| format!("MRD-DANGLING-WIKILINK: query failed: {error}"))?;
    let mut targets = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("MRD-DANGLING-WIKILINK: query failed: {error}"))?
        .filter_map(Result::ok)
        .filter(|target| !external_refs.contains(&target.as_str()))
        .collect::<Vec<_>>();
    let count = targets.len();
    targets.sort();
    targets.dedup();
    targets.truncate(20);
    Ok(DoctorCheckV0 {
        code: "MRD-DANGLING-WIKILINK",
        severity: "warning",
        count,
        sample_ids: targets,
        repair: "repair link target or add an external-reference allowlist entry",
    })
}

fn check_embeddings(connection: &Connection) -> Result<DoctorCheckV0, String> {
    let mut statement = connection
        .prepare("SELECT id, embedding, embedding_q FROM memories")
        .map_err(|error| format!("MRD-EMBED-SHORT: query failed: {error}"))?;
    let invalid = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<Vec<u8>>>(1)?,
                row.get::<_, Option<Vec<u8>>>(2)?,
            ))
        })
        .map_err(|error| format!("MRD-EMBED-SHORT: query failed: {error}"))?
        .filter_map(|row| match row {
            Ok((id, legacy, quantized))
                if !valid_embedding(legacy.as_deref(), quantized.as_deref()) =>
            {
                Some(Ok(id))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("MRD-EMBED-SHORT: row failed: {error}"))?;
    let mut sample_ids = invalid.clone();
    sample_ids.sort();
    sample_ids.truncate(20);
    Ok(DoctorCheckV0 {
        code: "MRD-EMBED-SHORT",
        severity: "critical",
        count: invalid.len(),
        sample_ids,
        repair: "reindex affected rows; do not promote with missing embeddings",
    })
}

fn valid_embedding(legacy: Option<&[u8]>, quantized: Option<&[u8]>) -> bool {
    match quantized {
        Some(blob) => valid_quantized_embedding(blob),
        None => legacy.is_some_and(|blob| blob.len() >= 8),
    }
}

fn check_scope_anomalies(connection: &Connection) -> Result<DoctorCheckV0, String> {
    let mut statement = connection
        .prepare("SELECT id, scope_id FROM memories")
        .map_err(|error| format!("MRD-SCOPE-ANOMALY: query failed: {error}"))?;
    let invalid = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(|error| format!("MRD-SCOPE-ANOMALY: query failed: {error}"))?
        .filter_map(|row| match row {
            Ok((_id, Some(scope)))
                if !scope.trim().is_empty() && crate::scope::normalize_scope(&scope) == scope =>
            {
                None
            }
            Ok((id, _)) => Some(Ok(id)),
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("MRD-SCOPE-ANOMALY: row failed: {error}"))?;
    let mut sample_ids = invalid.clone();
    sample_ids.sort();
    sample_ids.truncate(20);
    Ok(DoctorCheckV0 {
        code: "MRD-SCOPE-ANOMALY",
        severity: "warning",
        count: invalid.len(),
        sample_ids,
        repair: "normalize affected scopes with platform-aware scope tooling",
    })
}

fn valid_quantized_embedding(blob: &[u8]) -> bool {
    if blob.len() < 8 {
        return false;
    }
    let scale = f32::from_le_bytes([blob[0], blob[1], blob[2], blob[3]]);
    scale.is_finite() && scale > 0.0
}

fn table_exists(connection: &Connection, name: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .map_err(|error| format!("inspect table {name}: {error}"))
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool, String> {
    connection
        .query_row(
            &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name=?1"),
            [column],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .map_err(|error| format!("inspect column {table}.{column}: {error}"))
}

fn check(
    connection: &Connection,
    code: &'static str,
    severity: &'static str,
    sql: &str,
    repair: &'static str,
) -> Result<DoctorCheckV0, String> {
    let count_sql = format!("SELECT COUNT(*) FROM ({sql})");
    let count = connection
        .query_row(&count_sql, [], |row| row.get::<_, i64>(0))
        .map_err(|error| format!("{code}: count query failed: {error}"))?;
    let sample_sql = format!("SELECT * FROM ({sql}) ORDER BY 1 LIMIT 20");
    let mut statement = connection
        .prepare(&sample_sql)
        .map_err(|error| format!("{code}: sample query failed: {error}"))?;
    let samples = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("{code}: sample query failed: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("{code}: sample row failed: {error}"))?;
    Ok(DoctorCheckV0 {
        code,
        severity,
        count: usize::try_from(count).map_err(|_| format!("{code}: count exceeds usize"))?,
        sample_ids: samples,
        repair,
    })
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
        assert_eq!(report.schema_version, "CryptDoctorV0");
        assert!(report
            .checks
            .iter()
            .any(|check| check.code == "MRD-EFFECTIVENESS-UNVERIFIED"));
        assert_eq!(before, std::fs::read(&db).unwrap());
    }

    fn check_by_code<'a>(report: &'a DoctorReportV0, code: &str) -> &'a DoctorCheckV0 {
        report
            .checks
            .iter()
            .find(|check| check.code == code)
            .unwrap_or_else(|| panic!("missing doctor check {code}"))
    }

    #[test]
    fn doctor_reports_real_count_with_sorted_capped_samples() {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("doctor-count.db");
        let connection = Connection::open(&db).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE memories(id TEXT, embed_model TEXT, embedding BLOB, scope_id TEXT, inject_count INTEGER, access_count INTEGER);",
            )
            .unwrap();
        for index in (0..21).rev() {
            connection
                .execute(
                    "INSERT INTO memories VALUES (?1, NULL, X'00000000', 'global', 0, 0)",
                    [format!("memory-{index:02}")],
                )
                .unwrap();
        }
        drop(connection);

        let report = run(&db).unwrap();
        let check = check_by_code(&report, "MRD-EMBED-MODEL-DRIFT");
        assert_eq!(check.count, 21);
        assert_eq!(check.sample_ids.len(), 20);
        assert_eq!(check.sample_ids.first().unwrap(), "memory-00");
        assert_eq!(check.sample_ids.last().unwrap(), "memory-19");
    }

    #[test]
    fn doctor_effectiveness_unverified_means_injected_without_verified_access() {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("doctor-effectiveness.db");
        let connection = Connection::open(&db).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE memories(id TEXT, embed_model TEXT, embedding BLOB, scope_id TEXT, inject_count INTEGER, access_count INTEGER);
                 INSERT INTO memories VALUES
                    ('never-injected', 'model', X'00000000', 'global', 0, 0),
                    ('unverified', 'model', X'00000000', 'global', 1, 0),
                    ('verified', 'model', X'00000000', 'global', 1, 1),
                    ('accessed-without-injection', 'model', X'00000000', 'global', 0, 1);",
            )
            .unwrap();
        drop(connection);

        let report = run(&db).unwrap();
        let check = check_by_code(&report, "MRD-EFFECTIVENESS-UNVERIFIED");
        assert_eq!(check.count, 1);
        assert_eq!(check.sample_ids, vec!["unverified"]);
    }

    #[test]
    fn doctor_uses_nontrivial_quantized_embeddings_when_legacy_blobs_are_null() {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("doctor-quantized-embeddings.db");
        let connection = Connection::open(&db).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE memories(id TEXT, embed_model TEXT, embedding BLOB, embedding_q BLOB, scope_id TEXT, inject_count INTEGER, access_count INTEGER);
                 INSERT INTO memories VALUES
                    ('valid-q', 'model', NULL, X'0000803F01020304', 'global', 0, 0),
                    ('missing', 'model', NULL, NULL, 'global', 0, 0),
                    ('short-q', 'model', NULL, X'0000803F', 'global', 0, 0),
                    ('zero-scale-q', 'model', NULL, X'0000000001020304', 'global', 0, 0),
                    ('nan-scale-q', 'model', NULL, X'0000C07F01020304', 'global', 0, 0),
                    ('legacy', 'model', X'0000000000000000', NULL, 'global', 0, 0);",
            )
            .unwrap();
        drop(connection);

        let report = run(&db).unwrap();
        let check = check_by_code(&report, "MRD-EMBED-SHORT");
        assert_eq!(check.count, 4);
        assert_eq!(
            check.sample_ids,
            vec!["missing", "nan-scale-q", "short-q", "zero-scale-q"]
        );
    }

    #[test]
    fn doctor_stably_suppresses_lifecycle_checks_for_v19_shape() {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("doctor-v19.db");
        let connection = Connection::open(&db).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE memories(id TEXT, embed_model TEXT, embedding BLOB, scope_id TEXT, inject_count INTEGER, access_count INTEGER);
                 PRAGMA user_version = 19;",
            )
            .unwrap();
        drop(connection);

        let report = run(&db).unwrap();
        for code in [
            "MRD-LIFECYCLE-DANGLING",
            "MRD-LIFECYCLE-CYCLE",
            "MRD-LIFECYCLE-WINDOW",
            "MRD-PROTECTED-EXPIRED",
        ] {
            assert!(report.checks.iter().all(|check| check.code != code));
        }
    }

    #[test]
    fn doctor_fails_closed_when_a_check_query_cannot_read_ids() {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("doctor-query-error.db");
        let connection = Connection::open(&db).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE memories(id BLOB, embed_model TEXT, embedding BLOB, scope_id TEXT, inject_count INTEGER, access_count INTEGER);
                 INSERT INTO memories VALUES (X'00', NULL, X'00000000', 'global', 0, 0);",
            )
            .unwrap();
        drop(connection);

        let error = run(&db).unwrap_err();
        assert!(error.contains("MRD-EMBED-SHORT"), "{error}");
    }

    #[test]
    fn doctor_accepts_only_explicit_suppression_codes() {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("doctor-suppression.db");
        let connection = Connection::open(&db).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE memories(id TEXT, embed_model TEXT, embedding BLOB, scope_id TEXT, inject_count INTEGER, access_count INTEGER);
                 INSERT INTO memories VALUES ('x', NULL, X'00000000', 'd--Claude', 0, 0);",
            )
            .unwrap();
        drop(connection);

        let report = run_with_suppressions(&db, &["MRD-EMBED-MODEL-DRIFT"]).unwrap();
        assert!(report
            .checks
            .iter()
            .all(|check| check.code != "MRD-EMBED-MODEL-DRIFT"));
        assert_eq!(check_by_code(&report, "MRD-SCOPE-ANOMALY").count, 1);
    }

    #[test]
    fn doctor_filters_only_exact_external_reference_slugs() {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("doctor-external-refs.db");
        let connection = Connection::open(&db).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE memories(id TEXT, embed_model TEXT, embedding BLOB, scope_id TEXT, inject_count INTEGER, access_count INTEGER);
                 CREATE TABLE links(src_id TEXT, dst_slug TEXT);
                 INSERT INTO links VALUES ('source', 'external-doc'), ('source', 'missing-internal');",
            )
            .unwrap();
        drop(connection);

        let report = run_with_policy(&db, &[], &["external-doc"]).unwrap();
        let check = check_by_code(&report, "MRD-DANGLING-WIKILINK");
        assert_eq!(check.count, 1);
        assert_eq!(check.sample_ids, vec!["missing-internal"]);
    }

    #[test]
    fn doctor_reports_expired_lifecycle_superseded_injection_and_checkpoint_delivery() {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("doctor-lifecycle-delivery.db");
        let connection = Connection::open(&db).unwrap();
        connection.execute_batch(
            "CREATE TABLE memories(id TEXT, embed_model TEXT, embedding BLOB, scope_id TEXT, inject_count INTEGER, access_count INTEGER, lifecycle_state TEXT, superseded_by TEXT, effective_from_ms INTEGER, effective_until_ms INTEGER, expires_at_ms INTEGER, priority_class TEXT, record_type TEXT);
             CREATE TABLE memory_event_log(memory_id TEXT, event_kind TEXT, ts TEXT);
             INSERT INTO memories VALUES
                ('expired', 'model', X'00000000', 'global', 0, 0, 'active', NULL, NULL, NULL, 1, 'normal', 'memory'),
                ('superseded', 'model', X'00000000', 'global', 1, 0, 'superseded', 'replacement', NULL, 1, NULL, 'normal', 'memory'),
                ('checkpoint', 'model', X'00000000', 'global', 0, 0, 'active', NULL, NULL, NULL, 1, 'normal', 'checkpoint');
             INSERT INTO memory_event_log VALUES ('superseded', 'injected', '1970-01-01T00:00:01Z');",
        ).unwrap();
        drop(connection);

        let report = run(&db).unwrap();
        assert_eq!(
            check_by_code(&report, "MRD-LIFECYCLE-EXPIRED").sample_ids,
            vec!["expired"]
        );
        assert_eq!(
            check_by_code(&report, "MRD-SUPERSEDED-INJECTED").sample_ids,
            vec!["superseded"]
        );
        assert_eq!(
            check_by_code(&report, "MRD-CHECKPOINT-EXPIRED").sample_ids,
            vec!["checkpoint"]
        );
    }
}
