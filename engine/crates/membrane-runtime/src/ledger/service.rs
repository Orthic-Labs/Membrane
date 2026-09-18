//! The daemon's sole operational Ledger owner.
//!
//! CLI and MCP are clients of this owner. Lazy index repair is a cache effect
//! under an enrolled source grant, never permission to edit source documents.
//! Resolution tickets retain the original source expectations and task grant;
//! neither a cursor nor an agent-supplied root can increase read authority.

use super::{doc_spine, index, limits::WorkBudget, policy::SourcePolicy, qualification, query,
    resolve::{self, ResolveRequest}, skill_documents, LedgerDb};
use crate::authorization::{self, AuthorizationRequest};
use membrane_protocol::ReadPathV1;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::time::Duration;

pub const SERVICE_VERSION: &str = "ledger.owner.v1";
pub const RESOLVER_VERSION: &str = "ledger.exact-node.v1";
const TICKET_TTL_MS: i64 = 10 * 60 * 1000;
const OWNER_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS ledger_owner_roots (
    repository_root TEXT PRIMARY KEY, generation INTEGER NOT NULL,
    policy_digest TEXT NOT NULL, synced_at_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ledger_resolution_tickets (
    ticket_hash TEXT PRIMARY KEY, repository_root TEXT NOT NULL,
    caller_digest TEXT NOT NULL, doc_id TEXT NOT NULL,
    request_json TEXT NOT NULL, grant_id TEXT, expires_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS ledger_tickets_root ON ledger_resolution_tickets(repository_root,expires_at_ms);
CREATE TABLE IF NOT EXISTS ledger_erasure_fences (
    repository_root TEXT NOT NULL, path_digest TEXT NOT NULL,
    erased_at_ms INTEGER NOT NULL, PRIMARY KEY(repository_root,path_digest)
);
"#;
/// Ticket store schema for the durable catalog. Resolution tickets are
/// runtime state, not index projection: they live beside the catalog's
/// erasure/exclusion records so retrieval on the read connection never opens
/// a write transaction against the index file. The legacy index-side table
/// above is retained for forward compatibility; new tickets are issued and
/// validated only through the catalog.
const TICKET_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS ledger_resolution_tickets (
    ticket_hash TEXT PRIMARY KEY, repository_root TEXT NOT NULL,
    caller_digest TEXT NOT NULL, doc_id TEXT NOT NULL,
    request_json TEXT NOT NULL, grant_id TEXT, expires_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS ledger_tickets_root ON ledger_resolution_tickets(repository_root,expires_at_ms);
"#;

static DAEMON_OWNER: OnceLock<Result<Arc<LedgerService>, String>> = OnceLock::new();

/// Called only when the tray-owned daemon installs its native operation owner.
/// Failure is isolated to Ledger; explicit requests use canonical request owners.
pub(crate) fn install_daemon_owner() {
    DAEMON_OWNER.get_or_init(|| LedgerDb::open_default().and_then(LedgerService::new).map(Arc::new));
}
pub(crate) fn active_owner() -> Result<Arc<LedgerService>, String> {
    DAEMON_OWNER.get().ok_or("membrane_unavailable:hub_inactive")?.clone()
}

/// Request-scoped owner; opens canonical storage without installing residency.
pub(crate) fn open_explicit_owner() -> Result<Arc<LedgerService>, String> {
    LedgerDb::open_default().and_then(LedgerService::new).map(Arc::new)
}

#[derive(Clone)]
pub(crate) struct Caller {
    pub root: String,
    registry_root: String,
    pub repository_id: String,
    pub scope_id: String,
    descriptor: Option<Value>,
    level: Option<String>,
}
impl Caller {
    pub(crate) fn from_arguments(arguments: &Value) -> Result<Self, String> {
        let get = |key: &str| arguments.pointer(&format!("/caller/{key}"))
            .and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_owned)
            .ok_or_else(|| "ledger_caller_required".to_owned());
        let root = resolve::normalized_root(Path::new(&get("root")?)).map_err(|e| e.to_string())?;
        let caller = Self { root, registry_root: get("root")?, repository_id: get("repositoryId")?, scope_id: get("scopeId")?,
            descriptor: arguments.pointer("/caller/scopeDescriptor").cloned(),
            level: arguments.get("taskGrantLevel").and_then(Value::as_str).map(str::to_owned) };
        if arguments.get("repository").and_then(Value::as_str) != Some(caller.repository_id.as_str()) {
            return Err("ledger_repository_binding_denied".into());
        }
        Ok(caller)
    }
    /// Native federation has already canonicalized the root. Its repository
    /// identifier still must match, and the source grant must exist in the
    /// installation registry; a request string is not source enrollment.
    pub(crate) fn enrolled(root: &Path, repository: &str) -> Result<Self, String> {
        let canonical = resolve::normalized_root(root).map_err(|e| e.to_string())?;
        if membrane_federation::root::canonical_repository_id(root) != repository {
            return Err("ledger_repository_binding_denied".into());
        }
        let registry = authorization::load_installation_registry().map_err(|e| e.to_string())?;
        let binding = registry.bindings().iter().find(|binding| {
            resolve::normalized_root(Path::new(&binding.root)).ok().as_deref() == Some(&canonical)
        }).ok_or("ledger_root_not_enrolled")?;
        Ok(Self { root: canonical, registry_root: binding.root.clone(), repository_id: binding.repository_id.clone(),
            scope_id: binding.scope_id.clone(), descriptor: binding.scope_descriptor.clone(), level: None })
    }
    pub(crate) fn authorize(&self, action: &str) -> Result<(), String> {
        authorization::authorize(&AuthorizationRequest {
            caller_root: &self.registry_root, caller_repository_id: &self.repository_id,
            caller_scope_id: &self.scope_id, caller_scope_descriptor: self.descriptor.as_ref(),
            target_repository: &self.repository_id, task_grant_level: self.level.as_deref(), action,
        }).map(|_| ()).map_err(|e| format!("{}:{}", e.code(), e))
    }
    pub(crate) fn envelope(&self) -> Value {
        let mut value = json!({"root":self.registry_root,"repositoryId":self.repository_id,"scopeId":self.scope_id});
        if let Some(descriptor) = &self.descriptor { value["scopeDescriptor"] = descriptor.clone(); }
        value
    }
    fn digest(&self) -> String {
        resolve::digest(format!("{}\0{}\0{}", self.root, self.repository_id, self.scope_id).as_bytes())
    }
}

pub(crate) struct LedgerService {
    db: LedgerDb,
    /// Second WAL connection to the same index file used by every read-only
    /// retrieval op. Published-index reads observe the last committed
    /// projection while maintenance holds a long write transaction — they
    /// never queue behind `operation` or wait out a sync pass. `None` for
    /// in-memory owners, which cannot open a second connection.
    read_db: Option<LedgerDb>,
    catalog: crate::catalog::ContextCatalog,
    operation: Mutex<()>,
}
struct ResetProgress<'a>(&'a LedgerDb);
impl Drop for ResetProgress<'_> {
    fn drop(&mut self) { let _ = self.0.lock().progress_handler(0, None::<fn() -> bool>); }
}
impl LedgerService {
    fn new(db: LedgerDb) -> Result<Self, String> {
        let catalog = crate::catalog::ContextCatalog::open(crate::catalog::default_catalog_path().map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        Self::with_catalog(db,catalog)
    }
    fn with_catalog(db:LedgerDb,catalog:crate::catalog::ContextCatalog)->Result<Self,String> {
        // Owner DDL is a write transaction; a concurrent maintenance pass may
        // hold the index writer for a whole corpus walk, so the batches run
        // only when their objects are actually missing (fresh index).
        if !Self::owner_objects_ready(&db)? {
            db.lock().execute_batch(super::diagnostics::SCHEMA).map_err(|e|e.to_string())?;
            db.lock().execute_batch(OWNER_SCHEMA).map_err(|e| e.to_string())?;
        }
        // `ledger_fts` activation is a host decision bound to this build, not
        // a property a caller or a stale persisted row can carry forward:
        //   * a build shipping a qualified owner/resolver composition receipt
        //     reconciles the activation row to that receipt;
        //   * otherwise a persisted `ledger_fts` activation survives only
        //     while the receipt that authorized it remains trusted — a stale
        //     activation degrades to `shadow`, never silently to unrestricted
        //     retrieval.
        let mode = index::recall_mode(&db)?;
        let stored = index::activation_receipt(&db)?;
        if let Some(receipt) = qualification::qualified_fts_activation() {
            let already = mode == index::LedgerRecallMode::LedgerFts
                && stored.as_deref() == Some(receipt.receipt_sha256.as_str());
            if !already {
                index::activate(&db, index::LedgerRecallMode::LedgerFts, Some(&receipt))?;
            }
        } else if mode == index::LedgerRecallMode::LedgerFts
            && !stored.as_deref().is_some_and(index::trusted_fts_receipt)
        {
            index::activate(&db, index::LedgerRecallMode::Shadow, None)?;
        }
        let read_db = db
            .path()
            .map(|path| LedgerDb::open_reader(path))
            .transpose()?;
        Ok(Self { db, read_db, catalog, operation: Mutex::new(()) })
    }

    /// True when the owner-layer objects (`diagnostics::SCHEMA` and
    /// `OWNER_SCHEMA`) already exist. Read-only so an open during maintenance
    /// never queues behind the writer for a no-op DDL batch.
    fn owner_objects_ready(db: &LedgerDb) -> Result<bool, String> {
        db.lock()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name IN (
                    'ledger_document_manifests','idx_ledger_manifest_document',
                    'ledger_resolution_tickets','ledger_tickets_root','ledger_erasure_fences')",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count == 5)
            .map_err(|error| error.to_string())
    }

    fn read_db(&self) -> &LedgerDb {
        self.read_db.as_ref().unwrap_or(&self.db)
    }
    #[cfg(test)]
    pub(crate) fn in_memory() -> Self { Self::with_catalog(LedgerDb::open_in_memory(),crate::catalog::ContextCatalog::open_in_memory()).unwrap() }

    /// Return normalized converted Markdown for the internal Cortex
    /// projection sink. This exposes no Ledger index or navigation state.
    pub(crate) fn converted_markdown(
        &self,
        doc_id: &str,
    ) -> Result<(String, String, String, String), String> {
        self.read_db()
            .lock()
            .query_row(
                "SELECT c.markdown,c.source_ref,c.markdown_sha256,c.source_revision
                 FROM ledger_document_conversions c
                 JOIN ledger_doc_artifacts a ON a.doc_id=c.doc_id
                 WHERE c.doc_id=?1",
                [doc_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "ledger_projection_source_missing".into())
    }

    fn run<T>(&self, caller: &Caller, action: &str, budget: &WorkBudget,
        work: impl FnOnce(&LedgerDb) -> Result<T, String>) -> Result<T, String>
    {
        caller.authorize(action)?;
        let _operation = loop {
            budget.check()?;
            match self.operation.try_lock() {
                Ok(guard) => break guard,
                Err(TryLockError::Poisoned(_)) => return Err("ledger_owner_unavailable".into()),
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(1)),
            }
        };
        let observed = budget.clone();
        let _ = self.db.lock().progress_handler(1000, Some(move || observed.interrupted()));
        let _reset = ResetProgress(&self.db);
        super::erasure::synchronize(&self.catalog,&self.db,&caller.root,budget)?;
        let result = work(&self.db);
        budget.check()?;
        // Revocation is checked again before anything leaves the owner.
        caller.authorize(action)?;
        result
    }

    /// Read-only retrieval lane: published-index reads on the dedicated WAL
    /// connection. Unlike `run`, this never takes the `operation` mutex, so a
    /// maintenance sync holding the writer for a whole corpus pass cannot
    /// starve status/recall/skill reads — each op observes the last committed
    /// publication and degrades typed on real gaps rather than reporting a
    /// blanket availability floor. Erasure still binds: durable catalog
    /// exclusions are mirrored into the connection's TEMP view by
    /// `synchronize_read` before any query work, so a catalog-recorded
    /// erasure cannot resurrect through a stale fence cache.
    fn run_read<T>(&self, caller: &Caller, action: &str, budget: &WorkBudget,
        work: impl FnOnce(&LedgerDb) -> Result<T, String>) -> Result<T, String>
    {
        let stage = std::time::Instant::now();
        caller.authorize(action)?;
        eprintln!("run_read[{action}] authorize {:?}", stage.elapsed());
        let db = self.read_db();
        let observed = budget.clone();
        let _ = db.lock().progress_handler(1000, Some(move || observed.interrupted()));
        let _reset = ResetProgress(db);
        let stage = std::time::Instant::now();
        super::erasure::synchronize_read(&self.catalog, db, &caller.root, budget)?;
        eprintln!("run_read[{action}] synchronize_read {:?}", stage.elapsed());
        let stage = std::time::Instant::now();
        let result = work(db);
        eprintln!("run_read[{action}] work {:?}", stage.elapsed());
        budget.check()?;
        // Revocation is checked again before anything leaves the owner.
        caller.authorize(action)?;
        result
    }

    /// Persisted projection coverage for `root`: true once a maintenance sync
    /// has published a `ledger_owner_roots` row. Retrieval reads this cheaply
    /// instead of re-walking the worktree — absence means the projection was
    /// never built, which callers see as the `ledger_index_unpublished`
    /// omission rather than silently empty results.
    fn index_published(db: &LedgerDb, root: &str) -> bool {
        db.lock()
            .query_row(
                "SELECT 1 FROM ledger_owner_roots WHERE repository_root=?1",
                [root],
                |_| Ok(()),
            )
            .optional()
            .unwrap_or(None)
            .is_some()
    }

    fn sync_locked(db: &LedgerDb, caller: &Caller, budget: &WorkBudget) -> Result<doc_spine::DocSyncReport, String> {
        let report = doc_spine::sync_bounded(db, Path::new(&caller.root), budget)?;
        db.lock().execute("INSERT INTO ledger_owner_roots VALUES (?1,?2,?3,?4)
            ON CONFLICT(repository_root) DO UPDATE SET generation=excluded.generation,
            policy_digest=excluded.policy_digest,synced_at_ms=excluded.synced_at_ms",
            params![caller.root, report.index_generation, report.policy_digest, crate::time::now_millis() as i64])
            .map_err(|e| e.to_string())?;
        Ok(report)
    }

    /// Authorized maintenance: reconcile the caller's enrolled root into the
    /// persisted projection. This is the same `sync_locked` index/update
    /// mechanism the explicit "sync" operation uses — invoked by the resident
    /// engine's reconcile pass under background authority and by direct
    /// maintenance calls, never by retrieval.
    pub(crate) fn maintain(&self, caller: &Caller, budget: &WorkBudget) -> Result<doc_spine::DocSyncReport, String> {
        self.run(caller, "context", budget, |db| Self::sync_locked(db, caller, budget))
    }

    pub(crate) fn search(&self, caller: &Caller, task: &str, k: usize, literal: bool,
        ranges: Option<Vec<ReadPathV1>>, grant_id: Option<&str>, budget: &WorkBudget)
        -> Result<(query::QueryResult, Vec<String>), String>
    {
        self.run_read(caller, "context", budget, |db| {
            validate_task_grant(grant_id, caller, None, None)?;
            // Retrieval queries persisted projections only — full-root
            // reconciliation is maintenance work owned by the explicit "sync"
            // operation and the resident reconcile pass, never a read path.
            // Selected sources are still validated against live bytes,
            // revision and span hash inside `query::search`/`issue_ticket`,
            // so edited or deleted content degrades to typed omissions rather
            // than leaking stale text.
            let mut result = query::search(db, &query::QueryScope { root: caller.root.clone(), ranges },
                task, k, literal, budget)?;
            if !Self::index_published(db, &caller.root) {
                result.omissions.push("ledger_index_unpublished".into());
                result.complete = false;
            }
            let mut tickets = Vec::new();
            for hit in &result.hits {
                budget.check()?;
                tickets.push(issue_ticket(&self.catalog, caller, hit, grant_id)?);
            }
            validate_task_grant(grant_id, caller, None, None)?;
            Ok((result, tickets))
        })
    }

    /// Index-only catalog of registered portable skill documents under this
    /// caller's enrolled source authority (LDG-032).
    ///
    /// The catalog mirrors `search` semantics: repository enrollment is the
    /// authority, an explicit task grant narrows enumeration to granted paths
    /// when present, and rows are read from the persisted projection —
    /// reconciliation is maintenance, not a per-request scan. Entries carry
    /// source identity, revision, content hash and generation only — never
    /// bodies — and `document_hits`/`issue_ticket` re-validate live source
    /// bytes, revision and spans before any materialization.
    pub(crate) fn skill_catalog(
        &self,
        caller: &Caller,
        ranges: Option<Vec<ReadPathV1>>,
        grant_id: Option<&str>,
        budget: &WorkBudget,
    ) -> Result<Vec<skill_documents::SkillDocumentEntryV1>, String> {
        self.run_read(caller, "context", budget, |db| {
            validate_task_grant(grant_id, caller, None, None)?;
            skill_documents::catalog(db, &caller.root, ranges.as_deref(), budget)
        })
    }

    /// Search portable skill documents through the existing scoped query path
    /// and issue a resolution ticket per emitted hit (LDG-032).
    ///
    /// `ranges` carries the caller's grant narrowing exactly as `search` does;
    /// the adapter intersects it with the registered skill-document set so a
    /// grant can narrow the lane but never widen it. Each returned pair is the
    /// complete `membrane_source_read` binding Pull needs for candidate
    /// materialization — hit plus owner-issued ticket.
    pub(crate) fn skill_documents(
        &self,
        caller: &Caller,
        task: &str,
        k: usize,
        ranges: Option<Vec<ReadPathV1>>,
        grant_id: Option<&str>,
        budget: &WorkBudget,
    ) -> Result<(query::QueryResult, Vec<skill_documents::TicketedSkillDocumentV1>), String> {
        self.run_read(caller, "context", budget, |db| {
            validate_task_grant(grant_id, caller, None, None)?;
            let outcome = skill_documents::search(db, &caller.root, task, k, ranges, budget)?;
            let mut skills = Vec::new();
            for skill in outcome.skills {
                budget.check()?;
                let ticket = issue_ticket(&self.catalog, caller, &skill.hit, grant_id)?;
                skills.push(skill_documents::TicketedSkillDocumentV1 {
                    skill_id: skill.skill_id,
                    title: skill.title,
                    hit: skill.hit,
                    ticket,
                });
            }
            validate_task_grant(grant_id, caller, None, None)?;
            Ok((outcome.result, skills))
        })
    }

    /// Materialize one catalog entry as ticketed top-level spans after the
    /// caller has selected it (LDG-032). Drift, erasure or policy loss fails
    /// closed before a ticket is issued, and a caller grant must cover the
    /// whole source — a grant may narrow selection, never widen it.
    pub(crate) fn skill_document_hits(
        &self,
        caller: &Caller,
        skill_id: &str,
        ranges: Option<Vec<ReadPathV1>>,
        grant_id: Option<&str>,
        budget: &WorkBudget,
    ) -> Result<Vec<skill_documents::TicketedSkillDocumentV1>, String> {
        self.run_read(caller, "context", budget, |db| {
            validate_task_grant(grant_id, caller, None, None)?;
            let entry = skill_documents::catalog(db, &caller.root, None, budget)?
                .into_iter()
                .find(|entry| entry.skill_id == skill_id)
                .ok_or("ledger_skill_missing")?;
            let skills = match &ranges {
                Some(ranges) => {
                    skill_documents::document_hits_granted(db, &caller.root, &entry, ranges, budget)?
                }
                None => skill_documents::document_hits(db, &caller.root, &entry, budget)?,
            };
            let mut ticketed = Vec::new();
            for skill in skills {
                budget.check()?;
                let ticket = issue_ticket(&self.catalog, caller, &skill.hit, grant_id)?;
                ticketed.push(skill_documents::TicketedSkillDocumentV1 {
                    skill_id: skill.skill_id,
                    title: skill.title,
                    hit: skill.hit,
                    ticket,
                });
            }
            validate_task_grant(grant_id, caller, None, None)?;
            Ok(ticketed)
        })
    }

    pub(crate) fn operation(&self, arguments: &Value, budget: &WorkBudget) -> Result<Value, String> {
        let caller = Caller::from_arguments(arguments)?;
        let operation = arguments.get("operation").and_then(Value::as_str).ok_or("ledger_operation_required")?;
        match operation {
            "recall" | "literal" => {
                let query = arguments.get("query").and_then(Value::as_str).ok_or("ledger_query_required")?;
                let k = arguments.get("k").and_then(Value::as_u64).unwrap_or(6) as usize;
                let grant_id = arguments.get("scopeGrantId").and_then(Value::as_str);
                let ranges = if let Some(grant_id) = grant_id {
                    let task_id = arguments.get("taskId").and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .ok_or("ledger_task_id_required")?;
                    let session_id = arguments.get("sessionId").and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .ok_or("ledger_session_id_required")?;
                    validate_task_grant(Some(grant_id), &caller, Some(task_id), Some(session_id))?;
                    let grant = crate::catalog::lookup_grant(&self.catalog, grant_id)
                        .map_err(|e|e.to_string())?.ok_or("ledger_scope_grant_missing")?;
                    if grant.read_paths.is_empty() { return Err("ledger_scope_ranges_unavailable".into()); }
                    Some(grant.read_paths)
                } else { None };
                let (result, tickets) = self.search(&caller, query, k, operation == "literal", ranges, grant_id, budget)?;
                let mut value = serde_json::to_value(result).map_err(|e| e.to_string())?;
                if let Some(hits) = value.get_mut("hits").and_then(Value::as_array_mut) {
                    for (hit, ticket) in hits.iter_mut().zip(tickets) { hit["ledgerTicket"] = json!(ticket); }
                }
                Ok(value)
            }
            "ingest" => self.run(&caller, "context", budget, |db| {
                let grant_id = required_string(arguments, "scopeGrantId")?;
                let task_id = required_string(arguments, "taskId")?;
                let session_id = required_string(arguments, "sessionId")?;
                validate_task_grant(Some(&grant_id), &caller, Some(&task_id), Some(&session_id))?;
                let grant = crate::catalog::lookup_grant(&self.catalog, &grant_id)
                    .map_err(|error| error.to_string())?
                    .ok_or("ledger_scope_grant_missing")?;
                let path = required_string(arguments, "path")?.replace('\\', "/");
                if grant.read_paths.is_empty()
                    || !grant.read_paths.iter().any(|range| range.path == path)
                {
                    return Err("ledger_scope_path_denied".into());
                }
                let format = parse_document_format(arguments.get("format"))?;
                let raw_input = parse_raw_input(arguments.get("rawInput"))?;
                let maximum = arguments
                    .get("maxRawBytes")
                    .and_then(Value::as_u64)
                    .unwrap_or(8 * 1024 * 1024)
                    .clamp(1, 8 * 1024 * 1024) as usize;
                if raw_input.len() > maximum {
                    return Err(format!("conversion_input_too_large:{}:{}", raw_input.len(), maximum));
                }
                let source_ref = required_string(arguments, "sourceRef")?;
                if source_ref.starts_with("live:") || source_ref.starts_with("live://") {
                    return Err("ledger_live_source_requires_freshness".into());
                }
                if !(source_ref.starts_with("snapshot:")
                    || source_ref.starts_with("source://")
                    || source_ref.starts_with("doc://"))
                    || source_ref.contains("..")
                {
                    return Err("ledger_source_ref_denied".into());
                }
                let revision = arguments
                    .get("sourceRevision")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("snapshot:{}", resolve::digest(&raw_input)));
                let title = arguments
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(&path)
                    .to_owned();
                let conversion_grant = crate::ledger::document_conversion::DocumentConversionGrantV1::new(
                    [format.clone()], maximum,
                );
                let artifact = doc_spine::ingest_granted_document(
                    db,
                    &conversion_grant,
                    doc_spine::GrantedDocumentIngestV1 {
                        repository_root: caller.root.clone(),
                        repository_id: caller.repository_id.clone(),
                        revision,
                        path,
                        title,
                        document: crate::ledger::document_conversion::DocumentConversionInputV1 {
                            source_ref,
                            format,
                            raw_input,
                        },
                    },
                )?;
                validate_task_grant(Some(&grant_id), &caller, Some(&task_id), Some(&session_id))?;
                let conversion: Value = db.lock().query_row(
                    "SELECT source_ref,input_format,raw_sha256,markdown_sha256,converter,converter_version,config_digest,losses_json,omissions_json FROM ledger_document_conversions WHERE doc_id=?1",
                    [&artifact.doc_id],
                    |row| {
                        let losses: String = row.get(7)?;
                        let omissions: String = row.get(8)?;
                        Ok(json!({
                            "schemaVersion":"ledger.converted-document.v2",
                            "sourceRef":row.get::<_,String>(0)?,
                            "inputFormat":row.get::<_,String>(1)?,
                            "rawSha256":row.get::<_,String>(2)?,
                            "normalizedSha256":row.get::<_,String>(3)?,
                            "converter":row.get::<_,String>(4)?,
                            "converterVersion":row.get::<_,String>(5)?,
                            "configDigest":row.get::<_,String>(6)?,
                            "losses":serde_json::from_str::<Value>(&losses).unwrap_or_else(|_| json!([])),
                            "omissions":serde_json::from_str::<Value>(&omissions).unwrap_or_else(|_| json!([])),
                            "sourceKind":if row.get::<_,String>(0)?.starts_with("live:") || row.get::<_,String>(0)?.starts_with("live://") {"live"} else {"snapshot"}
                        }))
                    },
                ).map_err(|error| error.to_string())?;
                Ok(json!({"artifact":artifact,"conversion":conversion}))
            }),
            "sync" => self.run(&caller, "context", budget, |db| {
                serde_json::to_value(Self::sync_locked(db, &caller, budget)?).map_err(|e| e.to_string())
            }),
            "status" => self.run_read(&caller, "system_status", budget, |db| {
                let (active, total): (i64, i64) = db.lock().query_row(
                    "SELECT COALESCE(SUM(lifecycle_state='active' AND sensitivity='normal'),0),COUNT(*)
                     FROM ledger_doc_artifacts WHERE repository_root=?1", [&caller.root], |r| Ok((r.get(0)?,r.get(1)?)))
                    .map_err(|e| e.to_string())?;
                let state = db.lock().query_row("SELECT generation,policy_digest,synced_at_ms FROM ledger_owner_roots WHERE repository_root=?1",
                    [&caller.root], |r| Ok(json!({"generation":r.get::<_,i64>(0)?,"policyDigest":r.get::<_,String>(1)?,"syncedAtMs":r.get::<_,i64>(2)?})))
                    .optional().map_err(|e| e.to_string())?;
                Ok(json!({"schemaVersion":1,"serviceVersion":SERVICE_VERSION,"resolverVersion":RESOLVER_VERSION,
                    "owner":"tray-daemon","repositoryId":caller.repository_id,"enrolled":true,
                    "indexState":if state.is_some(){"published"}else{"not_indexed"},"publication":state,
                    "activeDocuments":active,"registeredDocuments":total,"mode":index::recall_mode(db)?.storage_name(),
                    "providerDelivery":"direct_pull","runtimeQualified":false,
                    "literalMatch":"source_bytes","cursorSupported":true,"sourceByteLimit":resolve::MAX_SOURCE_BYTES}))
            }),
            "outline" => self.run_read(&caller, "source_read", budget, |db| {
                let path = arguments.get("path").and_then(Value::as_str).ok_or("ledger_path_required")?;
                permitted_path(db, &caller.root, path, budget)?;
                let bytes = resolve::confined_bytes(Path::new(&caller.root), path).map_err(|e| e.to_string())?;
                budget.charge_bytes(bytes.len())?;
                let markdown = String::from_utf8(bytes).map_err(|_| "ledger_unsupported_encoding")?;
                let page = arguments.get("maxSections").and_then(Value::as_u64).unwrap_or(128).clamp(1,256) as usize;
                let outline = super::outline::build_outline_page(&format!("doc://repo/worktree/{path}"), &markdown,
                    "comrak-0.54.0", page, arguments.get("continuationCursor").and_then(Value::as_str)).map_err(|e| e.to_string())?;
                permitted_path(db, &caller.root, path, budget)?;
                serde_json::to_value(outline).map_err(|e| e.to_string())
            }),
            "activate" => self.run(&caller, "checkpoint", budget, |db| {
                let mode = match arguments.get("mode").and_then(Value::as_str) {
                    Some("legacy_scan") => index::LedgerRecallMode::LegacyScan,
                    Some("shadow") => index::LedgerRecallMode::Shadow,
                    // Existing allowlisted evidence qualifies the old query path,
                    // not the newly scoped owner/resolver composition.
                    Some("ledger_fts") => return Err("ledger_owner_qualification_required".into()),
                    _ => return Err("ledger_mode_invalid".into()),
                };
                index::activate(db, mode, None)?;
                Ok(json!({"mode":mode.storage_name(),"providerDelivery":"direct_pull"}))
            }),
            "erase" => self.run(&caller, "checkpoint", budget, |db| erase(db, &self.catalog, &caller, arguments)),
            "backlinks" | "related" | "manifests" | "drift" => self.run_read(&caller, "context", budget, |db| {
                let doc = required_string(arguments,"docId")?;
                match operation {
                    "backlinks" => super::diagnostics::backlinks(db,&caller.root,&doc,arguments.get("nodeId").and_then(Value::as_str),
                        arguments.get("limit").and_then(Value::as_u64).unwrap_or(64) as usize,budget),
                    "related" => super::diagnostics::related(db,&caller.root,&doc,&required_string(arguments,"nodeId")?,
                        arguments.get("limit").and_then(Value::as_u64).unwrap_or(64) as usize,budget),
                    "manifests" => super::diagnostics::manifests(db,&caller.root,&doc,budget),
                    _ => super::diagnostics::drift(db,&caller.root,&doc,&required_string(arguments,"fromManifest")?,&required_string(arguments,"toManifest")?,budget),
                }
            }),
            // LDG-032 portable skill-document adapter lane. Same grant
            // resolution as "recall": a scope grant narrows to granted paths
            // and binds tickets; without one, repository enrollment is the
            // authority. Every hit carries the membrane_source_read binding.
            "skillCatalog" | "skillDocuments" | "skillDocument" => {
                let grant_id = arguments.get("scopeGrantId").and_then(Value::as_str);
                let ranges = if let Some(grant_id) = grant_id {
                    let task_id = arguments.get("taskId").and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .ok_or("ledger_task_id_required")?;
                    let session_id = arguments.get("sessionId").and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .ok_or("ledger_session_id_required")?;
                    validate_task_grant(Some(grant_id), &caller, Some(task_id), Some(session_id))?;
                    let grant = crate::catalog::lookup_grant(&self.catalog, grant_id)
                        .map_err(|e|e.to_string())?.ok_or("ledger_scope_grant_missing")?;
                    if grant.read_paths.is_empty() { return Err("ledger_scope_ranges_unavailable".into()); }
                    Some(grant.read_paths)
                } else { None };
                match operation {
                    "skillCatalog" => {
                        let entries = self.skill_catalog(&caller, ranges, grant_id, budget)?;
                        Ok(json!({"schemaVersion":"ledger.skill-catalog.v1","skills":entries}))
                    }
                    "skillDocuments" => {
                        let query = arguments.get("query").and_then(Value::as_str).ok_or("ledger_query_required")?;
                        let k = arguments.get("k").and_then(Value::as_u64).unwrap_or(6) as usize;
                        let (result, skills) = self.skill_documents(&caller, query, k, ranges, grant_id, budget)?;
                        let mut value = serde_json::to_value(result).map_err(|e| e.to_string())?;
                        let mut entries = Vec::with_capacity(skills.len());
                        for skill in skills {
                            let mut hit = serde_json::to_value(&skill.hit).map_err(|e| e.to_string())?;
                            hit["ledgerTicket"] = json!(skill.ticket);
                            entries.push(json!({"skillId":skill.skill_id,"title":skill.title,"hit":hit}));
                        }
                        value["skills"] = json!(entries);
                        Ok(value)
                    }
                    _ => {
                        let skill_id = required_string(arguments, "skillId")?;
                        let skills = self.skill_document_hits(&caller, &skill_id, ranges, grant_id, budget)?;
                        let mut entries = Vec::with_capacity(skills.len());
                        let mut title = String::new();
                        for skill in skills {
                            title = skill.title.clone();
                            let mut hit = serde_json::to_value(&skill.hit).map_err(|e| e.to_string())?;
                            hit["ledgerTicket"] = json!(skill.ticket);
                            entries.push(hit);
                        }
                        Ok(json!({"schemaVersion":"ledger.skill-document.v1",
                            "skillId":skill_id,"title":title,"hits":entries}))
                    }
                }
            }
            _ => Err("ledger_operation_unsupported".into()),
        }
    }

    pub(crate) fn read(&self, arguments: &Value, budget: &WorkBudget) -> Result<Value, String> {
        let caller = Caller::from_arguments(arguments)?;
        let session_id = optional_string(arguments,"sessionId");
        let request = ResolveRequest {
            doc_id: optional_string(arguments,"docId"), node_id: optional_string(arguments,"nodeId"),
            source_ref: required_string(arguments,"sourceRef")?, anchor_id: required_string(arguments,"anchorId")?,
            expected_content_hash: required_string(arguments,"expectedContentHash")?,
            expected_revision: optional_string(arguments,"expectedRevision"), expected_span_hash: optional_string(arguments,"expectedSpanHash"),
            ledger_generation: arguments.get("ledgerGeneration").and_then(Value::as_i64),
            continuation_cursor: optional_string(arguments,"continuationCursor"),
            max_bytes: arguments.get("maxBytes").and_then(Value::as_u64).unwrap_or(12_000).min(12_000) as usize,
        };
        self.run_read(&caller, "source_read", budget, |db| {
            let ticket = arguments.get("ledgerTicket").and_then(Value::as_str);
            if request.node_id.is_some() || request.anchor_id.starts_with("ledger.node:") || request.source_ref.starts_with("ledger://") {
                validate_ticket(&self.catalog, &caller, ticket.ok_or("ledger_ticket_required")?, &request, session_id.as_deref())?;
            }
            if let Some(doc)=request.doc_id.as_deref().or_else(||request.source_ref.strip_prefix("ledger://doc/")) {
                let path:String=db.lock().query_row("SELECT path FROM ledger_doc_artifacts WHERE repository_root=?1 AND doc_id=?2",
                    params![caller.root,doc],|r|r.get(0)).map_err(|_|"ledger_source_missing")?;
                permitted_path(db,&caller.root,&path,budget)?;
            }
            if let Ok(reference) = super::identifier::WorktreeDocRef::parse(&request.source_ref) {
                permitted_path(db, &caller.root, reference.relative_path(), budget)?;
            }
            let result = match resolve::resolve(db, Path::new(&caller.root), &request) {
                Ok(result) => {
                    budget.charge_bytes(result.read.content.len())?;
                    json!({"ok":true,"contentSha256":result.projection_content_hash,"sourceRef":request.source_ref,
                        "section":result.read,"rawContentHash":result.raw_content_hash,"sourceKind":result.source_kind,
                        "sourceRevision":result.source_revision,"ledgerGeneration":result.ledger_generation,
                        "docId":result.doc_id,"nodeId":result.node_id,"converter":result.converter,
                        "losses":result.losses,"omissions":result.omissions,"resolverVersion":RESOLVER_VERSION})
                }
                Err(resolve::ResolveError::Missing) if ticket.is_none() && request.doc_id.is_none() && request.node_id.is_none()
                    && !request.anchor_id.starts_with("ledger.node:") => {
                    // Preserve the authorized known-section reader without
                    // silently registering sources or bypassing erasure fences.
                    let reference = super::identifier::WorktreeDocRef::parse(&request.source_ref).map_err(|_| "ledger_denied")?;
                    let bytes = resolve::confined_bytes(Path::new(&caller.root), reference.relative_path()).map_err(|e| e.to_string())?;
                    budget.charge_bytes(bytes.len())?;
                    let markdown = String::from_utf8(bytes).map_err(|_| "ledger_unsupported_encoding")?;
                    let expected = request.expected_content_hash.strip_prefix("sha256:").unwrap_or(&request.expected_content_hash);
                    let read = super::outline::read_section_with_cursor(&request.source_ref,&markdown,&request.anchor_id,
                        expected,request.max_bytes,request.continuation_cursor.as_deref()).map_err(|e| e.to_string())?;
                    json!({"ok":true,"contentSha256":read.content_hash,"section":read,"sourceRef":request.source_ref,
                        "sourceKind":"worktree","registered":false,"resolverVersion":RESOLVER_VERSION})
                }
                Err(error) => return Err(error.to_string()),
            };
            if let Some(ticket) = ticket { validate_ticket(&self.catalog, &caller, ticket, &request, session_id.as_deref())?; }
            if let Some(doc)=request.doc_id.as_deref().or_else(||request.source_ref.strip_prefix("ledger://doc/")) {
                let path:String=db.lock().query_row("SELECT path FROM ledger_doc_artifacts WHERE repository_root=?1 AND doc_id=?2",
                    params![caller.root,doc],|r|r.get(0)).map_err(|_|"ledger_source_missing")?;
                permitted_path(db,&caller.root,&path,budget)?;
            }
            if let Ok(reference) = super::identifier::WorktreeDocRef::parse(&request.source_ref) {
                permitted_path(db, &caller.root, reference.relative_path(), budget)?;
            }
            Ok(result)
        })
    }
}

fn required_string(value: &Value, field: &str) -> Result<String, String> {
    optional_string(value,field).filter(|s| !s.trim().is_empty()).ok_or_else(|| format!("ledger_{field}_required"))
}
fn optional_string(value: &Value, field: &str) -> Option<String> { value.get(field).and_then(Value::as_str).map(str::to_owned) }

fn parse_document_format(value: Option<&Value>) -> Result<crate::ledger::document_conversion::DocumentInputFormatV1, String> {
    let value = value.and_then(Value::as_str).ok_or("ledger_format_required")?;
    Ok(match value {
        "plain_text" | "plaintext" | "text" => crate::ledger::document_conversion::DocumentInputFormatV1::PlainText,
        "json" => crate::ledger::document_conversion::DocumentInputFormatV1::Json,
        "html" | "htm" => crate::ledger::document_conversion::DocumentInputFormatV1::Html,
        "pdf" => crate::ledger::document_conversion::DocumentInputFormatV1::Pdf,
        "docx" => crate::ledger::document_conversion::DocumentInputFormatV1::Docx,
        value if value.starts_with("media:") => crate::ledger::document_conversion::DocumentInputFormatV1::Media(value[6..].to_owned()),
        value => crate::ledger::document_conversion::DocumentInputFormatV1::Other(value.to_owned()),
    })
}

fn parse_raw_input(value: Option<&Value>) -> Result<Vec<u8>, String> {
    let value = value.ok_or("ledger_raw_input_required")?;
    match value {
        Value::String(value) => Ok(value.as_bytes().to_vec()),
        Value::Array(values) => values
            .iter()
            .map(|value| value.as_u64().filter(|byte| *byte <= 255).ok_or_else(|| "ledger_raw_input_invalid".to_owned()).map(|byte| byte as u8))
            .collect(),
        _ => Err("ledger_raw_input_invalid".into()),
    }
}

pub(crate) fn validate_task_grant(id: Option<&str>, caller: &Caller, task: Option<&str>, session: Option<&str>) -> Result<(), String> {
    let Some(id) = id else { return Ok(()); };
    let catalog = crate::catalog::ContextCatalog::open(crate::catalog::default_catalog_path().map_err(|e|e.to_string())?)
        .map_err(|e| e.to_string())?;
    let grant = crate::catalog::lookup_grant(&catalog,id).map_err(|e| e.to_string())?.ok_or("ledger_scope_grant_missing")?;
    let canonical = membrane_federation::root::canonical_repository_id(Path::new(&caller.root));
    if !grant.permits() || !grant.repository_ids.iter().any(|id| id == &caller.repository_id || id == &canonical)
        || !grant.permitted_edge_types.iter().any(|edge| edge == "source_read")
        || task.is_some_and(|task| grant.task_id != task) || session.is_some_and(|session| grant.session_id != session)
    { return Err("ledger_scope_grant_invalid".into()); }
    Ok(())
}

pub(crate) fn permitted_path(db: &LedgerDb, root: &str, path: &str, budget: &WorkBudget) -> Result<(), String> {
    {
        let conn = db.lock();
        super::erasure::ensure_read_exclusions(&conn)?;
        let erased: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM ledger_erasure_fences WHERE repository_root=?1 AND path_digest=?2)
            OR EXISTS(SELECT 1 FROM ledger_read_exclusions WHERE path_digest=?2)",
            params![root,resolve::digest(path.as_bytes())],|r|r.get(0)).map_err(|e| e.to_string())?;
        if erased { return Err("ledger_source_erased".into()); }
    }
    let mut policy = SourcePolicy::new(Path::new(root))?;
    if !policy.allows(path,false,budget)? { return Err("ledger_source_ineligible".into()); }
    policy.revalidate(budget)
}

