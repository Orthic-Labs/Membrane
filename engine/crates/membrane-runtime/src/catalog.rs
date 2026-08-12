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

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// Schema-version of the catalog. Migrations are additive (`ALTER TABLE ADD` /
/// `CREATE INDEX`) — never re-shape the Crypt DB, never delete a previously
/// persisted column.
pub const CATALOG_SCHEMA_VERSION: i64 = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogAlternate {
    pub path: PathBuf,
    pub status: String,
    pub schema_version: Option<i64>,
    pub catalog_installation_id: Option<String>,
    pub duplicate_catalog_identity: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStartupReport {
    pub canonical_path: PathBuf,
    pub status: String,
    pub degradation_reason: Option<String>,
    pub journal_mode: String,
    pub schema_version: i64,
    pub main_bytes: Option<u64>,
    pub wal_bytes: u64,
    pub catalog_installation_id: String,
    pub same_schema_alternate_count: usize,
    pub alternate_catalogs: Vec<CatalogAlternate>,
}

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
    startup_report: CatalogStartupReport,
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
        let startup_report = startup_report(&conn, &path, &known_catalog_candidates(&path))?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path,
            startup_report,
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
        let startup_report =
            startup_report(&conn, Path::new(":memory:"), &[]).expect("inventory in-memory catalog");
        Self {
            conn: Arc::new(Mutex::new(conn)),
            path: PathBuf::from(":memory:"),
            startup_report,
        }
    }

    /// Path to the catalog SQLite file. Surfaced in `/health` and tests.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn startup_report(&self) -> &CatalogStartupReport {
        &self.startup_report
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

const CATALOG_METADATA_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS catalog_metadata (
    singleton               INTEGER PRIMARY KEY CHECK (singleton = 1),
    catalog_installation_id TEXT NOT NULL UNIQUE,
    created_at_unix         INTEGER NOT NULL
);
";

fn new_catalog_installation_id() -> rusqlite::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|error| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(error.to_string())))
    })?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15]
    ))
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
            2 => {
                tx.execute_batch(CATALOG_METADATA_SCHEMA)?;
                tx.execute(
                    "INSERT OR IGNORE INTO catalog_metadata
                        (singleton, catalog_installation_id, created_at_unix)
                     VALUES (1, ?1, ?2)",
                    params![new_catalog_installation_id()?, ContextCatalog::now_unix()],
                )?;
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

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn file_bytes(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
}

fn catalog_installation_id(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row(
        "SELECT catalog_installation_id FROM catalog_metadata WHERE singleton = 1",
        [],
        |row| row.get(0),
    )
}

fn same_file(left: &Path, right: &Path) -> bool {
    left == right
        || match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
}

fn known_catalog_candidates(canonical: &Path) -> Vec<PathBuf> {
    let mut candidates = BTreeSet::new();
    if let Some(value) = std::env::var_os("RIGHTCONTEXT_CATALOG") {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            candidates.insert(path);
        }
    }
    if let Some(value) = std::env::var_os("CONTEXT_HOME") {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            candidates.insert(path.join("catalog.db"));
        }
    }
    if let Some(value) = std::env::var_os("CRYPT_DB") {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            if let Some(parent) = path.parent() {
                candidates.insert(parent.join("catalog.db"));
            }
        }
    }
    if let Some(value) = std::env::var_os("WORKSPACE_ROOT") {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            candidates.insert(path.join("catalog.db"));
            candidates.insert(path.join("tools/.cache/memory/catalog.db"));
        }
    }
    for value in [std::env::var_os("HOME"), std::env::var_os("USERPROFILE")]
        .into_iter()
        .flatten()
    {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            candidates.insert(path.join(".claude/rightcontext/catalog.db"));
        }
    }
    candidates
        .into_iter()
        .filter(|path| !same_file(path, canonical))
        .collect()
}

