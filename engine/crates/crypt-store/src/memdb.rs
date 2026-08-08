//! MemDb — the memory engine's own SQLite store (the `memories` table). A cloneable
//! `Arc<Mutex<Connection>>` handle, WAL, mirroring the interface `MemoryStore` needs. Self-contained
//! so Crypt is publishable without dragging in CodeRight's governance DB.

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS memories (
    id           TEXT PRIMARY KEY,
    tier         TEXT NOT NULL,
    content      TEXT NOT NULL,
    keywords     TEXT NOT NULL,
    score        REAL NOT NULL,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL DEFAULT '',
    access_count INTEGER NOT NULL,
    embedding    BLOB,
    embedding_q  BLOB,
    scope_id     TEXT NOT NULL DEFAULT 'global',
    inject_count INTEGER NOT NULL DEFAULT 0,
    content_hash TEXT,
    embed_model  TEXT,
    source_ids   TEXT NOT NULL DEFAULT '[]',
    artifact_family TEXT NOT NULL DEFAULT 'memory',
    producer     TEXT NOT NULL DEFAULT 'manual',
    record_type  TEXT NOT NULL DEFAULT 'memory',
    authority    TEXT NOT NULL DEFAULT 'A2',
    influence_class TEXT NOT NULL DEFAULT 'reference'
);
CREATE TABLE IF NOT EXISTS recall_log (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    ts             TEXT NOT NULL,
    scope          TEXT,
    query_chars    INTEGER NOT NULL,
    hit_count      INTEGER NOT NULL,
    full_chars     INTEGER NOT NULL,  -- sum of full memory content for every hit returned
    injected_chars INTEGER NOT NULL,  -- sum of the skeleton text actually sent in the response
    source         TEXT NOT NULL DEFAULT 'serve', -- 'serve' (agent-facing) | 'cli' (human debug)
    query_preview  TEXT,              -- first ~120 chars of the query, so a ranking change can be
                                      -- verified by replaying REAL traffic (T2.1 step 1a)
    -- 2026-07-09 (add-now plan, Phase 1): client attribution + hook provenance.
    -- `client` distinguishes which agent/UI issued the recall (claude|claudemm|codex|manual|smoke|cli).
    -- `session_id`/`cwd_scope`/`hook_event`/`trace_id` let us follow a single agent turn end-to-end
    -- (which project, which hook fired, which request) without joining external logs. `client_visibility`
    -- records the cross-client visibility policy in force when the row was written, so a future
    -- switch to per-client isolation is auditable from the row itself.
    client         TEXT NOT NULL DEFAULT 'unknown',
    session_id     TEXT,
    cwd_scope      TEXT,
    hook_event     TEXT,
    trace_id       TEXT,
    client_visibility TEXT NOT NULL DEFAULT 'all',
    traffic_class  TEXT NOT NULL DEFAULT 'production',
    candidate_hits TEXT NOT NULL DEFAULT '[]',
    admitted_hits  TEXT NOT NULL DEFAULT '[]',
    event_uid            TEXT,
    installation_id      TEXT,
    service_instance_id  TEXT,
    turn_id              TEXT,
    identity_status      TEXT NOT NULL DEFAULT 'legacy_unattributed'
        CHECK (identity_status IN ('observed', 'legacy_unattributed'))
);
CREATE TABLE IF NOT EXISTS recall_log_smoke (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    ts                     TEXT NOT NULL,
    scope                  TEXT,
    query_chars            INTEGER NOT NULL,
    hit_count              INTEGER NOT NULL,
    full_chars             INTEGER NOT NULL,
    injected_chars         INTEGER NOT NULL,
    source                 TEXT NOT NULL DEFAULT 'serve',
    query_preview          TEXT,
    client                 TEXT NOT NULL DEFAULT 'unknown',
    session_id             TEXT,
    cwd_scope              TEXT,
    hook_event             TEXT,
    trace_id               TEXT,
    client_visibility      TEXT NOT NULL DEFAULT 'all',
    traffic_class          TEXT NOT NULL DEFAULT 'smoke' CHECK (traffic_class != 'production'),
    candidate_hits         TEXT NOT NULL DEFAULT '[]',
    admitted_hits          TEXT NOT NULL DEFAULT '[]',
    source_recall_id       INTEGER,
    original_traffic_class TEXT NOT NULL,
    isolated_at            TEXT NOT NULL,
    isolation_reason       TEXT NOT NULL,
    event_uid              TEXT,
    installation_id        TEXT,
    service_instance_id    TEXT,
    turn_id                TEXT,
    identity_status        TEXT NOT NULL DEFAULT 'legacy_unattributed'
        CHECK (identity_status IN ('observed', 'legacy_unattributed'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_recall_log_smoke_source
    ON recall_log_smoke(source_recall_id) WHERE source_recall_id IS NOT NULL;
CREATE TABLE IF NOT EXISTS smoke_recall_isolation_ledger (
    migration_id      TEXT PRIMARY KEY,
    applied_at        TEXT NOT NULL,
    expected_count    INTEGER NOT NULL,
    moved_count       INTEGER NOT NULL,
    target_digest     TEXT NOT NULL,
    non_target_count  INTEGER NOT NULL,
    non_target_digest TEXT NOT NULL,
    isolation_reason  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS transform_log (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    ts           TEXT NOT NULL,
    verb         TEXT NOT NULL,      -- skel|compress|prep:<kind>|runc|curate
    scope        TEXT,
    before_chars INTEGER NOT NULL,   -- curate rows repurpose this as consolidated_count
    after_chars  INTEGER NOT NULL,   -- curate rows repurpose this as pruned_count
    meta         TEXT,               -- e.g. unit=tok for prep:compress rows
    event_uid            TEXT,
    installation_id      TEXT,
    service_instance_id  TEXT,
    client               TEXT NOT NULL DEFAULT 'legacy',
    session_id           TEXT,
    turn_id              TEXT,
    trace_id             TEXT,
    opportunity_uid      TEXT,
    identity_status      TEXT NOT NULL DEFAULT 'legacy_unattributed'
        CHECK (identity_status IN ('observed', 'legacy_unattributed'))
);
CREATE TABLE IF NOT EXISTS transform_opportunity_log (
    opportunity_uid      TEXT PRIMARY KEY,
    ts                   TEXT NOT NULL,
    verb                 TEXT NOT NULL CHECK (verb IN ('runc', 'skel', 'compress')),
    source               TEXT NOT NULL,
    reason_code          TEXT NOT NULL,
    outcome              TEXT NOT NULL DEFAULT 'recommended'
        CHECK (outcome IN ('recommended', 'used', 'error')),
    resolved_at          TEXT,
    resolved_event_uid   TEXT,
    installation_id      TEXT NOT NULL,
    service_instance_id  TEXT NOT NULL,
    client               TEXT NOT NULL,
    session_id           TEXT NOT NULL,
    turn_id              TEXT NOT NULL,
    trace_id             TEXT NOT NULL,
    identity_status      TEXT NOT NULL DEFAULT 'observed'
        CHECK (identity_status = 'observed')
);
CREATE INDEX IF NOT EXISTS idx_transform_opportunity_decision
    ON transform_opportunity_log(client, session_id, turn_id, trace_id);
CREATE INDEX IF NOT EXISTS idx_transform_opportunity_outcome
    ON transform_opportunity_log(verb, outcome, ts);
CREATE TABLE IF NOT EXISTS deletions (
    id         TEXT PRIMARY KEY,     -- memory id (scope-qualified) whose row was deleted
    deleted_at TEXT NOT NULL         -- ISO; lets cross-machine sync propagate deletions
);
CREATE TABLE IF NOT EXISTS memory_event_log (
    event_id   INTEGER PRIMARY KEY AUTOINCREMENT,
    ts         TEXT NOT NULL,
    event_kind TEXT NOT NULL,
    memory_id  TEXT,
    surface    TEXT NOT NULL DEFAULT 'unknown',
    session_id TEXT,
    trace_id   TEXT,
    scope_id   TEXT,
    quantity   INTEGER NOT NULL DEFAULT 1,
    meta       TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS memory_identity (
    memory_id           TEXT PRIMARY KEY,
    artifact_id         TEXT NOT NULL UNIQUE,
    origin_event_uid    TEXT NOT NULL UNIQUE,
    installation_id     TEXT,
    client              TEXT NOT NULL,
    session_id          TEXT NOT NULL,
    turn_id             TEXT NOT NULL,
    trace_id            TEXT NOT NULL,
    identity_status     TEXT NOT NULL CHECK (identity_status IN ('observed', 'legacy_unattributed')),
    created_at          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_memory_event_memory_ts
    ON memory_event_log(memory_id, ts);
CREATE INDEX IF NOT EXISTS idx_memory_event_surface_ts
    ON memory_event_log(surface, ts);
CREATE TABLE IF NOT EXISTS context_policy_assignment (
    session_id     TEXT NOT NULL,
    surface        TEXT NOT NULL,
    policy_version TEXT NOT NULL,
    cohort         TEXT NOT NULL CHECK (cohort IN ('control', 'candidate')),
    assigned_at    TEXT NOT NULL,
    task_class     TEXT NOT NULL DEFAULT 'unknown',
    PRIMARY KEY (session_id, surface, policy_version)
);
CREATE INDEX IF NOT EXISTS idx_context_policy_cohort_time
    ON context_policy_assignment(policy_version, cohort, assigned_at);
CREATE TABLE IF NOT EXISTS context_feedback (
    trace_id       TEXT NOT NULL,
    candidate_id   TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    outcome        TEXT NOT NULL,        -- used | ignored | contradicted
    verified       INTEGER NOT NULL,     -- 1 = observed action / cited verdict; 0 = advisory only
    verdict_ref    TEXT,
    scope_id       TEXT NOT NULL DEFAULT 'global',
    ts             TEXT NOT NULL,
    PRIMARY KEY (trace_id, candidate_id)
);
CREATE INDEX IF NOT EXISTS idx_context_feedback_candidate
    ON context_feedback(candidate_id);
CREATE TABLE IF NOT EXISTS links (
    src_id   TEXT NOT NULL,
    dst_slug TEXT NOT NULL,
    PRIMARY KEY (src_id, dst_slug)
);
CREATE INDEX IF NOT EXISTS idx_links_dst ON links(dst_slug);
CREATE TABLE IF NOT EXISTS skills (
    name        TEXT PRIMARY KEY,
    description TEXT NOT NULL DEFAULT '',
    body        TEXT NOT NULL,
    body_sha256 TEXT NOT NULL,
    resources   TEXT NOT NULL DEFAULT '[]',
    updated_at  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS memory_quarantine (
    id             TEXT PRIMARY KEY,
    tier           TEXT NOT NULL,
    content        TEXT NOT NULL,
    keywords       TEXT NOT NULL,
    score          REAL NOT NULL,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL DEFAULT '',
    access_count   INTEGER NOT NULL,
    embedding      BLOB,
    embedding_q    BLOB,
    scope_id       TEXT NOT NULL DEFAULT 'global',
    inject_count   INTEGER NOT NULL DEFAULT 0,
    content_hash   TEXT,
    embed_model    TEXT,
    source_ids     TEXT NOT NULL DEFAULT '[]',
    authority      TEXT NOT NULL DEFAULT 'A0',
    influence_class TEXT NOT NULL DEFAULT 'unknown',
    quarantined_at TEXT NOT NULL,
    reason         TEXT NOT NULL
);
";

/// Current durable Crypt schema contract. External migration tests must not
/// duplicate this value, because a promoted schema version changes atomically.
pub const LATEST_SCHEMA_VERSION: i64 = 23;
const SMOKE_ISOLATION_MIGRATION_ID: &str = "rc-2.3-smoke-spotcheck-production-v1";
const SMOKE_ISOLATION_REASON: &str = "legacy_production_smoke_spotcheck";
const SMOKE_RECALL_PREDICATE: &str =
    "lower(trim(client))='smoke-spotcheck' AND traffic_class='production'";
const RECALL_DIGEST_COLUMNS: &str =
    "id, ts, scope, query_chars, hit_count, full_chars, injected_chars, source, query_preview, \
     client, session_id, cwd_scope, hook_event, trace_id, client_visibility, traffic_class, \
     candidate_hits, admitted_hits";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecallLogSink {
    Production,
    Smoke,
}

#[derive(Debug, serde::Serialize)]
pub struct SmokeIsolationReport {
    pub schema_version: i64,
    pub mode: &'static str,
    pub expected_count: usize,
    pub matched_count: usize,
    pub moved_count: usize,
    pub production_smoke_rows: usize,
    pub target_digest: String,
    pub non_target_count: usize,
    pub non_target_digest: String,
    pub already_applied: bool,
}

fn digest_query_rows(conn: &Connection, sql: &str) -> rusqlite::Result<String> {
    let mut statement = conn.prepare(sql)?;
    let column_count = statement.column_count();
    let mut rows = statement.query([])?;
    let mut hasher = Sha256::new();
    while let Some(row) = rows.next()? {
        for index in 0..column_count {
            use rusqlite::types::ValueRef;
            match row.get_ref(index)? {
                ValueRef::Null => hasher.update([0]),
                ValueRef::Integer(value) => {
                    hasher.update([1]);
                    hasher.update(value.to_be_bytes());
                }
                ValueRef::Real(value) => {
                    hasher.update([2]);
                    hasher.update(value.to_bits().to_be_bytes());
                }
                ValueRef::Text(value) => {
                    hasher.update([3]);
                    hasher.update((value.len() as u64).to_be_bytes());
                    hasher.update(value);
                }
                ValueRef::Blob(value) => {
                    hasher.update([4]);
                    hasher.update((value.len() as u64).to_be_bytes());
                    hasher.update(value);
                }
            }
        }
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn opaque_legacy_identity(prefix: &str, memory_id: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(memory_id.as_bytes()));
    format!("{prefix}{}", &digest[..32])
}

fn backfill_legacy_memory_identity(conn: &mut Connection) -> rusqlite::Result<()> {
    let missing = {
        let mut statement = conn.prepare(
            "SELECT m.id, m.created_at
             FROM memories m
             LEFT JOIN memory_identity i ON i.memory_id=m.id
             WHERE i.memory_id IS NULL
             ORDER BY m.id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    if missing.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for (memory_id, created_at) in missing {
        let digest = format!("{:x}", Sha256::digest(memory_id.as_bytes()));
        tx.execute(
            "INSERT OR IGNORE INTO memory_identity
             (memory_id, artifact_id, origin_event_uid, installation_id, client,
              session_id, turn_id, trace_id, identity_status, created_at)
             VALUES (?1, ?2, ?3, NULL, 'legacy', ?4, ?5, ?6,
                     'legacy_unattributed', ?7)",
            rusqlite::params![
                memory_id,
                format!("memory.{}", &digest[..32]),
                opaque_legacy_identity("legacy-memory.", &memory_id),
                opaque_legacy_identity("session-", &memory_id),
                opaque_legacy_identity("turn-", &memory_id),
                opaque_legacy_identity("trace-", &memory_id),
                created_at,
            ],
        )?;
    }
    tx.commit()
}

fn backfill_legacy_transform_identity(conn: &mut Connection) -> rusqlite::Result<()> {
    let missing = {
        let mut statement = conn.prepare(
            "SELECT id FROM transform_log
             WHERE event_uid IS NULL OR session_id IS NULL OR turn_id IS NULL OR trace_id IS NULL
             ORDER BY id",
        )?;
        let rows = statement
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    if missing.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for id in missing {
        let seed = format!("transform:{id}");
        tx.execute(
            "UPDATE transform_log
             SET event_uid=COALESCE(event_uid, ?2),
                 client=CASE WHEN trim(client)='' THEN 'legacy' ELSE client END,
                 session_id=COALESCE(session_id, ?3),
                 turn_id=COALESCE(turn_id, ?4),
                 trace_id=COALESCE(trace_id, ?5),
                 identity_status='legacy_unattributed'
             WHERE id=?1",
            rusqlite::params![
                id,
                opaque_legacy_identity("legacy-transform.", &seed),
                opaque_legacy_identity("session-", &seed),
                opaque_legacy_identity("turn-", &seed),
                opaque_legacy_identity("trace-", &seed),
            ],
        )?;
    }
    tx.commit()
}

fn backfill_legacy_recall_identity(conn: &mut Connection) -> rusqlite::Result<()> {
    for (table, event_prefix) in [
        ("recall_log", "legacy-recall."),
        ("recall_log_smoke", "legacy-smoke-recall."),
    ] {
        let missing = {
            let mut statement = conn.prepare(&format!(
                "SELECT id FROM {table}
                 WHERE event_uid IS NULL OR session_id IS NULL OR turn_id IS NULL OR trace_id IS NULL
                 ORDER BY id"
            ))?;
            let rows = statement
                .query_map([], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        if missing.is_empty() {
            continue;
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for id in missing {
            let seed = format!("{table}:{id}");
            tx.execute(
                &format!(
                    "UPDATE {table}
                     SET event_uid=COALESCE(event_uid, ?2),
                         session_id=COALESCE(session_id, ?3),
                         turn_id=COALESCE(turn_id, ?4),
                         trace_id=COALESCE(trace_id, ?5),
                         identity_status='legacy_unattributed'
                     WHERE id=?1"
                ),
                rusqlite::params![
                    id,
                    opaque_legacy_identity(event_prefix, &seed),
                    opaque_legacy_identity("session-", &seed),
                    opaque_legacy_identity("turn-", &seed),
                    opaque_legacy_identity("trace-", &seed),
                ],
            )?;
        }
        tx.commit()?;
    }
    Ok(())
}

fn transform_event_uid() -> Option<String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).ok()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Some(format!(
        "transform.{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    ))
}

/// Inspect the frozen RC-2.3 predicate through a read-only SQLite handle. This path deliberately
/// does not call [`MemDb::open`], run migrations, switch journal mode, or create the v11+ sink.
pub fn inspect_smoke_recalls<P: AsRef<Path>>(
    path: P,
    expected: usize,
) -> Result<SmokeIsolationReport, String> {
    let expected_i64 = i64::try_from(expected).map_err(|_| "expected count too large")?;
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    let schema_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if !matches!(schema_version, 10..=12) {
        return Err(format!(
            "smoke isolation inspection requires schema v10, v11, or v12, found v{schema_version}"
        ));
    }
    let matched_count: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM recall_log WHERE {SMOKE_RECALL_PREDICATE}"),
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;

    if schema_version >= 11 {
        let ledger = conn
            .query_row(
                "SELECT expected_count, moved_count, target_digest, non_target_count,
                        non_target_digest
                   FROM smoke_recall_isolation_ledger WHERE migration_id=?1",
                [SMOKE_ISOLATION_MIGRATION_ID],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some((ledger_expected, moved, target_digest, non_target_count, non_target_digest)) =
            ledger
        {
            if ledger_expected != expected_i64 || matched_count != 0 {
                return Err(format!(
                    "isolation ledger mismatch: expected={ledger_expected}, requested={expected_i64}, production_smoke={matched_count}"
                ));
            }
            let copied_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM recall_log_smoke
                      WHERE isolation_reason='legacy_production_smoke_spotcheck'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            let copied_digest = digest_query_rows(
                &conn,
                "SELECT source_recall_id AS id, ts, scope, query_chars, hit_count, full_chars,
                        injected_chars, source, query_preview, client, session_id, cwd_scope,
                        hook_event, trace_id, client_visibility,
                        original_traffic_class AS traffic_class, candidate_hits, admitted_hits
                   FROM recall_log_smoke
                  WHERE isolation_reason='legacy_production_smoke_spotcheck'
                  ORDER BY source_recall_id",
            )
            .map_err(|error| error.to_string())?;
            if copied_count != moved || copied_digest != target_digest {
                return Err("isolation ledger does not match physically isolated rows".into());
            }
            return Ok(SmokeIsolationReport {
                schema_version,
                mode: "dry_run",
                expected_count: expected,
                matched_count: 0,
                moved_count: 0,
                production_smoke_rows: 0,
                target_digest,
                non_target_count: non_target_count as usize,
                non_target_digest,
                already_applied: true,
            });
        }
    }

    if matched_count != expected_i64 {
        return Err(format!(
            "expected exactly {expected_i64} production smoke rows, found {matched_count}"
        ));
    }
    let target_digest = digest_query_rows(
        &conn,
        &format!(
            "SELECT {RECALL_DIGEST_COLUMNS} FROM recall_log
              WHERE {SMOKE_RECALL_PREDICATE} ORDER BY id"
        ),
    )
    .map_err(|error| error.to_string())?;
    let non_target_count: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM recall_log WHERE NOT ({SMOKE_RECALL_PREDICATE})"),
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let non_target_digest = digest_query_rows(
        &conn,
        &format!(
            "SELECT {RECALL_DIGEST_COLUMNS} FROM recall_log
              WHERE NOT ({SMOKE_RECALL_PREDICATE}) ORDER BY id"
        ),
    )
    .map_err(|error| error.to_string())?;
    Ok(SmokeIsolationReport {
        schema_version,
        mode: "dry_run",
        expected_count: expected,
        matched_count: matched_count as usize,
        moved_count: 0,
        production_smoke_rows: matched_count as usize,
        target_digest,
        non_target_count: non_target_count as usize,
        non_target_digest,
        already_applied: false,
    })
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names.iter().any(|name| name == column))
}

fn add_column(
    conn: &Connection,
    table: &str,
    column: &str,
    declaration: &str,
) -> rusqlite::Result<()> {
    if !column_exists(conn, table, column)? {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {declaration}"),
            [],
        )?;
    }
    Ok(())
}

fn require_columns(conn: &Connection, table: &str, columns: &[&str]) -> rusqlite::Result<()> {
    for column in columns {
        if !column_exists(conn, table, column)? {
            return Err(rusqlite::Error::InvalidColumnName(format!(
                "{table}.{column} missing after migration"
            )));
        }
    }
    Ok(())
}

fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > LATEST_SCHEMA_VERSION {
        return Err(rusqlite::Error::InvalidQuery);
    }

    while version < LATEST_SCHEMA_VERSION {
        let next = version + 1;
        let tx = conn.transaction()?;
        match next {
            1 => {
                tx.execute_batch(SCHEMA)?;
                require_columns(
                    &tx,
                    "memories",
                    &[
                        "id",
                        "tier",
                        "content",
                        "keywords",
                        "score",
                        "created_at",
                        "access_count",
                    ],
                )?;
                require_columns(&tx, "recall_log", &["id", "ts", "query_chars", "hit_count"])?;
            }
            2 => {
                add_column(&tx, "memories", "embedding", "BLOB")?;
                add_column(&tx, "memories", "embedding_q", "BLOB")?;
                add_column(
                    &tx,
                    "memories",
                    "scope_id",
                    "TEXT NOT NULL DEFAULT 'global'",
                )?;
                add_column(
                    &tx,
                    "memories",
                    "inject_count",
                    "INTEGER NOT NULL DEFAULT 0",
                )?;
                add_column(&tx, "memories", "content_hash", "TEXT")?;
                add_column(&tx, "memories", "embed_model", "TEXT")?;
            }
            3 => {
                add_column(&tx, "recall_log", "source", "TEXT NOT NULL DEFAULT 'serve'")?;
                add_column(&tx, "recall_log", "query_preview", "TEXT")?;
                add_column(
                    &tx,
                    "recall_log",
                    "client",
                    "TEXT NOT NULL DEFAULT 'unknown'",
                )?;
                add_column(&tx, "recall_log", "session_id", "TEXT")?;
                add_column(&tx, "recall_log", "cwd_scope", "TEXT")?;
                add_column(&tx, "recall_log", "hook_event", "TEXT")?;
                add_column(&tx, "recall_log", "trace_id", "TEXT")?;
                add_column(
                    &tx,
                    "recall_log",
                    "client_visibility",
                    "TEXT NOT NULL DEFAULT 'all'",
                )?;
            }
            4 => {
                add_column(&tx, "memories", "source_ids", "TEXT NOT NULL DEFAULT '[]'")?;
                add_column(
                    &tx,
                    "recall_log",
                    "traffic_class",
                    "TEXT NOT NULL DEFAULT 'production'",
                )?;
                add_column(
                    &tx,
                    "recall_log",
                    "candidate_hits",
                    "TEXT NOT NULL DEFAULT '[]'",
                )?;
                add_column(
                    &tx,
                    "recall_log",
                    "admitted_hits",
                    "TEXT NOT NULL DEFAULT '[]'",
                )?;
            }
            5 => {
                add_column(&tx, "memories", "updated_at", "TEXT NOT NULL DEFAULT ''")?;
                tx.execute(
                    "UPDATE memories SET updated_at = created_at WHERE updated_at = ''",
                    [],
                )?;
            }
            6 => {
                tx.execute_batch(
                    "CREATE TABLE IF NOT EXISTS memory_event_log (
                        event_id   INTEGER PRIMARY KEY AUTOINCREMENT,
                        ts         TEXT NOT NULL,
                        event_kind TEXT NOT NULL,
                        memory_id  TEXT,
                        surface    TEXT NOT NULL DEFAULT 'unknown',
                        session_id TEXT,
                        trace_id   TEXT,
                        scope_id   TEXT,
                        quantity   INTEGER NOT NULL DEFAULT 1,
                        meta       TEXT NOT NULL DEFAULT '{}'
                    );
                    CREATE INDEX IF NOT EXISTS idx_memory_event_memory_ts
                        ON memory_event_log(memory_id, ts);
                    CREATE INDEX IF NOT EXISTS idx_memory_event_surface_ts
                        ON memory_event_log(surface, ts);
                    CREATE TABLE IF NOT EXISTS context_policy_assignment (
                        session_id     TEXT NOT NULL,
                        surface        TEXT NOT NULL,
                        policy_version TEXT NOT NULL,
                        cohort         TEXT NOT NULL CHECK (cohort IN ('control', 'candidate')),
                        assigned_at    TEXT NOT NULL,
                        task_class     TEXT NOT NULL DEFAULT 'unknown',
                        PRIMARY KEY (session_id, surface, policy_version)
                    );
                    CREATE INDEX IF NOT EXISTS idx_context_policy_cohort_time
                        ON context_policy_assignment(policy_version, cohort, assigned_at);",
                )?;
                require_columns(
                    &tx,
                    "memory_event_log",
                    &[
                        "event_id",
                        "ts",
                        "event_kind",
                        "memory_id",
                        "surface",
                        "session_id",
                        "trace_id",
                        "scope_id",
                        "quantity",
                        "meta",
                    ],
                )?;
                require_columns(
                    &tx,
                    "context_policy_assignment",
                    &[
                        "session_id",
                        "surface",
                        "policy_version",
                        "cohort",
                        "assigned_at",
                        "task_class",
                    ],
                )?;
            }
            7 => {
                tx.execute_batch(
                    "CREATE TABLE IF NOT EXISTS context_feedback (
                        trace_id       TEXT NOT NULL,
                        candidate_id   TEXT NOT NULL,
                        content_sha256 TEXT NOT NULL,
                        outcome        TEXT NOT NULL,
                        verified       INTEGER NOT NULL,
                        verdict_ref    TEXT,
                        scope_id       TEXT NOT NULL DEFAULT 'global',
                        ts             TEXT NOT NULL,
                        PRIMARY KEY (trace_id, candidate_id)
                    );
                    CREATE INDEX IF NOT EXISTS idx_context_feedback_candidate
                        ON context_feedback(candidate_id);",
                )?;
                require_columns(
                    &tx,
                    "context_feedback",
                    &[
                        "trace_id",
                        "candidate_id",
                        "content_sha256",
                        "outcome",
                        "verified",
                        "verdict_ref",
                        "scope_id",
                        "ts",
                    ],
                )?;
            }
            8 => {
                tx.execute_batch(
                    "CREATE TABLE IF NOT EXISTS links (
                        src_id   TEXT NOT NULL,
                        dst_slug TEXT NOT NULL,
                        PRIMARY KEY (src_id, dst_slug)
                    );
                    CREATE INDEX IF NOT EXISTS idx_links_dst ON links(dst_slug);",
                )?;
                require_columns(&tx, "links", &["src_id", "dst_slug"])?;
            }
            9 => {
                tx.execute_batch(
                    "CREATE TABLE IF NOT EXISTS skills (
                        name        TEXT PRIMARY KEY,
                        description TEXT NOT NULL DEFAULT '',
                        body        TEXT NOT NULL,
                        body_sha256 TEXT NOT NULL,
                        resources   TEXT NOT NULL DEFAULT '[]',
                        updated_at  TEXT NOT NULL
                    );",
                )?;
                require_columns(&tx, "skills", &["name", "body", "body_sha256"])?;
            }
            10 => {
                tx.execute_batch(
                    "CREATE TABLE IF NOT EXISTS memory_quarantine (
                        id             TEXT PRIMARY KEY,
                        tier           TEXT NOT NULL,
                        content        TEXT NOT NULL,
                        keywords       TEXT NOT NULL,
                        score          REAL NOT NULL,
                        created_at     TEXT NOT NULL,
                        updated_at     TEXT NOT NULL DEFAULT '',
                        access_count   INTEGER NOT NULL,
                        embedding      BLOB,
                        embedding_q    BLOB,
                        scope_id       TEXT NOT NULL DEFAULT 'global',
                        inject_count   INTEGER NOT NULL DEFAULT 0,
                        content_hash   TEXT,
                        embed_model    TEXT,
                        source_ids     TEXT NOT NULL DEFAULT '[]',
                        quarantined_at TEXT NOT NULL,
                        reason         TEXT NOT NULL
                    );",
                )?;
                require_columns(
                    &tx,
                    "memory_quarantine",
                    &[
                        "id",
                        "tier",
                        "content",
                        "keywords",
                        "score",
                        "created_at",
                        "updated_at",
                        "access_count",
                        "embedding",
                        "embedding_q",
                        "scope_id",
                        "inject_count",
                        "content_hash",
                        "embed_model",
                        "source_ids",
                        "quarantined_at",
                        "reason",
                    ],
                )?;
            }
            11 => {
                tx.execute_batch(
                    "CREATE TABLE IF NOT EXISTS recall_log_smoke (
                        id                     INTEGER PRIMARY KEY AUTOINCREMENT,
                        ts                     TEXT NOT NULL,
                        scope                  TEXT,
                        query_chars            INTEGER NOT NULL,
                        hit_count              INTEGER NOT NULL,
                        full_chars             INTEGER NOT NULL,
                        injected_chars         INTEGER NOT NULL,
                        source                 TEXT NOT NULL DEFAULT 'serve',
                        query_preview          TEXT,
                        client                 TEXT NOT NULL DEFAULT 'unknown',
                        session_id             TEXT,
                        cwd_scope              TEXT,
                        hook_event             TEXT,
                        trace_id               TEXT,
                        client_visibility      TEXT NOT NULL DEFAULT 'all',
                        traffic_class          TEXT NOT NULL DEFAULT 'smoke'
                            CHECK (traffic_class != 'production'),
                        candidate_hits         TEXT NOT NULL DEFAULT '[]',
                        admitted_hits          TEXT NOT NULL DEFAULT '[]',
                        source_recall_id       INTEGER,
                        original_traffic_class TEXT NOT NULL,
                        isolated_at            TEXT NOT NULL,
                        isolation_reason       TEXT NOT NULL
                    );
                    CREATE UNIQUE INDEX IF NOT EXISTS idx_recall_log_smoke_source
                        ON recall_log_smoke(source_recall_id)
                        WHERE source_recall_id IS NOT NULL;
                    CREATE TABLE IF NOT EXISTS smoke_recall_isolation_ledger (
                        migration_id      TEXT PRIMARY KEY,
                        applied_at        TEXT NOT NULL,
                        expected_count    INTEGER NOT NULL,
                        moved_count       INTEGER NOT NULL,
                        target_digest     TEXT NOT NULL,
                        non_target_count  INTEGER NOT NULL,
                        non_target_digest TEXT NOT NULL,
                        isolation_reason  TEXT NOT NULL
                    );",
                )?;
                require_columns(
                    &tx,
                    "recall_log_smoke",
                    &[
                        "id",
                        "ts",
                        "scope",
                        "query_chars",
                        "hit_count",
                        "full_chars",
                        "injected_chars",
                        "source",
                        "query_preview",
                        "client",
                        "session_id",
                        "cwd_scope",
                        "hook_event",
                        "trace_id",
                        "client_visibility",
                        "traffic_class",
                        "candidate_hits",
                        "admitted_hits",
                        "source_recall_id",
                        "original_traffic_class",
                        "isolated_at",
                        "isolation_reason",
                    ],
                )?;
                require_columns(
                    &tx,
                    "smoke_recall_isolation_ledger",
                    &[
                        "migration_id",
                        "applied_at",
                        "expected_count",
                        "moved_count",
                        "target_digest",
                        "non_target_count",
                        "non_target_digest",
                        "isolation_reason",
                    ],
                )?;
            }
            12 => {
                tx.execute_batch(crate::context_telemetry::CONTEXT_LEDGER_SCHEMA)?;
                add_column(
                    &tx,
                    "memories",
                    "artifact_family",
                    "TEXT NOT NULL DEFAULT 'memory'",
                )?;
                add_column(
                    &tx,
                    "memories",
                    "producer",
                    "TEXT NOT NULL DEFAULT 'manual'",
                )?;
                add_column(
                    &tx,
                    "memories",
                    "record_type",
                    "TEXT NOT NULL DEFAULT 'memory'",
                )?;
                tx.execute_batch(
                    "CREATE TABLE IF NOT EXISTS memory_batch_receipt (
                         batch_id       TEXT PRIMARY KEY,
                         request_sha256 TEXT NOT NULL,
                         item_count     INTEGER NOT NULL,
                         created_at     TEXT NOT NULL
                     );
                     CREATE TABLE IF NOT EXISTS memory_batch_item_receipt (
                         batch_id       TEXT NOT NULL,
                         ordinal        INTEGER NOT NULL,
                         item_id        TEXT NOT NULL,
                         memory_id      TEXT NOT NULL,
                         disposition    TEXT NOT NULL,
                         item_sha256    TEXT NOT NULL,
                         artifact_family TEXT NOT NULL,
                         producer       TEXT NOT NULL,
                         record_type    TEXT NOT NULL,
                         PRIMARY KEY (batch_id, item_id),
                         UNIQUE (batch_id, ordinal),
                         FOREIGN KEY (batch_id) REFERENCES memory_batch_receipt(batch_id)
                             ON DELETE RESTRICT
                     );",
                )?;
                require_columns(
                    &tx,
                    "memories",
                    &["artifact_family", "producer", "record_type"],
                )?;
                require_columns(
                    &tx,
                    "context_installation",
                    &[
                        "installation_id",
                        "created_at",
                        "first_observed_at",
                        "last_observed_at",
                        "installation_epoch",
                    ],
                )?;
                require_columns(
                    &tx,
                    "context_event_log",
                    &[
                        "seq",
                        "event_id",
                        "schema_version",
                        "ts",
                        "installation_id",
                        "service_instance_id",
                        "workspace_id",
                        "client",
                        "producer",
                        "session_id",
                        "trace_id",
                        "span_id",
                        "artifact_family",
                        "provider",
                        "phase",
                        "operation",
                        "status",
                        "canonical_sha256",
                    ],
                )?;
                require_columns(
                    &tx,
                    "context_event_measurement",
                    &["event_id", "name", "value_type", "unit"],
                )?;
                require_columns(
                    &tx,
                    "context_event_link",
                    &["event_id", "relation", "target_event_id"],
                )?;
                require_columns(
                    &tx,
                    "context_ingest_checkpoint",
                    &[
                        "source",
                        "watermark",
                        "source_count",
                        "ingested_count",
                        "rejected_count",
                        "checksum_sha256",
                    ],
                )?;
                require_columns(
                    &tx,
                    "memory_batch_receipt",
                    &["batch_id", "request_sha256", "item_count", "created_at"],
                )?;
                require_columns(
                    &tx,
                    "memory_batch_item_receipt",
                    &[
                        "batch_id",
                        "ordinal",
                        "item_id",
                        "memory_id",
                        "disposition",
                        "item_sha256",
                        "artifact_family",
                        "producer",
                        "record_type",
                    ],
                )?;
            }
            13 => {
                add_column(&tx, "context_event_log", "policy_activation_sha256", "TEXT")?;
                require_columns(&tx, "context_event_log", &["policy_activation_sha256"])?;
            }
            14 => {
                add_column(&tx, "context_event_log", "release_generation", "TEXT")?;
                require_columns(&tx, "context_event_log", &["release_generation"])?;
            }
            15 => {
                add_column(&tx, "memory_event_log", "event_uid", "TEXT")?;
                add_column(&tx, "memory_event_log", "installation_id", "TEXT")?;
                add_column(&tx, "memory_event_log", "client", "TEXT")?;
                add_column(&tx, "memory_event_log", "turn_id", "TEXT")?;
                add_column(&tx, "memory_event_log", "artifact_id", "TEXT")?;
                add_column(
                    &tx,
                    "memory_event_log",
                    "identity_status",
                    "TEXT NOT NULL DEFAULT 'legacy_unattributed'",
                )?;
                tx.execute_batch(
                    "CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_event_uid
                         ON memory_event_log(event_uid) WHERE event_uid IS NOT NULL;
                     CREATE INDEX IF NOT EXISTS idx_memory_event_decision
                         ON memory_event_log(client, session_id, turn_id, trace_id);
                     CREATE TABLE IF NOT EXISTS memory_identity (
                         memory_id           TEXT PRIMARY KEY,
                         artifact_id         TEXT NOT NULL UNIQUE,
                         origin_event_uid    TEXT NOT NULL UNIQUE,
                         installation_id     TEXT,
                         client              TEXT NOT NULL,
                         session_id          TEXT NOT NULL,
                         turn_id             TEXT NOT NULL,
                         trace_id            TEXT NOT NULL,
                         identity_status     TEXT NOT NULL CHECK (
                             identity_status IN ('observed', 'legacy_unattributed')
                         ),
                         created_at          TEXT NOT NULL
                     );",
                )?;
                require_columns(
                    &tx,
                    "memory_event_log",
                    &[
                        "event_uid",
                        "installation_id",
                        "client",
                        "turn_id",
                        "artifact_id",
                        "identity_status",
                    ],
                )?;
                require_columns(
                    &tx,
                    "memory_identity",
                    &[
                        "memory_id",
                        "artifact_id",
                        "origin_event_uid",
                        "installation_id",
                        "client",
                        "session_id",
                        "turn_id",
                        "trace_id",
                        "identity_status",
                        "created_at",
                    ],
                )?;
            }
            16 => {
                tx.execute_batch(
                    "CREATE TABLE IF NOT EXISTS transform_log (
                         id INTEGER PRIMARY KEY AUTOINCREMENT,
                         ts TEXT NOT NULL,
                         verb TEXT NOT NULL,
                         scope TEXT,
                         before_chars INTEGER NOT NULL,
                         after_chars INTEGER NOT NULL,
                         meta TEXT
                     );",
                )?;
                add_column(&tx, "transform_log", "event_uid", "TEXT")?;
                add_column(&tx, "transform_log", "installation_id", "TEXT")?;
                add_column(&tx, "transform_log", "service_instance_id", "TEXT")?;
                add_column(
                    &tx,
                    "transform_log",
                    "client",
                    "TEXT NOT NULL DEFAULT 'legacy'",
                )?;
                add_column(&tx, "transform_log", "session_id", "TEXT")?;
                add_column(&tx, "transform_log", "turn_id", "TEXT")?;
                add_column(&tx, "transform_log", "trace_id", "TEXT")?;
                add_column(
                    &tx,
                    "transform_log",
                    "identity_status",
                    "TEXT NOT NULL DEFAULT 'legacy_unattributed'",
                )?;
                tx.execute_batch(
                    "CREATE UNIQUE INDEX IF NOT EXISTS idx_transform_event_uid
                         ON transform_log(event_uid) WHERE event_uid IS NOT NULL;
                     CREATE INDEX IF NOT EXISTS idx_transform_decision
                         ON transform_log(client, session_id, turn_id, trace_id);",
                )?;
                require_columns(
                    &tx,
                    "transform_log",
                    &[
                        "event_uid",
                        "installation_id",
                        "service_instance_id",
                        "client",
                        "session_id",
                        "turn_id",
                        "trace_id",
                        "identity_status",
                    ],
                )?;
            }
            17 => {
                tx.execute_batch(
                    "CREATE TABLE IF NOT EXISTS recall_log (
                         id INTEGER PRIMARY KEY AUTOINCREMENT,
                         ts TEXT NOT NULL,
                         scope TEXT,
                         query_chars INTEGER NOT NULL,
                         hit_count INTEGER NOT NULL,
                         full_chars INTEGER NOT NULL,
                         injected_chars INTEGER NOT NULL,
                         source TEXT NOT NULL DEFAULT 'serve',
                         query_preview TEXT,
                         client TEXT NOT NULL DEFAULT 'unknown',
                         session_id TEXT,
                         cwd_scope TEXT,
                         hook_event TEXT,
                         trace_id TEXT,
                         client_visibility TEXT NOT NULL DEFAULT 'all',
                         traffic_class TEXT NOT NULL DEFAULT 'production',
                         candidate_hits TEXT NOT NULL DEFAULT '[]',
                         admitted_hits TEXT NOT NULL DEFAULT '[]'
                     );",
                )?;
                for table in ["recall_log", "recall_log_smoke"] {
                    add_column(&tx, table, "event_uid", "TEXT")?;
                    add_column(&tx, table, "installation_id", "TEXT")?;
                    add_column(&tx, table, "service_instance_id", "TEXT")?;
                    add_column(&tx, table, "turn_id", "TEXT")?;
                    add_column(
                        &tx,
                        table,
                        "identity_status",
                        "TEXT NOT NULL DEFAULT 'legacy_unattributed'",
                    )?;
                    require_columns(
                        &tx,
                        table,
                        &[
                            "event_uid",
                            "installation_id",
                            "service_instance_id",
                            "turn_id",
                            "identity_status",
                        ],
                    )?;
                }
                tx.execute_batch(
                    "CREATE UNIQUE INDEX IF NOT EXISTS idx_recall_event_uid
                         ON recall_log(event_uid) WHERE event_uid IS NOT NULL;
                     CREATE INDEX IF NOT EXISTS idx_recall_decision
                         ON recall_log(client, session_id, turn_id, trace_id);
                     CREATE UNIQUE INDEX IF NOT EXISTS idx_smoke_recall_event_uid
                         ON recall_log_smoke(event_uid) WHERE event_uid IS NOT NULL;
                     CREATE INDEX IF NOT EXISTS idx_smoke_recall_decision
                         ON recall_log_smoke(client, session_id, turn_id, trace_id);",
                )?;
            }
            18 => {
                add_column(&tx, "transform_log", "opportunity_uid", "TEXT")?;
                tx.execute_batch(
                    "CREATE TABLE IF NOT EXISTS transform_opportunity_log (
                         opportunity_uid TEXT PRIMARY KEY,
                         ts TEXT NOT NULL,
                         verb TEXT NOT NULL CHECK (verb IN ('runc', 'skel', 'compress')),
                         source TEXT NOT NULL,
                         reason_code TEXT NOT NULL,
                         outcome TEXT NOT NULL DEFAULT 'recommended'
                             CHECK (outcome IN ('recommended', 'used', 'error')),
                         resolved_at TEXT,
                         resolved_event_uid TEXT,
                         installation_id TEXT NOT NULL,
                         service_instance_id TEXT NOT NULL,
                         client TEXT NOT NULL,
                         session_id TEXT NOT NULL,
                         turn_id TEXT NOT NULL,
                         trace_id TEXT NOT NULL,
                         identity_status TEXT NOT NULL DEFAULT 'observed'
                             CHECK (identity_status = 'observed')
                     );
                     CREATE INDEX IF NOT EXISTS idx_transform_opportunity_decision
                         ON transform_opportunity_log(client, session_id, turn_id, trace_id);
                     CREATE INDEX IF NOT EXISTS idx_transform_opportunity_outcome
                         ON transform_opportunity_log(verb, outcome, ts);
                     CREATE INDEX IF NOT EXISTS idx_transform_opportunity_join
                         ON transform_log(opportunity_uid)
                         WHERE opportunity_uid IS NOT NULL;",
                )?;
                require_columns(&tx, "transform_log", &["opportunity_uid"])?;
                require_columns(
                    &tx,
                    "transform_opportunity_log",
                    &[
                        "opportunity_uid",
                        "ts",
                        "verb",
                        "source",
                        "reason_code",
                        "outcome",
                        "resolved_at",
                        "resolved_event_uid",
                        "installation_id",
                        "service_instance_id",
                        "client",
                        "session_id",
                        "turn_id",
                        "trace_id",
                        "identity_status",
                    ],
                )?;
            }
            19 => {
                add_column(&tx, "memories", "authority", "TEXT NOT NULL DEFAULT 'A2'")?;
                add_column(
                    &tx,
                    "memories",
                    "influence_class",
                    "TEXT NOT NULL DEFAULT 'reference'",
                )?;
                add_column(
                    &tx,
                    "memory_quarantine",
                    "authority",
                    "TEXT NOT NULL DEFAULT 'A0'",
                )?;
                add_column(
                    &tx,
                    "memory_quarantine",
                    "influence_class",
                    "TEXT NOT NULL DEFAULT 'unknown'",
                )?;
                require_columns(&tx, "memories", &["authority", "influence_class"])?;
                require_columns(&tx, "memory_quarantine", &["authority", "influence_class"])?;
            }
            20 => {
                for table in ["memories", "memory_quarantine"] {
                    add_column(
                        &tx,
                        table,
                        "lifecycle_state",
                        "TEXT NOT NULL DEFAULT 'active'",
                    )?;
                    add_column(&tx, table, "effective_from_ms", "INTEGER")?;
                    add_column(&tx, table, "effective_until_ms", "INTEGER")?;
                    add_column(&tx, table, "expires_at_ms", "INTEGER")?;
                    add_column(&tx, table, "review_after_ms", "INTEGER")?;
                    add_column(&tx, table, "superseded_by", "TEXT")?;
                    add_column(
                        &tx,
                        table,
                        "priority_class",
                        "TEXT NOT NULL DEFAULT 'normal'",
                    )?;
                    add_column(&tx, table, "confidence", "REAL")?;
                    add_column(&tx, table, "confidence_basis", "TEXT")?;
                    require_columns(
                        &tx,
                        table,
                        &[
                            "lifecycle_state",
                            "effective_from_ms",
                            "effective_until_ms",
                            "expires_at_ms",
                            "review_after_ms",
                            "superseded_by",
                            "priority_class",
                            "confidence",
                            "confidence_basis",
                        ],
                    )?;
                }
            }
            21 => {
                tx.execute_batch(
                    "CREATE TABLE IF NOT EXISTS membrane_event_outbox (
                         id INTEGER PRIMARY KEY AUTOINCREMENT,
                         batch_sha256 TEXT NOT NULL UNIQUE,
                         batch_json TEXT NOT NULL,
                         created_at TEXT NOT NULL
                     );",
                )?;
                require_columns(
                    &tx,
                    "membrane_event_outbox",
                    &["id", "batch_sha256", "batch_json", "created_at"],
                )?;
            }
            22 => {
                tx.execute_batch(crate::context_telemetry::CONTEXT_LEDGER_SCHEMA)?;
                let rows: Vec<(i64, String, String)> = tx
                    .prepare(
                        "SELECT seq,event_id,canonical_sha256 FROM context_event_log ORDER BY seq",
                    )?
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect::<rusqlite::Result<_>>()?;
                let mut prev = "0".to_string();
                for (ordinal, (seq, event_id, canonical)) in rows.iter().enumerate() {
                    let n = (ordinal % 256 + 1) as i64;
                    let segment_start = ordinal - (ordinal % 256);
                    let segment = format!("seg-{}", rows[segment_start].0);
                    let chain = crate::context_telemetry::chain_digest(
                        &segment, n, event_id, canonical, &prev,
                    );
                    tx.execute("INSERT OR IGNORE INTO context_event_integrity(seq,event_id,canonical_sha256,prev_chain_sha256,chain_sha256,segment_key,ordinal) VALUES (?1,?2,?3,?4,?5,?6,?7)", rusqlite::params![seq,event_id,canonical,prev,chain,segment,n])?;
                    tx.execute("INSERT INTO context_event_segment(segment_key,first_seq,last_seq,event_count,content_sha256,sealed_at) VALUES (?1,?2,?2,1,?3,CASE WHEN ?4=256 THEN ?5 END) ON CONFLICT(segment_key) DO UPDATE SET last_seq=excluded.last_seq,event_count=context_event_segment.event_count+1,content_sha256=excluded.content_sha256,sealed_at=CASE WHEN context_event_segment.event_count+1=256 THEN ?5 ELSE context_event_segment.sealed_at END", rusqlite::params![segment,seq,crate::context_telemetry::segment_digest(&segment,n,event_id,canonical,&prev),n,crate::time::now_iso()])?;
                    prev = chain;
                }
            }
            23 => {
                add_column(&tx, "context_feedback", "qualified_experiment_id", "TEXT")?;
                tx.execute_batch(
                    "CREATE TABLE learning_experiment (
                         experiment_id TEXT PRIMARY KEY,
                         request_sha256 TEXT NOT NULL,
                         policy_version TEXT NOT NULL,
                         policy_activation_sha256 TEXT NOT NULL,
                         task_class TEXT NOT NULL,
                         observed_since TEXT NOT NULL,
                         observed_through TEXT NOT NULL,
                         min_per_cohort INTEGER NOT NULL CHECK(min_per_cohort > 0),
                         min_effect_bps INTEGER NOT NULL CHECK(min_effect_bps BETWEEN -10000 AND 10000),
                         created_at TEXT NOT NULL
                     );
                     CREATE TABLE learning_update_target (
                         experiment_id TEXT NOT NULL,
                         ordinal INTEGER NOT NULL,
                         memory_id TEXT NOT NULL,
                         content_sha256 TEXT NOT NULL,
                         delta_micros INTEGER NOT NULL CHECK(delta_micros BETWEEN -1000000 AND 1000000),
                         PRIMARY KEY(experiment_id, ordinal),
                         UNIQUE(experiment_id, memory_id),
                         FOREIGN KEY(experiment_id) REFERENCES learning_experiment(experiment_id) ON DELETE RESTRICT
                     );
                     CREATE TABLE learning_promotion_receipt (
                         experiment_id TEXT PRIMARY KEY,
                         evidence_sha256 TEXT NOT NULL,
                         control_count INTEGER NOT NULL,
                         control_success INTEGER NOT NULL,
                         candidate_count INTEGER NOT NULL,
                         candidate_success INTEGER NOT NULL,
                         effect_bps INTEGER NOT NULL,
                         disposition TEXT NOT NULL CHECK(disposition IN ('qualified','rejected')),
                         reason_code TEXT NOT NULL,
                         applied_count INTEGER NOT NULL,
                         created_at TEXT NOT NULL,
                         FOREIGN KEY(experiment_id) REFERENCES learning_experiment(experiment_id) ON DELETE RESTRICT
                     );
                     CREATE INDEX idx_learning_experiment_policy ON learning_experiment(policy_version, task_class);
                     CREATE INDEX idx_learning_target_memory ON learning_update_target(memory_id);",
                )?;
            }
            _ => unreachable!(),
        }
        tx.pragma_update(None, "user_version", next)?;
        tx.commit()?;
        version = next;
    }
    Ok(())
}