/// Resolution tickets live in the durable catalog, not the index file. A
/// ticket write is runtime state, not projection data: keeping it off the
/// index means a search lane on the read connection never needs a write
/// transaction there, so retrieval stays responsive while maintenance holds
/// the index writer. The catalog's own lock serializes its brief writes.
fn issue_ticket(catalog: &crate::catalog::ContextCatalog, caller: &Caller, hit: &query::LedgerHit, grant_id: Option<&str>) -> Result<String, String> {
    let mut random = [0u8;32];
    getrandom::fill(&mut random).map_err(|_| "ledger_ticket_entropy_unavailable")?;
    let ticket = format!("ledger-ticket:{}",hex::encode(random));
    let now = crate::time::now_millis() as i64;
    let conn = catalog.lock();
    conn.execute_batch(TICKET_SCHEMA).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM ledger_resolution_tickets WHERE expires_at_ms<=?1",[now]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM ledger_resolution_tickets WHERE ticket_hash IN (
        SELECT ticket_hash FROM ledger_resolution_tickets WHERE repository_root=?1 ORDER BY expires_at_ms DESC LIMIT -1 OFFSET 480)",
        [&caller.root]).map_err(|e|e.to_string())?;
    conn.execute("INSERT INTO ledger_resolution_tickets VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![resolve::digest(ticket.as_bytes()),caller.root,caller.digest(),hit.doc_id,
            serde_json::to_string(&hit.resolve_request()).map_err(|e|e.to_string())?,grant_id,now+TICKET_TTL_MS])
        .map_err(|e| e.to_string())?;
    Ok(ticket)
}
fn validate_ticket(catalog: &crate::catalog::ContextCatalog, caller: &Caller, ticket: &str, request: &ResolveRequest, session_id: Option<&str>) -> Result<(), String> {
    if ticket.len() != 78 || !ticket.starts_with("ledger-ticket:") { return Err("ledger_ticket_invalid".into()); }
    let (request_json, grant): (String,Option<String>) = {
        let conn = catalog.lock();
        conn.execute_batch(TICKET_SCHEMA).map_err(|e| e.to_string())?;
        conn.query_row(
        "SELECT request_json,grant_id FROM ledger_resolution_tickets WHERE ticket_hash=?1 AND repository_root=?2
         AND caller_digest=?3 AND expires_at_ms>?4",params![resolve::digest(ticket.as_bytes()),caller.root,caller.digest(),crate::time::now_millis() as i64],
        |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?.ok_or("ledger_ticket_expired_or_denied")?
    };
    let expected: ResolveRequest = serde_json::from_str(&request_json).map_err(|_| "ledger_ticket_invalid")?;
    if expected.doc_id != request.doc_id || expected.node_id != request.node_id
        || expected.source_ref != request.source_ref || expected.anchor_id != request.anchor_id
        || expected.expected_content_hash != request.expected_content_hash
        || expected.expected_revision != request.expected_revision || expected.expected_span_hash != request.expected_span_hash
        || expected.ledger_generation != request.ledger_generation
    { return Err("ledger_ticket_binding_mismatch".into()); }
    if grant.is_some() {
        validate_task_grant(grant.as_deref(), caller, None, Some(session_id.ok_or("ledger_session_id_required")?))
    } else {
        validate_task_grant(None, caller, None, None)
    }
}

fn erase(db: &LedgerDb, catalog: &crate::catalog::ContextCatalog, caller: &Caller, arguments: &Value) -> Result<Value, String> {
    let doc_id = required_string(arguments,"docId")?;
    let expected = required_string(arguments,"expectedContentHash")?;
    let mut conn = db.lock();
    let tx = conn.transaction().map_err(|e|e.to_string())?;
    let (path, hash): (String,String) = tx.query_row("SELECT path,content_hash FROM ledger_doc_artifacts WHERE repository_root=?1 AND doc_id=?2",
        params![caller.root,doc_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?.ok_or("ledger_source_missing")?;
    if !resolve::hash_matches(&expected,&hash) { return Err("ledger_source_stale".into()); }
    let identities:i64=tx.query_row("SELECT COUNT(*) FROM ledger_doc_artifacts WHERE doc_id=?1",[&doc_id],|r|r.get(0)).map_err(|e|e.to_string())?;
    if identities!=1 {return Err("ledger_ambiguous_source_identity".into());}
    super::erasure::record(catalog,&caller.root,&resolve::digest(path.as_bytes()))?;
    tx.execute("INSERT OR REPLACE INTO ledger_erasure_fences VALUES (?1,?2,?3)",
        params![caller.root,resolve::digest(path.as_bytes()),crate::time::now_millis() as i64]).map_err(|e|e.to_string())?;
    for table in ["ledger_node_fts","ledger_nodes","ledger_index_publications","ledger_query_alias_evidence",
        "ledger_query_aliases","ledger_document_conversions","ledger_resolution_tickets","ledger_document_manifests"] {
        tx.execute(&format!("DELETE FROM {table} WHERE doc_id=?1"),[&doc_id]).map_err(|e|e.to_string())?;
    }
    tx.execute("DELETE FROM ledger_doc_projections WHERE parent_doc_id=?1",[&doc_id]).map_err(|e|e.to_string())?;
    tx.execute("DELETE FROM ledger_link_targets WHERE source_doc_id=?1 OR target_doc_id=?1",[&doc_id]).map_err(|e|e.to_string())?;
    tx.execute("DELETE FROM ledger_doc_artifacts WHERE repository_root=?1 AND doc_id=?2",params![caller.root,doc_id]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())?;
    // Resolution tickets are runtime state in the durable catalog; erasing a
    // source invalidates every ticket bound to it so a held ticket cannot
    // resurrect a deleted span.
    {
        let conn = catalog.lock();
        conn.execute_batch(TICKET_SCHEMA).map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM ledger_resolution_tickets WHERE repository_root=?1 AND doc_id=?2",
            params![caller.root, doc_id]).map_err(|e| e.to_string())?;
    }
    Ok(json!({"schemaVersion":1,"operation":"erase","docId":doc_id,"logicalProjectionErasure":true,
        "sourceFilesChanged":false,"physicalErasure":"not_claimed","automaticReindexFenced":true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Serialization for the process-global registry env var so parallel test
    /// binaries cannot interleave two registries inside one service check.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn enrolled_workspace(dir: &tempfile::TempDir) -> (std::path::PathBuf, String) {
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(workspace.join("docs")).unwrap();
        fs::write(
            workspace.join("docs/guide.md"),
            "# Guide\n\nneedle alpha content\n\n## Child\nchild needle\n",
        )
        .unwrap();
        fs::write(workspace.join("docs/target.md"), "# Target\n\nneedle beta\n").unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let registry_path = dir.path().join("registry.json");
        fs::write(
            &registry_path,
            serde_json::json!({
                "schema_version":2,
                "bindings":{workspace.to_string_lossy().as_ref():{
                    "repository_id":"repo-contention","scope_id":"installation-scope",
                    "grant_policy":{"level":"read-only"}}}
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_var("MEMBRANE_PROJECT_REGISTRY", &registry_path);
        (workspace, "repo-contention".to_owned())
    }

    fn caller(workspace: &std::path::Path, repository: &str) -> Caller {
        Caller::from_arguments(&json!({
            "repository":repository,
            "caller":{"root":workspace.to_string_lossy(),"repositoryId":repository,"scopeId":"installation-scope"}
        }))
        .unwrap()
    }

    /// Retrieval/maintenance separation: while the writer connection holds one
    /// open transaction — exactly the shape `sync_bounded` holds across a whole
    /// corpus walk — published-index reads on the dedicated WAL connection must
    /// return committed hits rather than queue behind maintenance and starve
    /// into `ledger_deadline_exhausted`. Erasure still binds mid-transaction:
    /// catalog exclusions are mirrored into the read connection's TEMP view,
    /// so an erased source cannot be recalled through a stale fence cache.
    /// Tickets are catalog writes, so issuance cannot contend with the index.
    #[test]
    fn published_reads_complete_while_maintenance_holds_writer() {
        let _env = env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (workspace, repository) = enrolled_workspace(&dir);
        let index_path = dir.path().join("ledger-index.sqlite3");
        let db = LedgerDb::open(&index_path).unwrap();
        let budget = WorkBudget::bounded(Duration::from_secs(30));
        let report = doc_spine::sync_bounded(&db, &workspace, &budget).unwrap();
        let catalog = crate::catalog::ContextCatalog::open(dir.path().join("catalog.db")).unwrap();
        let service = LedgerService::with_catalog(db, catalog).unwrap();
        // Mirror `sync_locked`'s owner-root publication row so `index_published`
        // reflects the maintenance-completed state rather than a fresh index.
        service
            .db
            .lock()
            .execute(
                "INSERT INTO ledger_owner_roots VALUES (?1,?2,?3,?4)",
                params![workspace.to_string_lossy().replace('\\', "/"), report.index_generation,
                    report.policy_digest, crate::time::now_millis() as i64],
            )
            .unwrap();
        let caller = caller(&workspace, &repository);

        // The pre-change starvation shape: the writer holds one open write
        // transaction exactly as `sync_bounded` does across a corpus walk.
        let mut writer = service.db.lock();
        let tx = writer.transaction().unwrap();
        tx.execute_batch(
            "INSERT OR IGNORE INTO ledger_erasure_fences VALUES ('maintenance','probe',0)",
        )
        .unwrap();

        let read_budget = WorkBudget::bounded(Duration::from_secs(5));
        let (result, tickets) = service
            .search(&caller, "needle alpha", 4, false, None, None, &read_budget)
            .expect("published-index read must complete while maintenance holds the writer");
        assert!(
            result.hits.iter().any(|hit| hit.source_ref.contains("guide.md")),
            "read must observe the committed publication, not starve: {result:?}"
        );
        assert_eq!(tickets.len(), result.hits.len());
        assert!(result.omissions.is_empty(), "{result:?}");

        // A catalog exclusion recorded while the writer transaction is open is
        // still enforced on the read connection via its TEMP exclusion view.
        let erased_digest = resolve::digest(b"docs/guide.md");
        super::super::erasure::record(&service.catalog, &caller.root, &erased_digest).unwrap();
        let (excluded, _) = service
            .search(&caller, "needle alpha", 4, false, None, None, &read_budget)
            .unwrap();
        assert!(
            excluded.hits.iter().all(|hit| !hit.source_ref.contains("guide.md")),
            "catalog-recorded erasure must bind reads even mid-maintenance: {excluded:?}"
        );

        tx.rollback().unwrap();
        drop(writer);
    }

    /// A persisted `ledger_fts` activation survives an owner open only while
    /// the receipt that authorized it remains trusted by this build; a stale
    /// or receipt-less activation degrades to `shadow` rather than running an
    /// unqualified retrieval lane.
    #[test]
    fn stale_fts_activation_degrades_at_open() {
        let _env = env_lock();
        let dir = tempfile::tempdir().unwrap();
        let index_path = dir.path().join("ledger-index.sqlite3");
        {
            let db = LedgerDb::open(&index_path).unwrap();
            db.lock()
                .execute(
                    "UPDATE ledger_activation SET mode='ledger_fts', qualification_receipt_sha256='0000000000000000000000000000000000000000000000000000000000000000'",
                    [],
                )
                .unwrap();
        }
        let catalog = crate::catalog::ContextCatalog::open(dir.path().join("catalog.db")).unwrap();
        let db = LedgerDb::open(&index_path).unwrap();
        let _service = LedgerService::with_catalog(db, catalog).unwrap();
        assert_eq!(
            index::recall_mode(&_service.db).unwrap(),
            index::LedgerRecallMode::Shadow,
            "untrusted persisted activation must degrade to shadow at open"
        );
    }
}