fn inventory_catalog_alternates(
    canonical: &Path,
    canonical_id: &str,
    candidates: &[PathBuf],
) -> Vec<CatalogAlternate> {
    let mut seen = BTreeSet::new();
    let mut alternates = Vec::new();
    for path in candidates {
        if !path.is_absolute()
            || same_file(path, canonical)
            || !path.is_file()
            || !seen.insert(path.clone())
        {
            continue;
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let alternate = match Connection::open_with_flags(path, flags) {
            Ok(conn) => match conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            {
                Ok(schema_version) => {
                    let alternate_id = if schema_version >= 2 {
                        catalog_installation_id(&conn).ok()
                    } else {
                        None
                    };
                    let identity_unavailable =
                        schema_version == CATALOG_SCHEMA_VERSION && alternate_id.is_none();
                    CatalogAlternate {
                        path: path.clone(),
                        status: if identity_unavailable {
                            "unavailable".to_string()
                        } else if schema_version == CATALOG_SCHEMA_VERSION {
                            "same_schema".to_string()
                        } else {
                            "different_schema".to_string()
                        },
                        schema_version: Some(schema_version),
                        duplicate_catalog_identity: alternate_id.as_deref() == Some(canonical_id),
                        catalog_installation_id: alternate_id,
                        reason: identity_unavailable
                            .then(|| "catalog_identity_unavailable".to_string()),
                    }
                }
                Err(error) => CatalogAlternate {
                    path: path.clone(),
                    status: "unavailable".to_string(),
                    schema_version: None,
                    catalog_installation_id: None,
                    duplicate_catalog_identity: false,
                    reason: Some(format!("read_user_version: {error}")),
                },
            },
            Err(error) => CatalogAlternate {
                path: path.clone(),
                status: "unavailable".to_string(),
                schema_version: None,
                catalog_installation_id: None,
                duplicate_catalog_identity: false,
                reason: Some(format!("read_only_open: {error}")),
            },
        };
        alternates.push(alternate);
    }
    alternates.sort_by(|left, right| left.path.cmp(&right.path));
    alternates
}

fn startup_report(
    conn: &Connection,
    path: &Path,
    candidates: &[PathBuf],
) -> rusqlite::Result<CatalogStartupReport> {
    let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    let schema_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let catalog_installation_id = catalog_installation_id(conn)?;
    let alternate_catalogs =
        inventory_catalog_alternates(path, &catalog_installation_id, candidates);
    let same_schema_alternate_count = alternate_catalogs
        .iter()
        .filter(|alternate| alternate.schema_version == Some(CATALOG_SCHEMA_VERSION))
        .count();
    let journal_is_expected = journal_mode == "wal" || path == Path::new(":memory:");
    let alternate_degradation = alternate_catalogs.iter().find_map(|alternate| {
        if alternate.duplicate_catalog_identity {
            Some("duplicate_catalog_identity")
        } else if alternate.status == "unavailable" {
            Some("alternate_catalog_unavailable")
        } else {
            None
        }
    });
    let degradation_reason = if !journal_is_expected {
        Some("journal_mode_not_wal".to_string())
    } else {
        alternate_degradation.map(str::to_string)
    };
    Ok(CatalogStartupReport {
        canonical_path: path.to_path_buf(),
        status: if degradation_reason.is_some() {
            "degraded"
        } else {
            "ok"
        }
        .to_string(),
        degradation_reason,
        journal_mode,
        schema_version,
        main_bytes: (path != Path::new(":memory:"))
            .then(|| file_bytes(path))
            .flatten(),
        wal_bytes: if path == Path::new(":memory:") {
            0
        } else {
            file_bytes(&sidecar_path(path, "-wal")).unwrap_or(0)
        },
        catalog_installation_id,
        same_schema_alternate_count,
        alternate_catalogs,
    })
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
    let startup_report = catalog.startup_report();
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
                "startup": startup_report,
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
            "startup": startup_report,
        }),
        Err(_) => serde_json::json!({
            "schemaVersion": CATALOG_SCHEMA_VERSION,
            "path": path_str,
            "status": "error",
            "receipts": serde_json::Value::Null,
            "retrievalEvents": serde_json::Value::Null,
            "activeGrants": serde_json::Value::Null,
            "cryptDbUntouched": true,
            "startup": startup_report,
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
        // Catalog DB exists and has the current schema stamp.
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
        assert!(tables.iter().any(|t| t == "catalog_metadata"));
    }

    #[test]
    fn catalog_identity_is_stable_and_startup_report_is_content_free() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalog.db");
        let first = ContextCatalog::open(&path).unwrap();
        let first_id = first.startup_report().catalog_installation_id.clone();
        assert_eq!(first.startup_report().canonical_path, path);
        assert_eq!(first.startup_report().journal_mode, "wal");
        assert_eq!(
            first.startup_report().schema_version,
            CATALOG_SCHEMA_VERSION
        );
        assert_eq!(first.startup_report().status, "ok");
        assert!(first.startup_report().main_bytes.is_some());
        drop(first);

        let reopened = ContextCatalog::open(&path).unwrap();
        assert_eq!(reopened.startup_report().catalog_installation_id, first_id);
        let health = health_snapshot(&reopened);
        assert_eq!(health["startup"]["catalogInstallationId"], first_id);
        assert_eq!(
            health["startup"]["canonicalPath"],
            path.to_string_lossy().as_ref()
        );
        assert_eq!(health["startup"]["journalMode"], "wal");
    }

    #[test]
    fn v1_catalog_migrates_additively_without_losing_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalog.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO identities (client, first_seen_unix, last_seen_unix, capability)
             VALUES ('client-1', 1, 2, 'standard')",
            [],
        )
        .unwrap();
        drop(conn);

        let catalog = ContextCatalog::open(&path).unwrap();
        let identity_count: i64 = catalog
            .lock()
            .query_row("SELECT COUNT(*) FROM identities", [], |row| row.get(0))
            .unwrap();
        assert_eq!(identity_count, 1);
        assert_eq!(catalog.startup_report().schema_version, 2);
        assert!(!catalog.startup_report().catalog_installation_id.is_empty());
    }

    #[test]
    fn alternate_inventory_is_read_only_and_distinguishes_cloned_identity() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().join("canonical/catalog.db");
        let alternate = dir.path().join("alternate/catalog.db");
        let missing = dir.path().join("missing/catalog.db");
        let catalog = ContextCatalog::open(&canonical).unwrap();
        let catalog_id = catalog.startup_report().catalog_installation_id.clone();
        catalog
            .lock()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        drop(catalog);
        std::fs::create_dir_all(alternate.parent().unwrap()).unwrap();
        std::fs::copy(&canonical, &alternate).unwrap();

        let entries = inventory_catalog_alternates(
            &canonical,
            &catalog_id,
            &[alternate.clone(), missing.clone()],
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, alternate);
        assert_eq!(entries[0].status, "same_schema");
        assert!(entries[0].duplicate_catalog_identity);
        assert_eq!(
            entries[0].catalog_installation_id.as_deref(),
            Some(catalog_id.as_str())
        );
        assert!(!missing.exists());
        assert!(!missing.parent().unwrap().exists());
    }

    #[test]
    fn same_schema_alternate_without_identity_is_typed_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().join("canonical/catalog.db");
        let malformed = dir.path().join("malformed/catalog.db");
        let catalog = ContextCatalog::open(&canonical).unwrap();
        let catalog_id = catalog.startup_report().catalog_installation_id.clone();
        std::fs::create_dir_all(malformed.parent().unwrap()).unwrap();
        let conn = Connection::open(&malformed).unwrap();
        conn.pragma_update(None, "user_version", CATALOG_SCHEMA_VERSION)
            .unwrap();
        drop(conn);

        let entries = inventory_catalog_alternates(&canonical, &catalog_id, &[malformed]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, "unavailable");
        assert_eq!(
            entries[0].reason.as_deref(),
            Some("catalog_identity_unavailable")
        );
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
