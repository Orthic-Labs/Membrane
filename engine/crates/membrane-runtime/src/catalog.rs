//! Central context catalog — the G3B storage lane.
//!
//! Strict storage boundary (locked 2026-07-12, dispatch §G3B):
//!
//! ```text
//! Crypt DB               durable memories only
//! <context-home>/catalog.db identities, handles, generations, scope grants,
//!                           retrieval events, capabilities, receipts
//! <context-home>/repos/...  separately attachable graph generations
//! ```
//!
//! The catalog NEVER reuses or migrates the Crypt durable-memory DB. It owns a
//! separate SQLite file, lives in a dedicated directory, runs its own
//! additive migrations via `PRAGMA user_version`, and exposes a small,
//! planner-specific surface (issuance / lookup / revoke for scope grants, plus
//! append-only event recording for retrieval events and acceptance of planner
//! receipts). Content-free by construction — receipts are stored by id only,
//! never with raw prompt or repository text.
//!
//! The catalog is `failure-isolated` from the existing memory lanes: a planner
//! error on the catalog path must never bring down `get`/`put`. The serve
//! router enforces this by passing a `MemDb` and a `ContextCatalog` as
//! independent handles; the catalog connection is wrapped in its own mutex so a
//! poisoned/panicked helper degrades without poisoning the route's other DB.

use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// Schema-version of the catalog. Migrations are additive (`ALTER TABLE ADD` /
/// `CREATE INDEX`) — never re-shape the Crypt DB, never delete a previously
/// persisted column.
pub const CATALOG_SCHEMA_VERSION: i64 = 1;

/// Typed resolver for one absolute catalog identity shared with provider shims.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CatalogPathError {
    #[error("catalog path binding {binding} must be absolute: {value}")]
    Relative {
        binding: &'static str,
        value: String,
    },
    #[error(
        "catalog path is unbound: set RIGHTCONTEXT_CATALOG, CONTEXT_HOME, CRYPT_DB, or WORKSPACE_ROOT"
    )]
    MissingBinding,
}

fn absolute_binding(
    binding: &'static str,
    value: Option<std::ffi::OsString>,
) -> Result<Option<PathBuf>, CatalogPathError> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(&value);
    if !path.is_absolute() {
        return Err(CatalogPathError::Relative {
            binding,
            value: path.to_string_lossy().into_owned(),
        });
    }
    Ok(Some(path))
}

pub fn resolve_catalog_path_from(
    rightcontext_catalog: Option<std::ffi::OsString>,
    context_home: Option<std::ffi::OsString>,
    crypt_db: Option<std::ffi::OsString>,
    workspace_root: Option<std::ffi::OsString>,
) -> Result<PathBuf, CatalogPathError> {
    if let Some(path) = absolute_binding("RIGHTCONTEXT_CATALOG", rightcontext_catalog)? {
        return Ok(path);
    }
    if let Some(path) = absolute_binding("CONTEXT_HOME", context_home)? {
        return Ok(path.join("catalog.db"));
    }
    if let Some(path) = absolute_binding("CRYPT_DB", crypt_db)? {
        return Ok(path
            .parent()
            .expect("absolute database path has a parent")
            .join("catalog.db"));
    }
    if let Some(path) = absolute_binding("WORKSPACE_ROOT", workspace_root)? {
        return Ok(path.join("tools/.cache/memory/catalog.db"));
    }
    Err(CatalogPathError::MissingBinding)
}

/// Resolve one canonical catalog path without current-directory fallback.
pub fn default_catalog_path() -> Result<PathBuf, CatalogPathError> {
    resolve_catalog_path_from(
        std::env::var_os("RIGHTCONTEXT_CATALOG"),
        std::env::var_os("CONTEXT_HOME"),
        std::env::var_os("CRYPT_DB"),
        std::env::var_os("WORKSPACE_ROOT"),
    )
}