/// Transactionally return a schema-v20 database to its v19-compatible shape.
///
/// This removes only v20 lifecycle columns; row contents and all v19 metadata remain intact.
/// Unknown newer schemas fail closed rather than risking a partial downgrade.
pub fn backout_v20_to_v19<P: AsRef<Path>>(path: P) -> rusqlite::Result<()> {
    let path = path.as_ref();
    backout_v23_to_v22(path)?;
    backout_v22_to_v21(path)?;
    backout_v21_to_v20(path)?;
    let mut conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA busy_timeout=5000;")?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version < 20 {
        return Ok(());
    }
    if version > 20 {
        return Err(rusqlite::Error::InvalidQuery);
    }

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for table in ["memories", "memory_quarantine"] {
        tx.execute_batch(&format!(
            "ALTER TABLE {table} DROP COLUMN lifecycle_state;
             ALTER TABLE {table} DROP COLUMN effective_from_ms;
             ALTER TABLE {table} DROP COLUMN effective_until_ms;
             ALTER TABLE {table} DROP COLUMN expires_at_ms;
             ALTER TABLE {table} DROP COLUMN review_after_ms;
             ALTER TABLE {table} DROP COLUMN superseded_by;
             ALTER TABLE {table} DROP COLUMN priority_class;
             ALTER TABLE {table} DROP COLUMN confidence;
             ALTER TABLE {table} DROP COLUMN confidence_basis;"
        ))?;
    }
    tx.pragma_update(None, "user_version", 19)?;
    tx.commit()
}