/// Handle to the central context catalog. Cheap to clone (Arc-shared); all
/// callers serialise through the inner mutex.
#[derive(Clone)]
pub struct ContextCatalog {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl ContextCatalog {
    /// Open (or create) the catalog at `path`, WAL + busy timeout, run
    /// migrations. The path is recorded so health/output can echo it; the
    /// Crypt DB is never touched.
    pub fn open<P: AsRef<Path>>(path: P) -> rusqlite::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
        }
        let mut conn = Connection::open(&path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;",
        )?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path,
        })
    }

    /// In-memory ephemeral catalog for tests. Mirrors MemDb::open_in_memory but
    /// lives in a separate connection so it can never accidentally write to
    /// the Crypt DB.
    pub fn open_in_memory() -> Self {
        let mut conn = Connection::open_in_memory().expect("open in-memory catalog");
        conn.execute_batch("PRAGMA busy_timeout=5000; PRAGMA temp_store=MEMORY;")
            .expect("enable in-memory catalog timeout");
        migrate(&mut conn).expect("migrate in-memory catalog");
        Self {
            conn: Arc::new(Mutex::new(conn)),
            path: PathBuf::from(":memory:"),
        }
    }

    /// Path to the catalog SQLite file. Surfaced in `/health` and tests.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Lock the connection for a unit of work. Poison-recovering so a panicked
    /// planner write cannot take down the whole service (mirrors the
    /// MemDb::lock contract).
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Server-issued monotonic timestamp seconds for fixtures.
    pub fn now_unix() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

// ---- Migrations ----------------------------------------------------------
//
// Catalogs are append-only additive. The dispatcher commits to "additive only,
// never rewrite the Crypt DB". Each migration step opens a transaction,
// applies its change, then bumps `PRAGMA user_version`. Migrations must remain
// idempotent so a partial application re-running the helper upgrades in place.

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS identities (
    client          TEXT PRIMARY KEY,
    first_seen_unix INTEGER NOT NULL,
    last_seen_unix  INTEGER NOT NULL,
    capability      TEXT NOT NULL DEFAULT 'standard'
);
CREATE TABLE IF NOT EXISTS handles (
    handle          TEXT PRIMARY KEY,
    client          TEXT NOT NULL,
    -- a stable per-session handle assigned by the gateway
    issued_at_unix  INTEGER NOT NULL,
    metadata        TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS generations (
    generation_id   TEXT PRIMARY KEY,
    repository_id   TEXT NOT NULL,
    source_root     TEXT NOT NULL,
    captured_at_unix INTEGER NOT NULL,
    manifest_digest TEXT NOT NULL,
    -- append-only lifecycle (no UPDATE allowed)
    status          TEXT NOT NULL CHECK (status IN ('fresh','stale','quarantined'))
                    DEFAULT 'fresh'
);
CREATE TABLE IF NOT EXISTS scope_grants (
    id              TEXT PRIMARY KEY,
    issuer          TEXT NOT NULL,
    client          TEXT NOT NULL,
    repository_ids  TEXT NOT NULL,
    permitted_edges TEXT NOT NULL,
    task_id         TEXT NOT NULL,
    session_id      TEXT NOT NULL,
    issued_at_unix  INTEGER NOT NULL,
    expires_at_unix INTEGER NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('active','revoked','expired'))
                    DEFAULT 'active',
    nonce           TEXT NOT NULL,
    manifest_digest TEXT NOT NULL,
    revoked_at_unix INTEGER
);
CREATE INDEX IF NOT EXISTS idx_scope_grants_client
    ON scope_grants(client, status);
CREATE TABLE IF NOT EXISTS retrieval_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_unix         INTEGER NOT NULL,
    trace_id        TEXT NOT NULL,
    client          TEXT NOT NULL,
    mode            TEXT NOT NULL,
    provider        TEXT NOT NULL,
    provider_status TEXT NOT NULL,
    fallback_mode   TEXT NOT NULL,
    degradation_reason TEXT NOT NULL,
    source_generation TEXT,
    candidate_count INTEGER NOT NULL,
    admitted_count  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_retrieval_events_trace
    ON retrieval_events(trace_id);
CREATE TABLE IF NOT EXISTS capabilities (
    capability      TEXT PRIMARY KEY,
    description     TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS receipts (
    receipt_id      TEXT PRIMARY KEY,
    trace_id        TEXT NOT NULL,
    candidate_id    TEXT NOT NULL,
    decision        TEXT NOT NULL,
    reason          TEXT NOT NULL,
    provider        TEXT NOT NULL,
    provider_status TEXT NOT NULL,
    fallback_mode   TEXT NOT NULL,
    degradation_reason TEXT NOT NULL,
    written_at_unix  INTEGER NOT NULL,
    -- receipt row is content-free: text is forbidden by schema, no column for it.
    bytes_sha256    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_receipts_trace
    ON receipts(trace_id);
";

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

fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > CATALOG_SCHEMA_VERSION {
        return Err(rusqlite::Error::InvalidQuery);
    }
    while version < CATALOG_SCHEMA_VERSION {
        let next = version + 1;
        let tx = conn.transaction()?;
        match next {
            1 => {
                tx.execute_batch(SCHEMA)?;
            }
            // Placeholder for additive future columns — handlers go here, never
            // a rewrite of an existing table.
            _ => unreachable!(),
        }
        tx.pragma_update(None, "user_version", next)?;
        tx.commit()?;
        version = next;
    }
    // Touch `add_column` so an unused-warning lint doesn't fail later when a
    // real migration shows up.
    let _ = add_column;
    Ok(())
}

// ---- Scope grants --------------------------------------------------------

/// Server-minted `ScopeGrantV1` mirror. The canonical contract is the JSON
/// schema in `tools/lib/context-contracts.schema.json`; the Rust mirror is the
/// authority for the lookup path in the resident service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeGrant {
    pub id: String,
    pub issuer: String,
    pub client: String,
    pub repository_ids: Vec<String>,
    pub permitted_edge_types: Vec<String>,
    pub task_id: String,
    pub session_id: String,
    pub issued_at_unix: i64,
    pub expires_at_unix: i64,
    pub status: GrantStatus,
    pub nonce: String,
    pub manifest_digest: String,
    pub revoked_at_unix: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantStatus {
    Active,
    Revoked,
    Expired,
}

impl GrantStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            GrantStatus::Active => "active",
            GrantStatus::Revoked => "revoked",
            GrantStatus::Expired => "expired",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "revoked" => GrantStatus::Revoked,
            "expired" => GrantStatus::Expired,
            _ => GrantStatus::Active,
        }
    }
}

/// Issue a new scope grant. The caller provides the derived fields; the
/// catalog stores the row and returns it. Issued grants start `active` and
/// expire automatically on lookup if `expires_at_unix` has elapsed.
#[allow(clippy::too_many_arguments)]
pub fn issue_scope_grant(
    catalog: &ContextCatalog,
    id: &str,
    client: &str,
    repository_ids: &[String],
    permitted_edge_types: &[String],
    task_id: &str,
    session_id: &str,
    ttl_seconds: i64,
    nonce: &str,
    manifest_digest: &str,
) -> rusqlite::Result<ScopeGrant> {
    let now = ContextCatalog::now_unix();
    let expires = now.saturating_add(ttl_seconds.max(1));
    let repo_csv = repository_ids.join(",");
    let edge_csv = permitted_edge_types.join(",");
    let conn = catalog.lock();
    conn.execute(
        "INSERT INTO scope_grants
            (id, issuer, client, repository_ids, permitted_edges,
             task_id, session_id, issued_at_unix, expires_at_unix,
             status, nonce, manifest_digest, revoked_at_unix)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL)",
        params![
            id,
            "rightcontext-gateway",
            client,
            repo_csv,
            edge_csv,
            task_id,
            session_id,
            now,
            expires,
            GrantStatus::Active.as_str(),
            nonce,
            manifest_digest,
        ],
    )?;
    Ok(ScopeGrant {
        id: id.to_string(),
        issuer: "rightcontext-gateway".into(),
        client: client.to_string(),
        repository_ids: repository_ids.to_vec(),
        permitted_edge_types: permitted_edge_types.to_vec(),
        task_id: task_id.to_string(),
        session_id: session_id.to_string(),
        issued_at_unix: now,
        expires_at_unix: expires,
        status: GrantStatus::Active,
        nonce: nonce.to_string(),
        manifest_digest: manifest_digest.to_string(),
        revoked_at_unix: None,
    })
}