/// Remove only the causal-learning v23 tables and feedback qualification marker.
pub fn backout_v23_to_v22<P: AsRef<Path>>(path: P) -> rusqlite::Result<()> {
    let conn = Connection::open(path)?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version < 23 {
        return Ok(());
    }
    if version != 23 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    conn.execute_batch(
        "DROP TABLE learning_promotion_receipt;
         DROP TABLE learning_update_target;
         DROP TABLE learning_experiment;
         ALTER TABLE context_feedback DROP COLUMN qualified_experiment_id;
         PRAGMA user_version=22;",
    )
}

fn backout_v22_to_v21(path: &Path) -> rusqlite::Result<()> {
    let conn = Connection::open(path)?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version < 22 {
        return Ok(());
    }
    if version != 22 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    conn.execute_batch("PRAGMA foreign_keys=OFF; DROP TABLE IF EXISTS context_event_retention_receipt; DROP TABLE IF EXISTS context_event_segment; DROP TABLE IF EXISTS context_event_integrity; PRAGMA foreign_keys=ON; PRAGMA user_version=21;")?;
    Ok(())
}

fn backout_v21_to_v20(path: &Path) -> rusqlite::Result<()> {
    let mut conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA busy_timeout=5000;")?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version < 21 {
        return Ok(());
    }
    if version != 21 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    conn.execute_batch(crate::context_telemetry::CONTEXT_LEDGER_SCHEMA)?;
    conn.execute_batch(
        "DROP TABLE IF EXISTS context_event_retention_receipt;
         DROP TABLE IF EXISTS context_event_segment;
         DROP TABLE IF EXISTS context_event_integrity;",
    )?;
    let event_path = resolve_event_db_path(path);
    conn.execute(
        "ATTACH DATABASE ?1 AS membrane_events",
        [event_path.to_string_lossy().as_ref()],
    )?;
    let migration = (|| -> rusqlite::Result<()> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(
            "INSERT OR IGNORE INTO main.context_installation SELECT * FROM membrane_events.context_installation;
             INSERT OR IGNORE INTO main.context_event_log SELECT * FROM membrane_events.context_event_log;
             INSERT OR IGNORE INTO main.context_event_measurement SELECT * FROM membrane_events.context_event_measurement;
             INSERT OR IGNORE INTO main.context_event_link SELECT * FROM membrane_events.context_event_link;
             INSERT OR REPLACE INTO main.context_ingest_checkpoint SELECT * FROM membrane_events.context_ingest_checkpoint;
             DROP TABLE main.membrane_event_outbox;
             PRAGMA main.user_version = 20;",
        )?;
        tx.commit()
    })();
    let detach = conn.execute_batch("DETACH DATABASE membrane_events;");
    migration?;
    detach
}