fn grant_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScopeGrant> {
    let repo_csv: String = row.get("repository_ids")?;
    let edge_csv: String = row.get("permitted_edges")?;
    let status_str: String = row.get("status")?;
    Ok(ScopeGrant {
        id: row.get("id")?,
        issuer: row.get("issuer")?,
        client: row.get("client")?,
        repository_ids: repo_csv
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        permitted_edge_types: edge_csv
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        task_id: row.get("task_id")?,
        session_id: row.get("session_id")?,
        issued_at_unix: row.get("issued_at_unix")?,
        expires_at_unix: row.get("expires_at_unix")?,
        status: GrantStatus::parse(&status_str),
        nonce: row.get("nonce")?,
        manifest_digest: row.get("manifest_digest")?,
        revoked_at_unix: row.get("revoked_at_unix")?,
    })
}

/// Look up a grant by id. If the grant expired since issue (or has been
/// revoked), returns the observed status — callers fail closed on
/// `Active` checks; expired/revoked permits are explicitly surfaced.
pub fn lookup_grant(catalog: &ContextCatalog, id: &str) -> rusqlite::Result<Option<ScopeGrant>> {
    let conn = catalog.lock();
    let mut stmt = conn.prepare("SELECT * FROM scope_grants WHERE id = ?1")?;
    let grant = stmt.query_row([id], grant_from_row).optional()?;
    if let Some(mut grant) = grant {
        if grant.status == GrantStatus::Active && is_expired(&grant) {
            grant.status = GrantStatus::Expired;
        }
        Ok(Some(grant))
    } else {
        Ok(None)
    }
}

fn is_expired(grant: &ScopeGrant) -> bool {
    ContextCatalog::now_unix() >= grant.expires_at_unix
}

impl ScopeGrant {
    /// Whether a grant can authorise a planner call right now.
    pub fn permits(&self) -> bool {
        self.status == GrantStatus::Active && !is_expired(self)
    }
}

/// Revoke a grant. Idempotent — revoking an already-revoked grant leaves the
/// original revoke timestamp intact.
pub fn revoke_scope_grant(catalog: &ContextCatalog, id: &str) -> rusqlite::Result<bool> {
    let now = ContextCatalog::now_unix();
    let conn = catalog.lock();
    let changed = conn.execute(
        "UPDATE scope_grants
         SET status = ?1, revoked_at_unix = COALESCE(revoked_at_unix, ?2)
         WHERE id = ?3 AND status = 'active'",
        params![GrantStatus::Revoked.as_str(), now, id],
    )?;
    Ok(changed > 0)
}

// ---- Events + receipts (append-only) -------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn record_retrieval_event(
    catalog: &ContextCatalog,
    trace_id: &str,
    client: &str,
    mode: &str,
    provider: &str,
    provider_status: &str,
    fallback_mode: &str,
    degradation_reason: &str,
    source_generation: Option<&str>,
    candidate_count: usize,
    admitted_count: usize,
) -> rusqlite::Result<()> {
    let conn = catalog.lock();
    conn.execute(
        "INSERT INTO retrieval_events
            (ts_unix, trace_id, client, mode, provider, provider_status,
             fallback_mode, degradation_reason, source_generation,
             candidate_count, admitted_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            ContextCatalog::now_unix(),
            trace_id,
            client,
            mode,
            provider,
            provider_status,
            fallback_mode,
            degradation_reason,
            source_generation,
            candidate_count as i64,
            admitted_count as i64
        ],
    )?;
    Ok(())
}

/// Append-only receipt persistence. `bytes_sha256` is the SHA-256 of the
/// serialised receipt payload (used for dedup + audit) — the raw JSON itself
/// stays the caller's responsibility. Content-free by construction.
#[allow(clippy::too_many_arguments)]
pub fn record_receipt(
    catalog: &ContextCatalog,
    receipt_id: &str,
    trace_id: &str,
    candidate_id: &str,
    decision: &str,
    reason: &str,
    provider: &str,
    provider_status: &str,
    fallback_mode: &str,
    degradation_reason: &str,
    bytes_sha256: &str,
) -> rusqlite::Result<()> {
    let conn = catalog.lock();
    conn.execute(
        "INSERT OR IGNORE INTO receipts
            (receipt_id, trace_id, candidate_id, decision, reason,
             provider, provider_status, fallback_mode, degradation_reason,
             written_at_unix, bytes_sha256)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            receipt_id,
            trace_id,
            candidate_id,
            decision,
            reason,
            provider,
            provider_status,
            fallback_mode,
            degradation_reason,
            ContextCatalog::now_unix(),
            bytes_sha256,
        ],
    )?;
    Ok(())
}