/// Older rollback entrypoints chain through v20 first when necessary. This keeps their existing
/// lossless v19→… rollback contracts valid without accepting an unknown newer schema.
fn backout_v20_if_present(path: &Path) -> rusqlite::Result<()> {
    let conn = Connection::open(path)?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    drop(conn);
    match version {
        0..=19 => Ok(()),
        20 => backout_v20_to_v19(path),
        21 => {
            backout_v21_to_v20(path)?;
            backout_v20_to_v19(path)
        }
        22 => {
            backout_v22_to_v21(path)?;
            backout_v21_to_v20(path)?;
            backout_v20_to_v19(path)
        }
        23 => {
            backout_v23_to_v22(path)?;
            backout_v22_to_v21(path)?;
            backout_v21_to_v20(path)?;
            backout_v20_to_v19(path)
        }
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn backout_identity_metadata_to_v14(path: &Path) -> rusqlite::Result<()> {
    let mut conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA busy_timeout=5000;")?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if !(15..=19).contains(&version) {
        return Ok(());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if version >= 19 {
        tx.execute_batch(
            "ALTER TABLE memories DROP COLUMN authority;
             ALTER TABLE memories DROP COLUMN influence_class;
             ALTER TABLE memory_quarantine DROP COLUMN authority;
             ALTER TABLE memory_quarantine DROP COLUMN influence_class;",
        )?;
    }
    if version >= 18 {
        tx.execute_batch(
            "DROP INDEX IF EXISTS idx_transform_opportunity_decision;
             DROP INDEX IF EXISTS idx_transform_opportunity_outcome;
             DROP INDEX IF EXISTS idx_transform_opportunity_join;
             DROP TABLE IF EXISTS transform_opportunity_log;
             ALTER TABLE transform_log DROP COLUMN opportunity_uid;",
        )?;
    }
    if version >= 17 {
        tx.execute_batch(
            "DROP INDEX IF EXISTS idx_recall_event_uid;
             DROP INDEX IF EXISTS idx_recall_decision;
             DROP INDEX IF EXISTS idx_smoke_recall_event_uid;
             DROP INDEX IF EXISTS idx_smoke_recall_decision;
             ALTER TABLE recall_log DROP COLUMN event_uid;
             ALTER TABLE recall_log DROP COLUMN installation_id;
             ALTER TABLE recall_log DROP COLUMN service_instance_id;
             ALTER TABLE recall_log DROP COLUMN turn_id;
             ALTER TABLE recall_log DROP COLUMN identity_status;
             ALTER TABLE recall_log_smoke DROP COLUMN event_uid;
             ALTER TABLE recall_log_smoke DROP COLUMN installation_id;
             ALTER TABLE recall_log_smoke DROP COLUMN service_instance_id;
             ALTER TABLE recall_log_smoke DROP COLUMN turn_id;
             ALTER TABLE recall_log_smoke DROP COLUMN identity_status;",
        )?;
    }
    if version >= 16 {
        tx.execute_batch(
            "DROP INDEX IF EXISTS idx_transform_event_uid;
             DROP INDEX IF EXISTS idx_transform_decision;
             ALTER TABLE transform_log DROP COLUMN event_uid;
             ALTER TABLE transform_log DROP COLUMN installation_id;
             ALTER TABLE transform_log DROP COLUMN service_instance_id;
             ALTER TABLE transform_log DROP COLUMN client;
             ALTER TABLE transform_log DROP COLUMN session_id;
             ALTER TABLE transform_log DROP COLUMN turn_id;
             ALTER TABLE transform_log DROP COLUMN trace_id;
             ALTER TABLE transform_log DROP COLUMN identity_status;",
        )?;
    }
    tx.execute_batch(
        "DROP INDEX IF EXISTS idx_memory_event_uid;
         DROP INDEX IF EXISTS idx_memory_event_decision;
         DROP TABLE IF EXISTS memory_identity;
         ALTER TABLE memory_event_log DROP COLUMN event_uid;
         ALTER TABLE memory_event_log DROP COLUMN installation_id;
         ALTER TABLE memory_event_log DROP COLUMN client;
         ALTER TABLE memory_event_log DROP COLUMN turn_id;
         ALTER TABLE memory_event_log DROP COLUMN artifact_id;
         ALTER TABLE memory_event_log DROP COLUMN identity_status;
         PRAGMA user_version = 14;",
    )?;
    tx.commit()
}

/// Transactionally return a schema-v10 database to the last v9-compatible shape.
/// Quarantined rows are restored without overwriting any active row with the same id, then the
/// v10-only table is removed and `user_version` is reset. An id collision aborts the transaction
/// so neither copy is lost. This is the supported binary rollback path; manually changing
/// `user_version` would strand quarantined data.
pub fn backout_v10_to_v9<P: AsRef<Path>>(path: P) -> rusqlite::Result<usize> {
    let path = path.as_ref();
    backout_v20_if_present(path)?;
    backout_identity_metadata_to_v14(path)?;
    let mut conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA busy_timeout=5000;")?;
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if matches!(version, 13 | 14) {
        drop(conn);
        backout_v13_to_v12(path)?;
        conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;
        version = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    }
    if version == 12 {
        drop(conn);
        backout_v12_to_v11(path)?;
        conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;
        version = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    }
    if version == 11 {
        drop(conn);
        backout_v11_to_v10(path)?;
        conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;
        version = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    }
    if version == 9 {
        return Ok(0);
    }
    if version != 10 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let tx = conn.transaction()?;
    let restored = tx.execute(
        "INSERT INTO memories
            (id, tier, content, keywords, score, created_at, updated_at, access_count,
             embedding, embedding_q, scope_id, inject_count, content_hash, embed_model, source_ids)
         SELECT id, tier, content, keywords, score, created_at, updated_at, access_count,
                embedding, embedding_q, scope_id, inject_count, content_hash, embed_model, source_ids
           FROM memory_quarantine",
        [],
    )?;
    tx.execute_batch("DROP TABLE memory_quarantine; PRAGMA user_version = 9;")?;
    tx.commit()?;
    Ok(restored)
}

/// Transactionally return schema v11 to v10 without losing either isolated legacy rows or later
/// prospective smoke recalls. Legacy rows regain their original ids; a collision aborts the whole
/// backout. Prospective rows receive fresh ids and retain their original nonproduction class.
pub fn backout_v11_to_v10<P: AsRef<Path>>(path: P) -> rusqlite::Result<usize> {
    let path = path.as_ref();
    backout_v20_if_present(path)?;
    backout_identity_metadata_to_v14(path)?;
    let mut conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA busy_timeout=5000;")?;
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if matches!(version, 13 | 14) {
        drop(conn);
        backout_v13_to_v12(path)?;
        conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;
        version = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    }
    if version == 12 {
        drop(conn);
        backout_v12_to_v11(path)?;
        conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;
        version = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    }
    if version == 10 {
        return Ok(0);
    }
    if version != 11 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let smoke_rows: i64 = tx.query_row("SELECT COUNT(*) FROM recall_log_smoke", [], |row| {
        row.get(0)
    })?;
    let restored_legacy = tx.execute(
        "INSERT INTO recall_log
            (id, ts, scope, query_chars, hit_count, full_chars, injected_chars, source,
             query_preview, client, session_id, cwd_scope, hook_event, trace_id,
             client_visibility, traffic_class, candidate_hits, admitted_hits)
         SELECT source_recall_id, ts, scope, query_chars, hit_count, full_chars, injected_chars,
                source, query_preview, client, session_id, cwd_scope, hook_event, trace_id,
                client_visibility, original_traffic_class, candidate_hits, admitted_hits
           FROM recall_log_smoke
          WHERE source_recall_id IS NOT NULL
          ORDER BY source_recall_id",
        [],
    )?;
    let restored_prospective = tx.execute(
        "INSERT INTO recall_log
            (ts, scope, query_chars, hit_count, full_chars, injected_chars, source,
             query_preview, client, session_id, cwd_scope, hook_event, trace_id,
             client_visibility, traffic_class, candidate_hits, admitted_hits)
         SELECT ts, scope, query_chars, hit_count, full_chars, injected_chars, source,
                query_preview, client, session_id, cwd_scope, hook_event, trace_id,
                client_visibility, original_traffic_class, candidate_hits, admitted_hits
           FROM recall_log_smoke
          WHERE source_recall_id IS NULL
          ORDER BY id",
        [],
    )?;
    if restored_legacy + restored_prospective != smoke_rows as usize {
        return Err(rusqlite::Error::ExecuteReturnedResults);
    }
    tx.execute_batch(
        "DROP TABLE smoke_recall_isolation_ledger;
         DROP TABLE recall_log_smoke;
         PRAGMA user_version = 10;",
    )?;
    tx.commit()?;
    Ok(smoke_rows as usize)
}

/// Transactionally return schema v12 to v11. The canonical ledger is a new projection at this
/// boundary, so rollback removes only its v12 ledger/batch tables and leaves every legacy source
/// table and additive memory attribution columns intact. The return value is the number of
/// canonical envelopes removed.
pub fn backout_v12_to_v11<P: AsRef<Path>>(path: P) -> rusqlite::Result<usize> {
    let path = path.as_ref();
    backout_v20_if_present(path)?;
    backout_identity_metadata_to_v14(path)?;
    let mut conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA busy_timeout=5000;")?;
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if matches!(version, 13 | 14) {
        drop(conn);
        backout_v13_to_v12(path)?;
        conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;
        version = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    }
    if version == 11 {
        return Ok(0);
    }
    if version != 12 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let removed: i64 = tx.query_row("SELECT COUNT(*) FROM context_event_log", [], |row| {
        row.get(0)
    })?;
    tx.execute_batch(
        "DROP TABLE memory_batch_item_receipt;
         DROP TABLE memory_batch_receipt;
         DROP TABLE context_event_link;
         DROP TABLE context_event_measurement;
         DROP TABLE context_event_log;
         DROP TABLE context_ingest_checkpoint;
         DROP TABLE context_installation;
         PRAGMA user_version = 11;",
    )?;
    tx.commit()?;
    Ok(removed as usize)
}

/// Transactionally return schema v14 to v13 while preserving every canonical ledger row.
/// The activation receipt is additive attribution, so rollback removes only that nullable column.
pub fn backout_v13_to_v12<P: AsRef<Path>>(path: P) -> rusqlite::Result<usize> {
    let path = path.as_ref();
    backout_v20_if_present(path)?;
    backout_identity_metadata_to_v14(path)?;
    let mut conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA busy_timeout=5000;")?;
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 14 {
        drop(conn);
        backout_v14_to_v13(path)?;
        conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;
        version = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    }
    if version == 12 {
        return Ok(0);
    }
    if version != 13 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let preserved: i64 = tx.query_row("SELECT COUNT(*) FROM context_event_log", [], |row| {
        row.get(0)
    })?;
    tx.execute_batch(
        "ALTER TABLE context_event_log DROP COLUMN policy_activation_sha256;
         PRAGMA user_version = 12;",
    )?;
    tx.commit()?;
    Ok(preserved as usize)
}

/// Transactionally return schema v14 to v13 while preserving every canonical ledger row.
/// Release generation is additive attribution, so rollback removes only that nullable column.
pub fn backout_v14_to_v13<P: AsRef<Path>>(path: P) -> rusqlite::Result<usize> {
    let path = path.as_ref();
    backout_v20_if_present(path)?;
    backout_identity_metadata_to_v14(path)?;
    let mut conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA busy_timeout=5000;")?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 13 {
        return Ok(0);
    }
    if version != 14 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let preserved: i64 = tx.query_row("SELECT COUNT(*) FROM context_event_log", [], |row| {
        row.get(0)
    })?;
    tx.execute_batch(
        "ALTER TABLE context_event_log DROP COLUMN release_generation;
         PRAGMA user_version = 13;",
    )?;
    tx.commit()?;
    Ok(preserved as usize)
}

/// A cloneable handle to the memory database; clones share one connection via the inner mutex.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemDbProbe {
    Ok,
    Busy,
    Error,
}

impl MemDbProbe {
    pub fn status(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Busy => "busy",
            Self::Error => "error",
        }
    }
}

fn resolve_event_db_path(memory_path: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("MEMBRANE_EVENT_DB") {
        return PathBuf::from(path);
    }
    let stem = memory_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("crypt");
    memory_path.with_file_name(format!("{stem}.membrane-events.sqlite3"))
}