pub fn count_receipts(catalog: &ContextCatalog) -> rusqlite::Result<i64> {
    let conn = catalog.lock();
    conn.query_row("SELECT COUNT(*) FROM receipts", [], |row| row.get(0))
}

pub fn count_receipts_for_trace(catalog: &ContextCatalog, trace_id: &str) -> rusqlite::Result<i64> {
    let conn = catalog.lock();
    conn.query_row(
        "SELECT COUNT(*) FROM receipts WHERE trace_id = ?1",
        [trace_id],
        |row| row.get(0),
    )
}

pub fn count_events(catalog: &ContextCatalog) -> rusqlite::Result<i64> {
    let conn = catalog.lock();
    conn.query_row("SELECT COUNT(*) FROM retrieval_events", [], |row| {
        row.get(0)
    })
}

// ---- Health snapshot -----------------------------------------------------

/// Snapshot the catalog reports into `/health`. Content-free and nonblocking by construction.
pub fn health_snapshot(catalog: &ContextCatalog) -> serde_json::Value {
    let path_str = catalog.path().to_string_lossy().into_owned();
    let conn = match catalog.conn.try_lock() {
        Ok(conn) => conn,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => {
            return serde_json::json!({
                "schemaVersion": CATALOG_SCHEMA_VERSION,
                "path": path_str,
                "status": "busy",
                "receipts": serde_json::Value::Null,
                "retrievalEvents": serde_json::Value::Null,
                "activeGrants": serde_json::Value::Null,
                "cryptDbUntouched": true,
            });
        }
    };
    let counts = conn.query_row(
        "SELECT
                (SELECT COUNT(*) FROM receipts),
                (SELECT COUNT(*) FROM retrieval_events),
                (SELECT COUNT(*) FROM scope_grants WHERE status = 'active')",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    );
    match counts {
        Ok((receipts, events, active_grants)) => serde_json::json!({
            "schemaVersion": CATALOG_SCHEMA_VERSION,
            "path": path_str,
            "status": "ok",
            "receipts": receipts,
            "retrievalEvents": events,
            "activeGrants": active_grants,
            "cryptDbUntouched": true,
        }),
        Err(_) => serde_json::json!({
            "schemaVersion": CATALOG_SCHEMA_VERSION,
            "path": path_str,
            "status": "error",
            "receipts": serde_json::Value::Null,
            "retrievalEvents": serde_json::Value::Null,
            "activeGrants": serde_json::Value::Null,
            "cryptDbUntouched": true,
        }),
    }
}

// ---- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_connections_use_memory_temp_store() {
        let catalog = ContextCatalog::open_in_memory();
        let temp_store: i64 = catalog
            .lock()
            .query_row("PRAGMA temp_store", [], |row| row.get(0))
            .unwrap();
        assert_eq!(temp_store, 2);
    }

    #[test]
    fn health_snapshot_returns_busy_without_waiting_for_catalog_lock() {
        let catalog = ContextCatalog::open_in_memory();
        let held_lock = catalog.lock();
        let worker_catalog = catalog.clone();
        let rendezvous = std::sync::Arc::new(std::sync::Barrier::new(2));
        let worker_rendezvous = std::sync::Arc::clone(&rendezvous);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            worker_rendezvous.wait();
            let snapshot = health_snapshot(&worker_catalog);
            sender.send(snapshot).unwrap();
        });

        rendezvous.wait();
        let snapshot = match receiver.recv_timeout(std::time::Duration::from_millis(250)) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                drop(held_lock);
                worker.join().unwrap();
                panic!("health snapshot blocked on the held catalog lock: {error}");
            }
        };
        drop(held_lock);
        worker.join().unwrap();

        assert_eq!(snapshot["status"], "busy");
        assert_eq!(snapshot["receipts"], serde_json::Value::Null);
        assert_eq!(snapshot["retrievalEvents"], serde_json::Value::Null);
        assert_eq!(snapshot["activeGrants"], serde_json::Value::Null);
        assert_eq!(snapshot["cryptDbUntouched"], true);
    }

    #[test]
    fn health_snapshot_does_not_report_zero_counts_as_healthy_on_query_failure() {
        let catalog = ContextCatalog::open_in_memory();
        catalog.lock().execute("DROP TABLE receipts", []).unwrap();

        let snapshot = health_snapshot(&catalog);

        assert_eq!(snapshot["status"], "error");
        assert_eq!(snapshot["receipts"], serde_json::Value::Null);
        assert_eq!(snapshot["retrievalEvents"], serde_json::Value::Null);
        assert_eq!(snapshot["activeGrants"], serde_json::Value::Null);
    }

    #[test]
    fn catalog_opens_without_touching_crypt_db() {
        // Distinct file paths. If the catalog implementation ever tried to open
        // the Crypt DB, one of these paths would receive writes or schema
        // changes — both detectable here.
        let dir = tempfile::tempdir().unwrap();
        let crypt_path = dir.path().join("crypt-engine.db");
        let catalog_path = dir.path().join("catalog.db");
        std::fs::write(&crypt_path, b"CRYPT_DB_FORGE").unwrap();

        let catalog = ContextCatalog::open(&catalog_path).unwrap();
        let _ = catalog; // touch handle

        // Crypt DB must be byte-identical — the catalog never opened it.
        assert_eq!(std::fs::read(&crypt_path).unwrap(), b"CRYPT_DB_FORGE");
        // Catalog DB exists and has the v1 schema stamp.
        assert!(catalog_path.exists());
        let conn = rusqlite::Connection::open(&catalog_path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CATALOG_SCHEMA_VERSION);
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(tables.iter().any(|t| t == "scope_grants"));
        assert!(tables.iter().any(|t| t == "receipts"));
    }

    #[test]
    fn grant_issuance_lookup_expiry_and_revocation() {
        let catalog = ContextCatalog::open_in_memory();
        let grant = issue_scope_grant(
            &catalog,
            "sg-1",
            "claude-mm",
            &["D--Claude".into()],
            &["exact".into(), "lexical".into()],
            "task-a",
            "sess-1",
            30,
            "nonce-abcdef",
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        assert!(grant.permits());
        let looked = lookup_grant(&catalog, "sg-1").unwrap().unwrap();
        assert_eq!(looked.id, "sg-1");
        assert_eq!(looked.client, "claude-mm");
        assert!(looked.permits());
        assert!(revoke_scope_grant(&catalog, "sg-1").unwrap());
        let revoked = lookup_grant(&catalog, "sg-1").unwrap().unwrap();
        assert_eq!(revoked.status, GrantStatus::Revoked);
        assert!(!revoked.permits());
        // Revoking again is idempotent.
        assert!(!revoke_scope_grant(&catalog, "sg-1").unwrap());
    }

    #[test]
    fn grant_with_elapsed_ttl_is_observed_as_expired() {
        let catalog = ContextCatalog::open_in_memory();
        let mut grant = issue_scope_grant(
            &catalog,
            "sg-ttl",
            "claude-mm",
            &["D--Claude".into()],
            &["exact".into()],
            "task-t",
            "sess-t",
            1,
            "nonce-1234",
            "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        )
        .unwrap();
        // Simulate elapsed ttl without sleeping the test process.
        grant.expires_at_unix -= 10;
        assert!(!grant.permits());
    }

    #[test]
    fn receipts_are_appended_and_persisted_content_free() {
        let catalog = ContextCatalog::open_in_memory();
        for n in 0..3 {
            record_receipt(
                &catalog,
                &format!("r-{n}"),
                "trace-a",
                &format!("c-{n}"),
                "admitted",
                "within_global_budget",
                "blueprint",
                "fresh",
                "none",
                "none",
                "abcd",
            )
            .unwrap();
        }
        assert_eq!(count_receipts(&catalog).unwrap(), 3);
        assert_eq!(count_receipts_for_trace(&catalog, "trace-a").unwrap(), 3);
        assert_eq!(count_events(&catalog).unwrap(), 0);

        // Inserting the same receipt_id is idempotent (INSERT OR IGNORE).
        record_receipt(
            &catalog,
            "r-0",
            "trace-a",
            "c-0",
            "admitted",
            "within_global_budget",
            "blueprint",
            "fresh",
            "none",
            "none",
            "abcd",
        )
        .unwrap();
        assert_eq!(count_receipts(&catalog).unwrap(), 3);
    }
}