fn extract_event_ledger(
    memory_conn: &mut Connection,
    event_path: &Path,
) -> rusqlite::Result<Connection> {
    let event_conn = Connection::open(event_path)?;
    event_conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;",
    )?;
    event_conn.execute_batch(crate::context_telemetry::CONTEXT_LEDGER_SCHEMA)?;
    drop(event_conn);

    memory_conn.execute(
        "ATTACH DATABASE ?1 AS membrane_events",
        [event_path.to_string_lossy().as_ref()],
    )?;
    let migration = (|| -> rusqlite::Result<()> {
        let has_legacy: bool = memory_conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name='context_event_log')",
            [],
            |row| row.get(0),
        )?;
        if !has_legacy {
            return Ok(());
        }
        let tx = memory_conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(
            "INSERT OR IGNORE INTO membrane_events.context_installation SELECT * FROM main.context_installation;
             INSERT OR IGNORE INTO membrane_events.context_event_log SELECT * FROM main.context_event_log;
             INSERT OR IGNORE INTO membrane_events.context_event_integrity SELECT * FROM main.context_event_integrity;
             INSERT OR IGNORE INTO membrane_events.context_event_segment SELECT * FROM main.context_event_segment;
             INSERT OR IGNORE INTO membrane_events.context_event_retention_receipt SELECT * FROM main.context_event_retention_receipt;
             INSERT OR IGNORE INTO membrane_events.context_event_measurement SELECT * FROM main.context_event_measurement;
             INSERT OR IGNORE INTO membrane_events.context_event_link SELECT * FROM main.context_event_link;
             INSERT OR REPLACE INTO membrane_events.context_ingest_checkpoint SELECT * FROM main.context_ingest_checkpoint;",
        )?;
        let missing: i64 = tx.query_row(
            "SELECT COUNT(*) FROM main.context_event_log source
              LEFT JOIN membrane_events.context_event_log target USING(event_id)
             WHERE target.event_id IS NULL",
            [],
            |row| row.get(0),
        )?;
        if missing != 0 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        tx.execute_batch(
            "DROP TABLE main.context_event_retention_receipt;
             DROP TABLE main.context_event_segment;
             DROP TABLE main.context_event_integrity;
             DROP TABLE main.context_event_link;
             DROP TABLE main.context_event_measurement;
             DROP TABLE main.context_event_log;
             DROP TABLE main.context_installation;
             DROP TABLE main.context_ingest_checkpoint;",
        )?;
        tx.commit()
    })();
    let detach = memory_conn.execute_batch("DETACH DATABASE membrane_events;");
    migration?;
    detach?;

    let event_conn = Connection::open(event_path)?;
    event_conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;",
    )?;
    Ok(event_conn)
}

#[derive(Clone)]
pub struct MemDb {
    conn: Arc<Mutex<Connection>>,
    event_conn: Arc<Mutex<Connection>>,
    event_db_path: Option<Arc<PathBuf>>,
}

impl MemDb {
    /// Open (or create) the DB at `path`, WAL + busy timeout, run migrations.
    pub fn open<P: AsRef<Path>>(path: P) -> rusqlite::Result<Self> {
        let path = path.as_ref();
        let mut conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;",
        )?;
        migrate(&mut conn)?;
        backfill_legacy_memory_identity(&mut conn)?;
        backfill_legacy_transform_identity(&mut conn)?;
        backfill_legacy_recall_identity(&mut conn)?;
        let event_path = resolve_event_db_path(path);
        let event_conn = extract_event_ledger(&mut conn, &event_path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            event_conn: Arc::new(Mutex::new(event_conn)),
            event_db_path: Some(Arc::new(event_path)),
        };
        db.flush_context_event_outbox()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        Ok(db)
    }

    /// Ephemeral in-memory DB (tests / no path).
    pub fn open_in_memory() -> Self {
        let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
        conn.execute_batch("PRAGMA busy_timeout=5000; PRAGMA temp_store=MEMORY;")
            .ok();
        migrate(&mut conn).expect("migrate in-memory memdb");
        backfill_legacy_memory_identity(&mut conn).expect("backfill in-memory memory identity");
        backfill_legacy_transform_identity(&mut conn)
            .expect("backfill in-memory transform identity");
        backfill_legacy_recall_identity(&mut conn).expect("backfill in-memory recall identity");
        let conn = Arc::new(Mutex::new(conn));
        Self {
            conn: conn.clone(),
            event_conn: conn,
            event_db_path: None,
        }
    }

    /// Lock the connection for a unit of work. Poison-recovering: a worker thread that panics
    /// mid-statement must NOT wedge every other worker's DB access into a serve-wide outage
    /// (the serve loop runs `route` under `catch_unwind`, so a panic is contained to one request;
    /// the mutex must not amplify it back to the whole service).
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn lock_events(&self) -> MutexGuard<'_, Connection> {
        self.event_conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn event_db_path(&self) -> Option<&Path> {
        self.event_db_path.as_deref().map(PathBuf::as_path)
    }

    /// Probe the live SQLite connection without waiting behind another request. The forge query
    /// verifies both connection execution and the required primary table; an empty table is healthy.
    pub fn health_probe(&self) -> MemDbProbe {
        let (conn, recovered_poison) = match self.conn.try_lock() {
            Ok(conn) => (conn, false),
            Err(std::sync::TryLockError::WouldBlock) => return MemDbProbe::Busy,
            Err(std::sync::TryLockError::Poisoned(error)) => (error.into_inner(), true),
        };
        match conn
            .query_row("SELECT 1 FROM memories LIMIT 1", [], |row| {
                row.get::<_, i64>(0)
            })
            .optional()
        {
            Ok(_) => {
                if recovered_poison {
                    self.conn.clear_poison();
                }
                MemDbProbe::Ok
            }
            Err(_) => MemDbProbe::Error,
        }
    }

    /// Record one recall's full-corpus-vs-injected-skeleton size, so savings are measurable instead
    /// of assumed. `source` separates agent-facing serve recalls from human CLI debugging (the
    /// effectiveness numbers read serve rows only); `query_preview` (first ~120 chars) makes
    /// ranking changes verifiable by replaying real traffic. Best-effort: a logging failure must
    /// never break a recall response.
    ///
    /// 2026-07-09 (add-now plan, Phase 1): `client`/`session_id`/`cwd_scope`/`hook_event`/
    /// `trace_id`/`client_visibility` add cross-client observability. All optional; absent values
    /// backfill as NULL/`unknown`/`all` so older call sites keep working unchanged.
    #[allow(clippy::too_many_arguments)]
    pub fn log_recall(
        &self,
        ts: &str,
        scope: Option<&str>,
        query_chars: usize,
        hit_count: usize,
        full_chars: usize,
        injected_chars: usize,
        source: &str,
        query_preview: Option<&str>,
        client: Option<&str>,
        session_id: Option<&str>,
        cwd_scope: Option<&str>,
        hook_event: Option<&str>,
        trace_id: Option<&str>,
        client_visibility: Option<&str>,
    ) {
        self.log_recall_with_replay(
            ts,
            scope,
            query_chars,
            hit_count,
            full_chars,
            injected_chars,
            source,
            query_preview,
            client,
            session_id,
            cwd_scope,
            hook_event,
            trace_id,
            client_visibility,
            "production",
            "[]",
            "[]",
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_recall_with_replay(
        &self,
        ts: &str,
        scope: Option<&str>,
        query_chars: usize,
        hit_count: usize,
        full_chars: usize,
        injected_chars: usize,
        source: &str,
        query_preview: Option<&str>,
        client: Option<&str>,
        session_id: Option<&str>,
        cwd_scope: Option<&str>,
        hook_event: Option<&str>,
        trace_id: Option<&str>,
        client_visibility: Option<&str>,
        traffic_class: &str,
        candidate_hits: &str,
        admitted_hits: &str,
    ) {
        let _ = self.log_recall_with_replay_to(
            RecallLogSink::Production,
            ts,
            scope,
            query_chars,
            hit_count,
            full_chars,
            injected_chars,
            source,
            query_preview,
            client,
            session_id,
            cwd_scope,
            hook_event,
            trace_id,
            client_visibility,
            traffic_class,
            candidate_hits,
            admitted_hits,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_recall_with_replay_to(
        &self,
        sink: RecallLogSink,
        ts: &str,
        scope: Option<&str>,
        query_chars: usize,
        hit_count: usize,
        full_chars: usize,
        injected_chars: usize,
        source: &str,
        query_preview: Option<&str>,
        client: Option<&str>,
        session_id: Option<&str>,
        cwd_scope: Option<&str>,
        hook_event: Option<&str>,
        trace_id: Option<&str>,
        client_visibility: Option<&str>,
        traffic_class: &str,
        candidate_hits: &str,
        admitted_hits: &str,
    ) -> Result<(), String> {
        self.log_recall_with_replay_to_identity(
            sink,
            ts,
            scope,
            query_chars,
            hit_count,
            full_chars,
            injected_chars,
            source,
            query_preview,
            client,
            session_id,
            cwd_scope,
            hook_event,
            trace_id,
            client_visibility,
            traffic_class,
            candidate_hits,
            admitted_hits,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_recall_with_replay_to_identity(
        &self,
        sink: RecallLogSink,
        ts: &str,
        scope: Option<&str>,
        query_chars: usize,
        hit_count: usize,
        full_chars: usize,
        injected_chars: usize,
        source: &str,
        query_preview: Option<&str>,
        client: Option<&str>,
        session_id: Option<&str>,
        cwd_scope: Option<&str>,
        hook_event: Option<&str>,
        trace_id: Option<&str>,
        client_visibility: Option<&str>,
        traffic_class: &str,
        candidate_hits: &str,
        admitted_hits: &str,
        installation_id: Option<&str>,
        service_instance_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> Result<(), String> {
        let event_uid = transform_event_uid()
            .map(|value| value.replacen("transform.", "recall.", 1))
            .ok_or_else(|| "generate recall event uid".to_string())?;
        let identity_status = if installation_id.is_some()
            && service_instance_id.is_some()
            && session_id.is_some()
            && turn_id.is_some()
            && trace_id.is_some()
        {
            "observed"
        } else {
            "legacy_unattributed"
        };
        let fallback_session = opaque_legacy_identity("session-", &event_uid);
        let fallback_turn = opaque_legacy_identity("turn-", &event_uid);
        let fallback_trace = opaque_legacy_identity("trace-", &event_uid);
        let conn = self.lock();
        match sink {
            RecallLogSink::Production => conn.execute(
                "INSERT INTO recall_log
                    (ts, scope, query_chars, hit_count, full_chars, injected_chars, source,
                     query_preview, client, session_id, cwd_scope, hook_event, trace_id,
                     client_visibility, traffic_class, candidate_hits, admitted_hits,
                     event_uid, installation_id, service_instance_id, turn_id, identity_status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                         ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
                rusqlite::params![
                    ts,
                    scope,
                    query_chars as i64,
                    hit_count as i64,
                    full_chars as i64,
                    injected_chars as i64,
                    source,
                    query_preview,
                    client.unwrap_or("unknown"),
                    session_id.unwrap_or(&fallback_session),
                    cwd_scope,
                    hook_event,
                    trace_id.unwrap_or(&fallback_trace),
                    client_visibility.unwrap_or("all"),
                    traffic_class,
                    candidate_hits,
                    admitted_hits,
                    event_uid,
                    installation_id,
                    service_instance_id,
                    turn_id.unwrap_or(&fallback_turn),
                    identity_status,
                ],
            ),
            RecallLogSink::Smoke => conn.execute(
                "INSERT INTO recall_log_smoke
                    (ts, scope, query_chars, hit_count, full_chars, injected_chars, source,
                     query_preview, client, session_id, cwd_scope, hook_event, trace_id,
                     client_visibility, traffic_class, candidate_hits, admitted_hits,
                     source_recall_id, original_traffic_class, isolated_at, isolation_reason,
                     event_uid, installation_id, service_instance_id, turn_id, identity_status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                         'smoke', ?16, ?17, NULL, ?15, ?1, 'prospective_smoke',
                         ?18, ?19, ?20, ?21, ?22)",
                rusqlite::params![
                    ts,
                    scope,
                    query_chars as i64,
                    hit_count as i64,
                    full_chars as i64,
                    injected_chars as i64,
                    source,
                    query_preview,
                    client.unwrap_or("unknown"),
                    session_id.unwrap_or(&fallback_session),
                    cwd_scope,
                    hook_event,
                    trace_id.unwrap_or(&fallback_trace),
                    client_visibility.unwrap_or("all"),
                    traffic_class,
                    candidate_hits,
                    admitted_hits,
                    event_uid,
                    installation_id,
                    service_instance_id,
                    turn_id.unwrap_or(&fallback_turn),
                    identity_status,
                ],
            ),
        }
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    /// Plan or apply the one-time RC-2.3 cleanup. Apply holds an IMMEDIATE transaction from the
    /// exact-count check through copy verification, deletion, non-target verification, and ledger
    /// write. The ledger makes a verified rerun a no-op; any new matching production rows fail.
    pub fn isolate_smoke_recalls(
        &self,
        expected: usize,
        apply: bool,
    ) -> Result<SmokeIsolationReport, String> {
        let expected_i64 = i64::try_from(expected).map_err(|_| "expected count too large")?;
        let mut conn = self.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;

        let ledger = tx
            .query_row(
                "SELECT expected_count, moved_count, target_digest, non_target_count,
                        non_target_digest
                   FROM smoke_recall_isolation_ledger WHERE migration_id=?1",
                [SMOKE_ISOLATION_MIGRATION_ID],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let production_smoke_rows: i64 = tx
            .query_row(
                &format!("SELECT COUNT(*) FROM recall_log WHERE {SMOKE_RECALL_PREDICATE}"),
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;

        if let Some((ledger_expected, moved, target_digest, non_target_count, non_target_digest)) =
            ledger
        {
            if ledger_expected != expected_i64 {
                return Err(format!(
                    "isolation ledger expected {ledger_expected}, requested {expected_i64}"
                ));
            }
            if production_smoke_rows != 0 {
                return Err(format!(
                    "isolation ledger exists but {production_smoke_rows} new production smoke rows match"
                ));
            }
            let copied_count: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM recall_log_smoke
                      WHERE isolation_reason='legacy_production_smoke_spotcheck'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            let copied_digest = digest_query_rows(
                &tx,
                "SELECT source_recall_id AS id, ts, scope, query_chars, hit_count, full_chars,
                        injected_chars, source, query_preview, client, session_id, cwd_scope,
                        hook_event, trace_id, client_visibility,
                        original_traffic_class AS traffic_class, candidate_hits, admitted_hits
                   FROM recall_log_smoke
                  WHERE isolation_reason='legacy_production_smoke_spotcheck'
                  ORDER BY source_recall_id",
            )
            .map_err(|error| error.to_string())?;
            if copied_count != moved || copied_digest != target_digest {
                return Err("isolation ledger does not match physically isolated rows".into());
            }
            tx.commit().map_err(|error| error.to_string())?;
            return Ok(SmokeIsolationReport {
                schema_version: LATEST_SCHEMA_VERSION,
                mode: if apply { "apply" } else { "dry_run" },
                expected_count: expected,
                matched_count: 0,
                moved_count: 0,
                production_smoke_rows: 0,
                target_digest,
                non_target_count: non_target_count as usize,
                non_target_digest,
                already_applied: true,
            });
        }

        if production_smoke_rows != expected_i64 {
            return Err(format!(
                "expected exactly {expected_i64} production smoke rows, found {production_smoke_rows}"
            ));
        }
        let target_digest = digest_query_rows(
            &tx,
            &format!(
                "SELECT {RECALL_DIGEST_COLUMNS} FROM recall_log
                  WHERE {SMOKE_RECALL_PREDICATE} ORDER BY id"
            ),
        )
        .map_err(|error| error.to_string())?;
        let non_target_count: i64 = tx
            .query_row(
                &format!("SELECT COUNT(*) FROM recall_log WHERE NOT ({SMOKE_RECALL_PREDICATE})"),
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let non_target_digest = digest_query_rows(
            &tx,
            &format!(
                "SELECT {RECALL_DIGEST_COLUMNS} FROM recall_log
                  WHERE NOT ({SMOKE_RECALL_PREDICATE}) ORDER BY id"
            ),
        )
        .map_err(|error| error.to_string())?;

        if !apply {
            tx.commit().map_err(|error| error.to_string())?;
            return Ok(SmokeIsolationReport {
                schema_version: LATEST_SCHEMA_VERSION,
                mode: "dry_run",
                expected_count: expected,
                matched_count: production_smoke_rows as usize,
                moved_count: 0,
                production_smoke_rows: production_smoke_rows as usize,
                target_digest,
                non_target_count: non_target_count as usize,
                non_target_digest,
                already_applied: false,
            });
        }

        let isolated_at = crate::time::now_iso();
        let copied = tx
            .execute(
                &format!(
                    "INSERT INTO recall_log_smoke
                        (ts, scope, query_chars, hit_count, full_chars, injected_chars, source,
                         query_preview, client, session_id, cwd_scope, hook_event, trace_id,
                         client_visibility, traffic_class, candidate_hits, admitted_hits,
                         source_recall_id, original_traffic_class, isolated_at, isolation_reason,
                         event_uid, installation_id, service_instance_id, turn_id, identity_status)
                     SELECT ts, scope, query_chars, hit_count, full_chars, injected_chars, source,
                            query_preview, client, session_id, cwd_scope, hook_event, trace_id,
                            client_visibility, 'smoke', candidate_hits, admitted_hits, id,
                            traffic_class, ?1, ?2, event_uid, installation_id,
                            service_instance_id, turn_id, identity_status
                       FROM recall_log WHERE {SMOKE_RECALL_PREDICATE} ORDER BY id"
                ),
                rusqlite::params![isolated_at, SMOKE_ISOLATION_REASON],
            )
            .map_err(|error| error.to_string())?;
        if copied != expected {
            return Err(format!("copied {copied} rows, expected {expected}"));
        }
        let copied_digest = digest_query_rows(
            &tx,
            "SELECT source_recall_id AS id, ts, scope, query_chars, hit_count, full_chars,
                    injected_chars, source, query_preview, client, session_id, cwd_scope,
                    hook_event, trace_id, client_visibility,
                    original_traffic_class AS traffic_class, candidate_hits, admitted_hits
               FROM recall_log_smoke
              WHERE isolation_reason='legacy_production_smoke_spotcheck'
              ORDER BY source_recall_id",
        )
        .map_err(|error| error.to_string())?;
        if copied_digest != target_digest {
            return Err("copied smoke recall digest does not match source digest".into());
        }
        let deleted = tx
            .execute(
                &format!("DELETE FROM recall_log WHERE {SMOKE_RECALL_PREDICATE}"),
                [],
            )
            .map_err(|error| error.to_string())?;
        if deleted != expected {
            return Err(format!("deleted {deleted} rows, expected {expected}"));
        }
        let remaining_smoke: i64 = tx
            .query_row(
                &format!("SELECT COUNT(*) FROM recall_log WHERE {SMOKE_RECALL_PREDICATE}"),
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let post_non_target_count: i64 = tx
            .query_row("SELECT COUNT(*) FROM recall_log", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        let post_non_target_digest = digest_query_rows(
            &tx,
            &format!("SELECT {RECALL_DIGEST_COLUMNS} FROM recall_log ORDER BY id"),
        )
        .map_err(|error| error.to_string())?;
        if remaining_smoke != 0
            || post_non_target_count != non_target_count
            || post_non_target_digest != non_target_digest
        {
            return Err("post-isolation production verification failed".into());
        }
        tx.execute(
            "INSERT INTO smoke_recall_isolation_ledger
                (migration_id, applied_at, expected_count, moved_count, target_digest,
                 non_target_count, non_target_digest, isolation_reason)
             VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                SMOKE_ISOLATION_MIGRATION_ID,
                isolated_at,
                expected_i64,
                target_digest,
                non_target_count,
                non_target_digest,
                SMOKE_ISOLATION_REASON,
            ],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(SmokeIsolationReport {
            schema_version: LATEST_SCHEMA_VERSION,
            mode: "apply",
            expected_count: expected,
            matched_count: expected,
            moved_count: expected,
            production_smoke_rows: 0,
            target_digest,
            non_target_count: non_target_count as usize,
            non_target_digest,
            already_applied: false,
        })
    }

    /// Record one transform invocation so per-layer savings are measurable instead of assumed.
    /// Best-effort: a logging failure must never break the transform it's measuring.
    pub fn log_transform(
        &self,
        ts: &str,
        verb: &str,
        scope: Option<&str>,
        before_chars: usize,
        after_chars: usize,
        meta: Option<&str>,
    ) {
        let conn = self.lock();
        let _ = conn.execute(
            "INSERT INTO transform_log (ts, verb, scope, before_chars, after_chars, meta) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                ts,
                verb,
                scope,
                before_chars as i64,
                after_chars as i64,
                meta
            ],
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_transform_with_identity(
        &self,
        ts: &str,
        verb: &str,
        scope: Option<&str>,
        before_chars: usize,
        after_chars: usize,
        meta: Option<&str>,
        installation_id: &str,
        service_instance_id: &str,
        client: &str,
        session_id: &str,
        turn_id: &str,
        trace_id: &str,
    ) {
        let Some(event_uid) = transform_event_uid() else {
            return;
        };
        let conn = self.lock();
        let _ = conn.execute(
            "INSERT INTO transform_log
             (ts, verb, scope, before_chars, after_chars, meta, event_uid,
              installation_id, service_instance_id, client, session_id, turn_id,
              trace_id, identity_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'observed')",
            rusqlite::params![
                ts,
                verb,
                scope,
                before_chars as i64,
                after_chars as i64,
                meta,
                event_uid,
                installation_id,
                service_instance_id,
                client,
                session_id,
                turn_id,
                trace_id,
            ],
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_transform_opportunity(
        &self,
        ts: &str,
        opportunity_uid: &str,
        verb: &str,
        source: &str,
        reason_code: &str,
        installation_id: &str,
        service_instance_id: &str,
        client: &str,
        session_id: &str,
        turn_id: &str,
        trace_id: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT OR IGNORE INTO transform_opportunity_log
             (opportunity_uid, ts, verb, source, reason_code, outcome,
              installation_id, service_instance_id, client, session_id, turn_id,
              trace_id, identity_status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'recommended', ?6, ?7, ?8, ?9, ?10, ?11, 'observed')",
            rusqlite::params![
                opportunity_uid,
                ts,
                verb,
                source,
                reason_code,
                installation_id,
                service_instance_id,
                client,
                session_id,
                turn_id,
                trace_id,
            ],
        )?;
        Ok(())
    }

    pub fn log_transform_for_opportunity(
        &self,
        ts: &str,
        verb: &str,
        scope: Option<&str>,
        before_chars: usize,
        after_chars: usize,
        meta: Option<&str>,
        opportunity_uid: &str,
    ) -> rusqlite::Result<()> {
        let Some(event_uid) = transform_event_uid() else {
            return Err(rusqlite::Error::InvalidQuery);
        };
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let identity: (String, String, String, String, String, String) = tx.query_row(
            "SELECT installation_id, service_instance_id, client, session_id, turn_id, trace_id
             FROM transform_opportunity_log
             WHERE opportunity_uid=?1 AND verb=?2",
            rusqlite::params![opportunity_uid, verb],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )?;
        tx.execute(
            "INSERT INTO transform_log
             (ts, verb, scope, before_chars, after_chars, meta, event_uid,
              installation_id, service_instance_id, client, session_id, turn_id,
              trace_id, opportunity_uid, identity_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 'observed')",
            rusqlite::params![
                ts,
                verb,
                scope,
                before_chars as i64,
                after_chars as i64,
                meta,
                event_uid,
                identity.0,
                identity.1,
                identity.2,
                identity.3,
                identity.4,
                identity.5,
                opportunity_uid,
            ],
        )?;
        let outcome = if meta.unwrap_or_default().starts_with("status=error") {
            "error"
        } else {
            "used"
        };
        tx.execute(
            "UPDATE transform_opportunity_log
             SET outcome=?2, resolved_at=?3, resolved_event_uid=?4
             WHERE opportunity_uid=?1",
            rusqlite::params![opportunity_uid, outcome, ts, event_uid],
        )?;
        tx.commit()
    }

    /// Per-verb aggregation of the transform log (empty if nothing has been logged).
    pub fn transform_metrics(&self) -> Vec<TransformVerbMetrics> {
        let conn = self.lock();
        let mut stmt = match conn.prepare(
            "SELECT verb, COUNT(*), COALESCE(SUM(before_chars),0), COALESCE(SUM(after_chars),0) \
             FROM transform_log GROUP BY verb ORDER BY verb",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |row| {
            Ok(TransformVerbMetrics {
                verb: row.get(0)?,
                runs: row.get::<_, i64>(1)? as u64,
                before_chars: row.get::<_, i64>(2)? as u64,
                after_chars: row.get::<_, i64>(3)? as u64,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Effectiveness snapshot over the memories table: corpus size, distinct entries ever
    /// injected (serve path bumps inject_count), and distinct injected entries later fetched in
    /// full (`get` bumps access_count). The fetch rate is a LOWER BOUND on usefulness — preview
    /// sufficiency and fetch friction both suppress it (§10.2 paired-metric protocol).
    pub fn effectiveness_metrics(&self) -> (u64, u64, u64) {
        let conn = self.lock();
        conn.query_row(
            "SELECT (SELECT COUNT(*) FROM memories), \
                    (SELECT COUNT(*) FROM memories WHERE inject_count > 0), \
                    (SELECT COUNT(*) FROM memories WHERE inject_count > 0 AND access_count > 0)",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)? as u64,
                    r.get::<_, i64>(1)? as u64,
                    r.get::<_, i64>(2)? as u64,
                ))
            },
        )
        .unwrap_or((0, 0, 0))
    }

    /// Most recent recall-log rows (live-usage feed for the dashboard), newest first.
    pub fn recent_recalls(&self, n: usize) -> Vec<RecentRecall> {
        let conn = self.lock();
        conn.prepare(
            "SELECT ts, COALESCE(scope,''), query_chars, hit_count, injected_chars,
                    traffic_class, candidate_hits, admitted_hits \
             FROM recall_log ORDER BY ts DESC LIMIT ?1",
        )
        .and_then(|mut st| {
            st.query_map([n as i64], |r| {
                Ok(RecentRecall {
                    ts: r.get(0)?,
                    scope: r.get(1)?,
                    query_chars: r.get(2)?,
                    hit_count: r.get(3)?,
                    injected_chars: r.get(4)?,
                    traffic_class: r.get(5)?,
                    candidate_hits: r.get(6)?,
                    admitted_hits: r.get(7)?,
                })
            })
            .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default()
    }

    /// Most recent transform-log rows, newest first.
    pub fn recent_transforms(&self, n: usize) -> Vec<(String, String, i64, i64)> {
        let conn = self.lock();
        conn.prepare(
            "SELECT ts, verb, before_chars, after_chars FROM transform_log \
             ORDER BY ts DESC LIMIT ?1",
        )
        .and_then(|mut st| {
            st.query_map([n as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default()
    }

    /// Aggregate the recall log into a savings summary. `None` if no recalls have been logged yet.
    /// Serve rows only: CLI debugging recalls (`source='cli'`) never skew the agent-facing numbers.
    pub fn recall_metrics(&self) -> Option<RecallMetrics> {
        let conn = self.lock();
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(hit_count),0), COALESCE(SUM(full_chars),0), \
             COALESCE(SUM(injected_chars),0), MIN(ts), MAX(ts) FROM recall_log \
             WHERE source = 'serve' AND query_chars > 0 AND traffic_class = 'production'",
            [],
            |row| {
                let recalls: i64 = row.get(0)?;
                if recalls == 0 {
                    return Ok(None);
                }
                Ok(Some(RecallMetrics {
                    recalls: recalls as u64,
                    hits: row.get::<_, i64>(1)? as u64,
                    full_chars: row.get::<_, i64>(2)? as u64,
                    injected_chars: row.get::<_, i64>(3)? as u64,
                    first_ts: row.get(4)?,
                    last_ts: row.get(5)?,
                }))
            },
        )
        .ok()
        .flatten()
    }

    /// 2026-07-09 (add-now plan, Phase 1): per-client recall counts over serve rows.
    /// Returns (client, count) ordered by count desc, then client asc for stable output.
    /// `client = 'unknown'` is a backfill for rows written before the attribution columns
    /// existed (or for hooks that never set the field) — kept as a separate bucket so we can
    /// tell "no client claim" from "zero recalls".
    pub fn client_recall_counts(&self) -> Vec<(String, u64)> {
        let conn = self.lock();
        conn.prepare(
            "SELECT client, COUNT(*) FROM recall_log WHERE source = 'serve' \
             AND query_chars > 0 AND traffic_class = 'production' \
             AND (client != 'unknown' OR query_preview IS NOT NULL) \
             GROUP BY client ORDER BY COUNT(*) DESC, client ASC",
        )
        .and_then(|mut st| {
            st.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
            })
            .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default()
    }

    /// Attribution quality for interpreting `client='unknown'`.
    ///
    /// The DB was migrated in-place: old rows did not record `query_preview` or client metadata,
    /// so they must stay visible as legacy rather than being treated as active pollution.
    /// Empty-query rows are excluded from primary metrics, but kept here as a raw hygiene count.
    pub fn attribution_quality(&self) -> AttributionQuality {
        let conn = self.lock();
        conn.query_row(
            "SELECT \
               SUM(CASE WHEN source='serve' AND query_chars > 0 AND client!='unknown' THEN 1 ELSE 0 END), \
               SUM(CASE WHEN source='serve' AND client='unknown' THEN 1 ELSE 0 END), \
               SUM(CASE WHEN source='serve' AND client='unknown' AND query_chars > 0 AND query_preview IS NULL THEN 1 ELSE 0 END), \
               SUM(CASE WHEN source='serve' AND client='unknown' AND query_chars = 0 THEN 1 ELSE 0 END), \
               SUM(CASE WHEN source='serve' AND client='unknown' AND query_chars > 0 AND query_preview IS NOT NULL THEN 1 ELSE 0 END), \
               SUM(CASE WHEN source='serve' AND client='unknown' AND query_chars > 0 \
                         AND query_preview IS NOT NULL \
                         AND identity_status='legacy_unattributed' \
                         AND cwd_scope IS NULL AND hook_event IS NULL \
                    THEN 1 ELSE 0 END), \
               MIN(CASE WHEN source='serve' AND client='unknown' THEN ts ELSE NULL END), \
               MAX(CASE WHEN source='serve' AND client='unknown' THEN ts ELSE NULL END) \
             FROM recall_log WHERE traffic_class = 'production'",
            [],
            |r| {
                Ok(AttributionQuality {
                    attributed_meaningful: r.get::<_, Option<i64>>(0)?.unwrap_or(0) as u64,
                    unknown_total_raw: r.get::<_, Option<i64>>(1)?.unwrap_or(0) as u64,
                    unknown_legacy_no_preview: r.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                    unknown_empty_query: r.get::<_, Option<i64>>(3)?.unwrap_or(0) as u64,
                    unknown_current_meaningful: r.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
                    unknown_current_no_metadata: r.get::<_, Option<i64>>(5)?.unwrap_or(0) as u64,
                    unknown_first_ts: r.get(6)?,
                    unknown_last_ts: r.get(7)?,
                })
            },
        )
        .unwrap_or_default()
    }
}

/// Aggregate token-savings report: what would have been sent if every recall dumped full memory
/// content, vs what was actually injected (the skeleton).
pub struct RecentRecall {
    pub ts: String,
    pub scope: String,
    pub query_chars: i64,
    pub hit_count: i64,
    pub injected_chars: i64,
    pub traffic_class: String,
    pub candidate_hits: String,
    pub admitted_hits: String,
}

pub struct RecallMetrics {
    pub recalls: u64,
    pub hits: u64,
    pub full_chars: u64,
    pub injected_chars: u64,
    pub first_ts: String,
    pub last_ts: String,
}

#[derive(Default)]
pub struct AttributionQuality {
    pub attributed_meaningful: u64,
    pub unknown_total_raw: u64,
    pub unknown_legacy_no_preview: u64,
    pub unknown_empty_query: u64,
    pub unknown_current_meaningful: u64,
    pub unknown_current_no_metadata: u64,
    pub unknown_first_ts: Option<String>,
    pub unknown_last_ts: Option<String>,
}

impl RecallMetrics {
    /// Chars saved per recall by injecting a skeleton instead of full content.
    pub fn chars_saved(&self) -> i64 {
        self.full_chars as i64 - self.injected_chars as i64
    }

    /// Rough token estimate at ~4 chars/token (no tokenizer dependency, intentionally approximate).
    pub fn tokens_saved(&self) -> i64 {
        self.chars_saved() / 4
    }
}

/// One verb's aggregated transform savings (`crypt metrics` transforms block).
pub struct TransformVerbMetrics {
    pub verb: String,
    pub runs: u64,
    pub before_chars: u64,
    pub after_chars: u64,
}

impl TransformVerbMetrics {
    pub fn chars_saved(&self) -> i64 {
        self.before_chars as i64 - self.after_chars as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_connections_use_memory_temp_store() {
        let db = MemDb::open_in_memory();
        let temp_store: i64 = db
            .lock()
            .query_row("PRAGMA temp_store", [], |row| row.get(0))
            .unwrap();
        assert_eq!(temp_store, 2);
    }

    #[test]
    fn health_probe_recovers_a_poisoned_connection_after_a_successful_forge() {
        let db = MemDb::open_in_memory();
        let connection = Arc::clone(&db.conn);
        let poisoner = std::thread::spawn(move || {
            let _guard = connection.lock().unwrap();
            panic!("poison the test connection mutex");
        });
        assert!(poisoner.join().is_err());
        assert!(db.conn.is_poisoned());

        assert_eq!(db.health_probe(), MemDbProbe::Ok);
        assert!(
            !db.conn.is_poisoned(),
            "a successful forge must clear recovered mutex poison"
        );
    }

    #[test]
    fn health_probe_keeps_poison_when_the_recovered_forge_fails() {
        let db = MemDb::open_in_memory();
        db.lock().execute("DROP TABLE memories", []).unwrap();
        let connection = Arc::clone(&db.conn);
        let poisoner = std::thread::spawn(move || {
            let _guard = connection.lock().unwrap();
            panic!("poison the test connection mutex");
        });
        assert!(poisoner.join().is_err());
        assert!(db.conn.is_poisoned());

        assert_eq!(db.health_probe(), MemDbProbe::Error);
        assert!(
            db.conn.is_poisoned(),
            "a failed forge must not clear mutex poison"
        );
    }

    fn insert_recall_fixture(
        db: &MemDb,
        id: i64,
        client: &str,
        traffic_class: &str,
        query_preview: &str,
    ) {
        db.lock()
            .execute(
                r#"INSERT INTO recall_log
                    (id, ts, scope, query_chars, hit_count, full_chars, injected_chars, source,
                     query_preview, client, session_id, cwd_scope, hook_event, trace_id,
                     client_visibility, traffic_class, candidate_hits, admitted_hits)
                 VALUES (?1, '2026-07-18T00:00:00Z', 'global', 11, 2, 100, 20, 'serve',
                         ?2, ?3, 'session', 'D--Claude', 'UserPromptSubmit', 'trace', 'all',
                         ?4, '[{"id":"global/a"}]', '[{"id":"global/a"}]')"#,
                rusqlite::params![id, query_preview, client, traffic_class],
            )
            .unwrap();
    }

    #[test]
    fn latest_schema_preserves_v11_physical_smoke_sink_and_ledger() {
        let db = MemDb::open_in_memory();
        let conn = db.lock();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
        let columns = conn
            .prepare("PRAGMA table_info(recall_log_smoke)")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(1))
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            })
            .unwrap();
        assert_eq!(
            columns,
            vec![
                "id",
                "ts",
                "scope",
                "query_chars",
                "hit_count",
                "full_chars",
                "injected_chars",
                "source",
                "query_preview",
                "client",
                "session_id",
                "cwd_scope",
                "hook_event",
                "trace_id",
                "client_visibility",
                "traffic_class",
                "candidate_hits",
                "admitted_hits",
                "source_recall_id",
                "original_traffic_class",
                "isolated_at",
                "isolation_reason",
                "event_uid",
                "installation_id",
                "service_instance_id",
                "turn_id",
                "identity_status",
            ]
        );
        let ledger: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='table' AND name='smoke_recall_isolation_ledger'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ledger, 1);
    }

    #[test]
    fn smoke_sink_logging_never_enters_production_recall_log() {
        let db = MemDb::open_in_memory();
        db.log_recall_with_replay_to(
            RecallLogSink::Smoke,
            "2026-07-18T00:00:00Z",
            Some("global"),
            10,
            1,
            100,
            20,
            "serve",
            Some("smoke query"),
            Some("smoke-spotcheck"),
            None,
            None,
            None,
            None,
            Some("all"),
            "evaluation",
            "[]",
            "[]",
        )
        .unwrap();
        let conn = db.lock();
        let production: i64 = conn
            .query_row("SELECT COUNT(*) FROM recall_log", [], |row| row.get(0))
            .unwrap();
        let smoke: (i64, String, String, String) = conn
            .query_row(
                "SELECT COUNT(*), traffic_class, original_traffic_class, isolation_reason
                   FROM recall_log_smoke",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(production, 0);
        assert_eq!(
            smoke,
            (
                1,
                "smoke".into(),
                "evaluation".into(),
                "prospective_smoke".into()
            )
        );
    }

    #[test]
    fn smoke_isolation_dry_run_is_non_mutating_and_apply_is_exact_and_idempotent() {
        let db = MemDb::open_in_memory();
        insert_recall_fixture(&db, 10, " smoke-spotcheck ", "production", "target-a");
        insert_recall_fixture(&db, 11, "SMOKE-SPOTCHECK", "production", "target-b");
        insert_recall_fixture(&db, 12, "codex", "production", "keep-production");
        insert_recall_fixture(&db, 13, "smoke-spotcheck", "evaluation", "keep-evaluation");

        let dry = db.isolate_smoke_recalls(2, false).unwrap();
        assert_eq!(
            (dry.matched_count, dry.moved_count, dry.already_applied),
            (2, 0, false)
        );
        assert!(dry.target_digest.starts_with("sha256:"));
        assert!(dry.non_target_digest.starts_with("sha256:"));
        assert_eq!(
            db.lock()
                .query_row("SELECT COUNT(*) FROM recall_log", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            4
        );

        let applied = db.isolate_smoke_recalls(2, true).unwrap();
        assert_eq!(
            (
                applied.matched_count,
                applied.moved_count,
                applied.production_smoke_rows
            ),
            (2, 2, 0)
        );
        assert_eq!(applied.target_digest, dry.target_digest);
        assert_eq!(applied.non_target_digest, dry.non_target_digest);
        let conn = db.lock();
        let remaining: Vec<(i64, String, String)> = conn
            .prepare("SELECT id, client, traffic_class FROM recall_log ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            remaining,
            vec![
                (12, "codex".into(), "production".into()),
                (13, "smoke-spotcheck".into(), "evaluation".into())
            ]
        );
        let copied: Vec<(i64, String, String)> = conn
            .prepare("SELECT source_recall_id, traffic_class, original_traffic_class FROM recall_log_smoke ORDER BY source_recall_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            copied,
            vec![
                (10, "smoke".into(), "production".into()),
                (11, "smoke".into(), "production".into())
            ]
        );
        drop(conn);

        let rerun = db.isolate_smoke_recalls(2, true).unwrap();
        assert!(rerun.already_applied);
        assert_eq!((rerun.moved_count, rerun.production_smoke_rows), (0, 0));
        assert_eq!(
            db.lock()
                .query_row(
                    "SELECT COUNT(*) FROM smoke_recall_isolation_ledger",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn smoke_isolation_exact_count_and_copy_faults_roll_back_everything() {
        let db = MemDb::open_in_memory();
        insert_recall_fixture(&db, 20, "smoke-spotcheck", "production", "target");
        assert!(db.isolate_smoke_recalls(2, true).is_err());
        assert_eq!(
            db.lock()
                .query_row("SELECT COUNT(*) FROM recall_log", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );

        db.lock()
            .execute(
                "INSERT INTO recall_log_smoke
                (ts, query_chars, hit_count, full_chars, injected_chars, source, client,
                 client_visibility, traffic_class, candidate_hits, admitted_hits,
                 source_recall_id, original_traffic_class, isolated_at, isolation_reason)
             VALUES ('t',0,0,0,0,'serve','smoke','all','smoke','[]','[]',20,
                     'production','t','preexisting-conflict')",
                [],
            )
            .unwrap();
        assert!(db.isolate_smoke_recalls(1, true).is_err());
        let conn = db.lock();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM recall_log", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM smoke_recall_isolation_ledger",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn schema_v11_backout_restores_migrated_and_prospective_smoke_rows_without_loss() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backout-v11.db");
        let db = MemDb::open(&path).unwrap();
        insert_recall_fixture(&db, 30, "smoke-spotcheck", "production", "migrated");
        db.isolate_smoke_recalls(1, true).unwrap();
        db.log_recall_with_replay_to(
            RecallLogSink::Smoke,
            "2026-07-18T01:00:00Z",
            None,
            4,
            0,
            0,
            0,
            "serve",
            Some("prospective"),
            Some("smoke"),
            None,
            None,
            None,
            None,
            Some("all"),
            "evaluation",
            "[]",
            "[]",
        )
        .unwrap();
        drop(db);

        let restored = backout_v11_to_v10(&path).unwrap();
        assert_eq!(restored, 2);
        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        let rows: Vec<(i64, String, String)> = conn
            .prepare("SELECT id, query_preview, traffic_class FROM recall_log ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(version, 10);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], (30, "migrated".into(), "production".into()));
        assert_eq!(rows[1].1, "prospective");
        assert_eq!(rows[1].2, "evaluation");
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='recall_log_smoke'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn schema_v11_backout_collision_rolls_back_without_losing_either_copy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backout-v11-collision.db");
        let db = MemDb::open(&path).unwrap();
        insert_recall_fixture(&db, 40, "smoke-spotcheck", "production", "isolated-copy");
        db.isolate_smoke_recalls(1, true).unwrap();
        insert_recall_fixture(&db, 40, "codex", "production", "collision-copy");
        drop(db);

        assert!(backout_v11_to_v10(&path).is_err());
        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        let production: String = conn
            .query_row(
                "SELECT query_preview FROM recall_log WHERE id=40",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let isolated: String = conn
            .query_row(
                "SELECT query_preview FROM recall_log_smoke WHERE source_recall_id=40",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            (version, production, isolated),
            (11, "collision-copy".into(), "isolated-copy".into())
        );
    }

    #[test]
    fn dry_run_inspection_of_v10_database_is_byte_for_byte_nonmutating() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("read-only-dry-run-v10.db");
        let db = MemDb::open(&path).unwrap();
        drop(db);
        backout_v11_to_v10(&path).unwrap();
        let conn = Arc::new(Mutex::new(Connection::open(&path).unwrap()));
        let db = MemDb {
            conn: conn.clone(),
            event_conn: conn,
            event_db_path: None,
        };
        insert_recall_fixture(&db, 50, "smoke-spotcheck", "production", "dry-run-target");
        drop(db);
        let before = std::fs::read(&path).unwrap();

        let report = inspect_smoke_recalls(&path, 1).unwrap();
        assert_eq!(
            (
                report.schema_version,
                report.matched_count,
                report.moved_count
            ),
            (10, 1, 0)
        );
        assert!(!report.already_applied);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            10
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='recall_log_smoke'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM recall_log", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn v10_upgrade_to_latest_and_backout_preserve_all_recall_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v10-v11-roundtrip.db");
        let db = MemDb::open(&path).unwrap();
        drop(db);
        backout_v11_to_v10(&path).unwrap();
        let conn = Arc::new(Mutex::new(Connection::open(&path).unwrap()));
        let db = MemDb {
            conn: conn.clone(),
            event_conn: conn,
            event_db_path: None,
        };
        insert_recall_fixture(&db, 60, "codex", "production", "roundtrip-production");
        insert_recall_fixture(
            &db,
            61,
            "smoke-spotcheck",
            "evaluation",
            "roundtrip-evaluation",
        );
        let before = digest_query_rows(
            &db.lock(),
            &format!("SELECT {RECALL_DIGEST_COLUMNS} FROM recall_log ORDER BY id"),
        )
        .unwrap();
        drop(db);

        let upgraded = MemDb::open(&path).unwrap();
        assert_eq!(
            upgraded
                .lock()
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            LATEST_SCHEMA_VERSION
        );
        let after_upgrade = digest_query_rows(
            &upgraded.lock(),
            &format!("SELECT {RECALL_DIGEST_COLUMNS} FROM recall_log ORDER BY id"),
        )
        .unwrap();
        assert_eq!(after_upgrade, before);
        drop(upgraded);

        assert_eq!(backout_v11_to_v10(&path).unwrap(), 0);
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            10
        );
        let after_backout = digest_query_rows(
            &conn,
            &format!("SELECT {RECALL_DIGEST_COLUMNS} FROM recall_log ORDER BY id"),
        )
        .unwrap();
        assert_eq!(after_backout, before);
    }

    #[test]
    fn transform_log_records_and_aggregates() {
        let db = MemDb::open_in_memory();
        db.log_transform(
            "2026-07-02T00:00:00Z",
            "skel",
            Some("D--Claude"),
            1000,
            200,
            None,
        );
        db.log_transform(
            "2026-07-02T00:00:01Z",
            "skel",
            Some("D--Claude"),
            500,
            100,
            None,
        );
        db.log_transform("2026-07-02T00:00:02Z", "curate", None, 3, 5, None);
        let m = db.transform_metrics();
        assert_eq!(m.len(), 2);
        let skel = m.iter().find(|v| v.verb == "skel").expect("skel row");
        assert_eq!(
            (skel.runs, skel.before_chars, skel.after_chars),
            (2, 1500, 300)
        );
        assert_eq!(skel.chars_saved(), 1200);
        let curate = m.iter().find(|v| v.verb == "curate").expect("curate row");
        assert_eq!((curate.before_chars, curate.after_chars), (3, 5));
    }

    #[test]
    fn transform_log_records_complete_observed_identity() {
        let db = MemDb::open_in_memory();
        db.log_transform_with_identity(
            "2026-07-21T00:00:00Z",
            "skel",
            Some("D--Claude"),
            1000,
            200,
            None,
            "047e84c2-0c46-4a73-a0fc-41e9a3d25d10",
            "service-047e84c2",
            "codex",
            "session-codex-1",
            "turn-codex-1",
            "trace-codex-1",
        );
        let row: (
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        ) = db
            .lock()
            .query_row(
                "SELECT event_uid, installation_id, service_instance_id, client,
                        session_id, turn_id, trace_id, identity_status
                 FROM transform_log WHERE verb='skel'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert!(row.0.starts_with("transform."));
        assert_eq!(row.1, "047e84c2-0c46-4a73-a0fc-41e9a3d25d10");
        assert_eq!(row.2, "service-047e84c2");
        assert_eq!(row.3, "codex");
        assert_eq!(row.4, "session-codex-1");
        assert_eq!(row.5, "turn-codex-1");
        assert_eq!(row.6, "trace-codex-1");
        assert_eq!(row.7, "observed");
    }

    #[test]
    fn transform_opportunity_is_a_denominator_and_links_exact_execution() {
        let db = MemDb::open_in_memory();
        let opportunity_uid = "opportunity.runc.test-1";
        db.log_transform_opportunity(
            "2026-07-21T00:00:00Z",
            opportunity_uid,
            "runc",
            "brief-bash",
            "broad-read",
            "047e84c2-0c46-4a73-a0fc-41e9a3d25d10",
            "service-047e84c2",
            "codex",
            "session-codex-1",
            "turn-recommendation-1",
            "trace-recommendation-1",
        )
        .unwrap();

        db.log_transform_for_opportunity(
            "2026-07-21T00:00:01Z",
            "runc",
            Some("D--Claude"),
            1000,
            200,
            Some("status=ok"),
            opportunity_uid,
        )
        .unwrap();

        let row: (String, String, String, String, String, String) = db
            .lock()
            .query_row(
                "SELECT o.outcome, o.client, o.session_id, o.turn_id,
                        t.opportunity_uid, t.event_uid
                 FROM transform_opportunity_log o
                 JOIN transform_log t USING (opportunity_uid)
                 WHERE o.opportunity_uid=?1",
                [opportunity_uid],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, "used");
        assert_eq!(row.1, "codex");
        assert_eq!(row.2, "session-codex-1");
        assert_eq!(row.3, "turn-recommendation-1");
        assert_eq!(row.4, opportunity_uid);
        assert!(row.5.starts_with("transform."));
    }

    #[test]
    fn reopen_backfills_legacy_transform_identity_without_fabricating_installation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("legacy-transform.db");
        let db = MemDb::open(&path).unwrap();
        db.log_transform(
            "2026-07-01T00:00:00Z",
            "compress",
            Some("D--Claude"),
            500,
            250,
            None,
        );
        drop(db);

        let reopened = MemDb::open(&path).unwrap();
        let row: (
            String,
            Option<String>,
            Option<String>,
            String,
            String,
            String,
            String,
            String,
        ) = reopened
            .lock()
            .query_row(
                "SELECT event_uid, installation_id, service_instance_id, client,
                            session_id, turn_id, trace_id, identity_status
                     FROM transform_log WHERE verb='compress'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert!(row.0.starts_with("legacy-transform."));
        assert_eq!(row.1, None);
        assert_eq!(row.2, None);
        assert_eq!(row.3, "legacy");
        assert!(row.4.starts_with("session-"));
        assert!(row.5.starts_with("turn-"));
        assert!(row.6.starts_with("trace-"));
        assert_eq!(row.7, "legacy_unattributed");
    }

    #[test]
    fn inject_count_migration_applies_to_existing_db() {
        let db = MemDb::open_in_memory();
        let n: i64 = db
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('memories') WHERE name='inject_count'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "inject_count column must exist after migrate");
    }

    #[test]
    fn v20_adds_identical_integer_ms_lifecycle_columns_to_memory_and_quarantine() {
        let db = MemDb::open_in_memory();
        let conn = db.lock();
        let expected = vec![
            ("lifecycle_state", "TEXT", 1, Some("'active'")),
            ("effective_from_ms", "INTEGER", 0, None),
            ("effective_until_ms", "INTEGER", 0, None),
            ("expires_at_ms", "INTEGER", 0, None),
            ("review_after_ms", "INTEGER", 0, None),
            ("superseded_by", "TEXT", 0, None),
            ("priority_class", "TEXT", 1, Some("'normal'")),
            ("confidence", "REAL", 0, None),
            ("confidence_basis", "TEXT", 0, None),
        ];

        for table in ["memories", "memory_quarantine"] {
            let columns = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .and_then(|mut statement| {
                    statement
                        .query_map([], |row| {
                            Ok((
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, i64>(3)?,
                                row.get::<_, Option<String>>(4)?,
                            ))
                        })
                        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                })
                .unwrap();

            for (name, ty, not_null, default) in &expected {
                assert!(
                    columns.iter().any(|column| {
                        column.0 == *name
                            && column.1 == *ty
                            && column.2 == *not_null
                            && column.3.as_deref() == *default
                    }),
                    "{table}.{name} must be {ty} not_null={not_null} default={default:?}; have: {columns:?}"
                );
            }
        }

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn v20_backout_restores_v19_shape_transactionally() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("v20-backout.db");
        let db = MemDb::open(&path).unwrap();
        drop(db);
        backout_v20_to_v19(&path).unwrap();
        let connection = Connection::open(&path).unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 19);
        for table in ["memories", "memory_quarantine"] {
            let count: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name='lifecycle_state'"),
                [],
                |row| row.get(0),
            ).unwrap();
            assert_eq!(count, 0, "{table} retains no v20 lifecycle columns");
        }
    }

    #[test]
    fn lifecycle_event_migration_is_content_free_and_indexed() {
        let db = MemDb::open_in_memory();
        let conn = db.lock();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
        let columns = conn
            .prepare("PRAGMA table_info(memory_event_log)")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(1))
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            })
            .unwrap();
        assert_eq!(
            columns,
            vec![
                "event_id",
                "ts",
                "event_kind",
                "memory_id",
                "surface",
                "session_id",
                "trace_id",
                "scope_id",
                "quantity",
                "meta",
                "event_uid",
                "installation_id",
                "client",
                "turn_id",
                "artifact_id",
                "identity_status",
            ]
        );
        assert!(!columns.iter().any(|column| column == "content"));
        for required in [
            "event_uid",
            "installation_id",
            "client",
            "turn_id",
            "artifact_id",
            "identity_status",
        ] {
            assert!(columns.iter().any(|column| column == required));
        }
        let identity_columns = conn
            .prepare("PRAGMA table_info(memory_identity)")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(1))
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            })
            .unwrap();
        assert_eq!(
            identity_columns,
            vec![
                "memory_id",
                "artifact_id",
                "origin_event_uid",
                "installation_id",
                "client",
                "session_id",
                "turn_id",
                "trace_id",
                "identity_status",
                "created_at",
            ]
        );
        let indexes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='index' AND tbl_name='memory_event_log'
                   AND name IN ('idx_memory_event_memory_ts', 'idx_memory_event_surface_ts')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexes, 2);
        let policy_columns = conn
            .prepare("PRAGMA table_info(context_policy_assignment)")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(1))
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            })
            .unwrap();
        assert_eq!(
            policy_columns,
            vec![
                "session_id",
                "surface",
                "policy_version",
                "cohort",
                "assigned_at",
                "task_class",
            ]
        );
    }

    #[test]
    fn reopen_backfills_content_free_identity_for_legacy_memories() {
        let path = std::env::temp_dir().join(format!(
            "crypt-legacy-identity-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&path);
        {
            let db = MemDb::open(&path).unwrap();
            db.lock()
                .execute(
                    "INSERT INTO memories
                     (id, tier, content, keywords, score, created_at, access_count)
                     VALUES ('global/legacy-secret-name', 'semantic', 'secret body', '[]',
                             1.0, '2026-07-21T00:00:00Z', 0)",
                    [],
                )
                .unwrap();
        }

        let reopened = MemDb::open(&path).unwrap();
        let identity = reopened
            .lock()
            .query_row(
                "SELECT artifact_id, origin_event_uid, installation_id, client,
                        session_id, turn_id, trace_id, identity_status
                 FROM memory_identity WHERE memory_id='global/legacy-secret-name'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                },
            )
            .unwrap();
        assert!(identity.0.starts_with("memory."));
        assert!(identity.1.starts_with("legacy-memory."));
        assert_eq!(identity.2, None);
        assert_eq!(identity.3, "legacy");
        assert!(identity.4.starts_with("session-"));
        assert!(identity.5.starts_with("turn-"));
        assert!(identity.6.starts_with("trace-"));
        assert_eq!(identity.7, "legacy_unattributed");
        let encoded = serde_json::to_string(&identity).unwrap();
        assert!(!encoded.contains("legacy-secret-name"));
        assert!(!encoded.contains("secret body"));
        drop(reopened);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn malformed_legacy_schema_fails_migration_without_advancing_version() {
        let path = std::env::temp_dir().join(format!(
            "crypt-bad-migration-{}-{}.db",
            std::process::id(),
            crate::time::now_iso().replace([':', '.'], "-")
        ));
        let _ = std::fs::remove_file(&path);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE memories (id TEXT PRIMARY KEY); PRAGMA user_version = 0;",
            )
            .unwrap();
        }

        let err = MemDb::open(&path)
            .err()
            .expect("invalid schema must fail loudly");
        assert!(
            err.to_string().contains("memories"),
            "unexpected error: {err}"
        );
        let conn = rusqlite::Connection::open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 0, "failed migration must not advance version");
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn migrations_record_a_nonzero_schema_version() {
        let db = MemDb::open_in_memory();
        let version: i64 = db
            .lock()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert!(version > 0);
    }

    #[test]
    fn schema_v10_backout_restores_quarantine_before_resetting_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backout.db");
        let db = MemDb::open(&path).unwrap();
        {
            let conn = db.lock();
            conn.execute(
                "INSERT INTO memory_quarantine
                    (id,tier,content,keywords,score,created_at,updated_at,access_count,scope_id,
                     inject_count,source_ids,quarantined_at,reason)
                 VALUES ('global/restored','\"Semantic\"','body','[]',0.5,'t','t',0,'global',0,
                         '[]','t','low_effectiveness')",
                [],
            )
            .unwrap();
        }
        drop(db);

        assert_eq!(backout_v10_to_v9(&path).unwrap(), 1);
        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let restored: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE id='global/restored'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let quarantine_table: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='memory_quarantine'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((version, restored, quarantine_table), (9, 1, 0));
    }

    #[test]
    fn schema_v10_backout_aborts_without_data_loss_on_id_collision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backout-collision.db");
        let db = MemDb::open(&path).unwrap();
        {
            let conn = db.lock();
            conn.execute(
                "INSERT INTO memories
                    (id,tier,content,keywords,score,created_at,updated_at,access_count,scope_id,
                     inject_count,source_ids)
                 VALUES ('global/collision','\"Semantic\"','active','[]',0.5,'t','t',0,
                         'global',0,'[]')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO memory_quarantine
                    (id,tier,content,keywords,score,created_at,updated_at,access_count,scope_id,
                     inject_count,source_ids,quarantined_at,reason)
                 VALUES ('global/collision','\"Semantic\"','quarantined','[]',0.5,'t','t',0,
                         'global',0,'[]','t','low_effectiveness')",
                [],
            )
            .unwrap();
        }
        drop(db);

        assert!(backout_v10_to_v9(&path).is_err());
        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let active: String = conn
            .query_row(
                "SELECT content FROM memories WHERE id='global/collision'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let quarantined: String = conn
            .query_row(
                "SELECT content FROM memory_quarantine WHERE id='global/collision'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            (version, active, quarantined),
            (10, "active".into(), "quarantined".into())
        );
    }

    #[test]
    fn bundled_sqlite_contains_wal_reset_fix() {
        let db = MemDb::open_in_memory();
        let version: String = db
            .lock()
            .query_row("SELECT sqlite_version()", [], |row| row.get(0))
            .unwrap();
        let parts = version
            .split('.')
            .map(|part| part.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert!(
            parts.as_slice() >= &[3, 51, 3],
            "SQLite {version} is too old"
        );
    }

    #[test]
    fn migration_backfills_updated_at_from_created_at() {
        let path = std::env::temp_dir().join(format!(
            "crypt-updated-at-{}-{}.db",
            std::process::id(),
            crate::time::now_millis()
        ));
        let _ = std::fs::remove_file(&path);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE memories (
                id TEXT PRIMARY KEY, tier TEXT NOT NULL, content TEXT NOT NULL,
                keywords TEXT NOT NULL, score REAL NOT NULL, created_at TEXT NOT NULL,
                access_count INTEGER NOT NULL, embedding BLOB, embedding_q BLOB,
                scope_id TEXT NOT NULL DEFAULT 'global', inject_count INTEGER NOT NULL DEFAULT 0,
                content_hash TEXT, embed_model TEXT, source_ids TEXT NOT NULL DEFAULT '[]'
             );
             INSERT INTO memories
             (id, tier, content, keywords, score, created_at, access_count, scope_id)
             VALUES ('legacy-updated', '\"Semantic\"', 'body', '[]', 0.5,
                     '2026-01-02T03:04:05Z', 0, 'global');
             PRAGMA user_version = 4;",
        )
        .unwrap();
        drop(conn);
        let db = MemDb::open(&path).unwrap();
        let updated_at: String = db
            .lock()
            .query_row(
                "SELECT updated_at FROM memories WHERE id='legacy-updated'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(updated_at, "2026-01-02T03:04:05Z");
        drop(db);
        let _ = std::fs::remove_file(&path);
    }

    /// 2026-07-09 (add-now plan, Phase 1): client attribution columns must be present after
    /// migrate so an older DB upgraded in place keeps working. Without the test, an ALTER-TABLE
    /// typo would only surface at the first /recall on a real machine.
    #[test]
    fn client_attribution_columns_present_after_migrate() {
        let db = MemDb::open_in_memory();
        let cols: Vec<String> = db
            .lock()
            .prepare("SELECT name FROM pragma_table_info('recall_log') ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .flatten()
            .collect();
        for required in [
            "client",
            "session_id",
            "cwd_scope",
            "hook_event",
            "trace_id",
            "client_visibility",
        ] {
            assert!(
                cols.iter().any(|c| c == required),
                "recall_log missing required column {required} after migrate; have: {cols:?}"
            );
        }
    }

    /// 2026-07-09 (add-now plan, Phase 1): nulls/defaults on attribution columns — older call
    /// sites that pass only the original 8 args should still produce rows that read cleanly
    /// back through SELECT (the metric queries GROUP BY client and must see 'unknown' rows).
    #[test]
    fn log_recall_default_attribution_when_caller_omits() {
        let db = MemDb::open_in_memory();
        db.log_recall(
            "2026-07-09T00:00:00Z",
            Some("D--Claude"),
            100,
            3,
            900,
            240,
            "serve",
            Some("first 120 chars of the query go here"),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let row: (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
        ) = db
            .lock()
            .query_row(
                "SELECT client, session_id, cwd_scope, hook_event, trace_id, client_visibility \
                     FROM recall_log LIMIT 1",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            row.0, "unknown",
            "client defaults to 'unknown' for backfill"
        );
        assert!(row
            .1
            .as_deref()
            .is_some_and(|value| value.starts_with("session-")));
        assert!(row.2.is_none(), "cwd_scope stays NULL");
        assert!(row.3.is_none(), "hook_event stays NULL");
        assert!(row
            .4
            .as_deref()
            .is_some_and(|value| value.starts_with("trace-")));
        assert_eq!(row.5, "all", "client_visibility defaults to 'all'");
    }

    /// 2026-07-09 (add-now plan, Phase 1): per-client counts group serve rows by attribution.
    /// The agent-effectiveness numbers stay on source=serve, but now we can answer
    /// "did ClaudeMM and Codex both produce rows this session?".
    #[test]
    fn client_recall_counts_groups_by_attribution() {
        let db = MemDb::open_in_memory();
        for (client, n) in [("claude", 2u64), ("claudemm", 3u64), ("codex", 1u64)] {
            for _ in 0..n {
                db.log_recall(
                    "2026-07-09T00:00:00Z",
                    Some("D--Claude"),
                    50,
                    1,
                    200,
                    80,
                    "serve",
                    Some("q"),
                    Some(client),
                    Some("sess-1"),
                    Some("D--Claude"),
                    Some("UserPromptSubmit"),
                    Some("trace-1"),
                    Some("all"),
                );
            }
        }
        // A CLI row must NOT show up in the serve-only client count.
        db.log_recall(
            "2026-07-09T00:00:00Z",
            None,
            50,
            1,
            200,
            80,
            "cli",
            Some("q"),
            Some("cli"),
            None,
            None,
            None,
            None,
            None,
        );
        let counts = db.client_recall_counts();
        let by: std::collections::HashMap<String, u64> = counts.into_iter().collect();
        assert_eq!(by.get("claude").copied(), Some(2));
        assert_eq!(by.get("claudemm").copied(), Some(3));
        assert_eq!(by.get("codex").copied(), Some(1));
        assert!(
            !by.contains_key("cli"),
            "source=cli rows must not show in client_recall_counts: {by:?}"
        );
    }

    #[test]
    fn metrics_exclude_empty_queries_but_report_attribution_quality() {
        let db = MemDb::open_in_memory();
        db.log_recall(
            "2026-07-09T00:00:00Z",
            Some("D--Claude"),
            0,
            0,
            0,
            0,
            "serve",
            Some(""),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        db.log_recall(
            "2026-07-09T00:00:01Z",
            Some("D--Claude"),
            50,
            1,
            200,
            80,
            "serve",
            Some("legacy-ish query"),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        db.log_recall(
            "2026-07-09T00:00:02Z",
            Some("D--Claude"),
            60,
            1,
            200,
            80,
            "serve",
            Some("attributed query"),
            Some("claudemm"),
            Some("sess-1"),
            Some("D--Claude"),
            Some("UserPromptSubmit"),
            Some("trace-1"),
            Some("all"),
        );

        let metrics = db.recall_metrics().expect("one meaningful serve row");
        assert_eq!(metrics.recalls, 2, "empty query row excluded");
        let counts: std::collections::HashMap<String, u64> =
            db.client_recall_counts().into_iter().collect();
        assert_eq!(counts.get("unknown").copied(), Some(1));
        assert_eq!(counts.get("claudemm").copied(), Some(1));

        let q = db.attribution_quality();
        assert_eq!(q.attributed_meaningful, 1);
        assert_eq!(q.unknown_total_raw, 2);
        assert_eq!(q.unknown_empty_query, 1);
        assert_eq!(q.unknown_current_meaningful, 1);
        assert_eq!(q.unknown_current_no_metadata, 1);
    }

    #[test]
    fn production_aggregates_exclude_nonproduction_traffic() {
        fn log(
            db: &MemDb,
            ts: &str,
            query_chars: usize,
            preview: Option<&str>,
            client: Option<&str>,
            with_metadata: bool,
            traffic_class: &str,
        ) {
            let hit_count = if query_chars > 0 { 1 } else { 0 };
            let full_chars = if query_chars > 0 { 200 } else { 0 };
            let injected_chars = if query_chars > 0 { 80 } else { 0 };
            let session_id = if with_metadata {
                Some("session-1")
            } else {
                None
            };
            let cwd_scope = if with_metadata {
                Some("D--Claude")
            } else {
                None
            };
            let hook_event = if with_metadata {
                Some("UserPromptSubmit")
            } else {
                None
            };
            let trace_id = if with_metadata { Some("trace-1") } else { None };
            db.log_recall_with_replay(
                ts,
                Some("D--Claude"),
                query_chars,
                hit_count,
                full_chars,
                injected_chars,
                "serve",
                preview,
                client,
                session_id,
                cwd_scope,
                hook_event,
                trace_id,
                Some("all"),
                traffic_class,
                "[]",
                "[]",
            );
        }

        let db = MemDb::open_in_memory();
        log(
            &db,
            "2026-07-09T00:00:00Z",
            0,
            Some(""),
            None,
            false,
            "production",
        );
        log(
            &db,
            "2026-07-09T00:00:01Z",
            50,
            Some("current unknown"),
            None,
            false,
            "production",
        );
        log(
            &db,
            "2026-07-09T00:00:02Z",
            60,
            Some("attributed"),
            Some("claude"),
            true,
            "production",
        );
        log(
            &db,
            "2026-07-09T00:00:03Z",
            40,
            None,
            None,
            false,
            "production",
        );

        log(
            &db,
            "1999-01-01T00:00:00Z",
            0,
            Some(""),
            None,
            false,
            "evaluation",
        );
        log(
            &db,
            "2099-01-01T00:00:00Z",
            40,
            None,
            None,
            false,
            "evaluation",
        );
        log(
            &db,
            "2099-01-01T00:00:01Z",
            50,
            Some("evaluation unknown"),
            None,
            false,
            "evaluation",
        );
        log(
            &db,
            "2099-01-01T00:00:02Z",
            60,
            Some("evaluation attributed"),
            Some("evaluation-client"),
            true,
            "evaluation",
        );

        let metrics = db.recall_metrics().expect("production recalls");
        assert_eq!(metrics.recalls, 3);
        assert_eq!(metrics.hits, 3);
        assert_eq!(metrics.full_chars, 600);
        assert_eq!(metrics.injected_chars, 240);
        assert_eq!(metrics.first_ts, "2026-07-09T00:00:01Z");
        assert_eq!(metrics.last_ts, "2026-07-09T00:00:03Z");

        let counts: std::collections::HashMap<String, u64> =
            db.client_recall_counts().into_iter().collect();
        assert_eq!(counts.get("unknown").copied(), Some(1));
        assert_eq!(counts.get("claude").copied(), Some(1));
        assert!(!counts.contains_key("evaluation-client"));

        let quality = db.attribution_quality();
        assert_eq!(quality.attributed_meaningful, 1);
        assert_eq!(quality.unknown_total_raw, 3);
        assert_eq!(quality.unknown_legacy_no_preview, 1);
        assert_eq!(quality.unknown_empty_query, 1);
        assert_eq!(quality.unknown_current_meaningful, 1);
        assert_eq!(quality.unknown_current_no_metadata, 1);
        assert_eq!(
            quality.unknown_first_ts.as_deref(),
            Some("2026-07-09T00:00:00Z")
        );
        assert_eq!(
            quality.unknown_last_ts.as_deref(),
            Some("2026-07-09T00:00:03Z")
        );
    }
}
